//! menagerie-agent: reports this machine's health and configuration
//! revision over HTTP. Read-only: it never runs anything on request.
//!
//!     menagerie-agent [--listen ADDR]   serve /status and /metrics
//!     menagerie-agent --once            print one /status as JSON and exit
//!
//! ADDR defaults to 127.0.0.1:9184 (or $MENAGERIE_AGENT_LISTEN). Publish it
//! with `tailscale serve`, or listen on the machine's Tailscale address.

mod metrics;
mod probes;
mod revision;
mod snapshot;

use std::net::SocketAddr;

use anyhow::Context;
use axum::Router;
use axum::extract::State;
use axum::http::header;
use axum::response::{IntoResponse, Json};
use axum::routing::get;

use snapshot::{Prober, Shared};

const DEFAULT_LISTEN: &str = "127.0.0.1:9184";

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let mut listen = std::env::var("MENAGERIE_AGENT_LISTEN").ok();
    let mut once = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--listen" => listen = Some(args.next().context("--listen needs an address")?),
            "--once" => once = true,
            "--version" => {
                println!("menagerie-agent {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "-h" | "--help" => {
                println!("usage: menagerie-agent [--listen ADDR] [--once]");
                return Ok(());
            }
            other => anyhow::bail!("unknown argument {other:?}; see --help"),
        }
    }

    let shared = Shared::new();
    let mut prober = Prober::new(shared.clone(), revision::Roots::system());
    // Fill the cache before the first request, so nothing reads empty.
    prober.run_all();

    if once {
        println!("{}", serde_json::to_string_pretty(&shared.status())?);
        return Ok(());
    }

    std::thread::Builder::new()
        .name("probes".into())
        .spawn(move || prober.run_forever())?;

    let addr: SocketAddr = listen
        .as_deref()
        .unwrap_or(DEFAULT_LISTEN)
        .parse()
        .context("--listen must be an address such as 127.0.0.1:9184")?;
    let app = Router::new()
        .route("/", get(index))
        .route("/status", get(status))
        .route("/metrics", get(metrics_text))
        .with_state(shared);

    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async move {
            let listener = tokio::net::TcpListener::bind(addr)
                .await
                .with_context(|| format!("listen on {addr}"))?;
            eprintln!("menagerie-agent listening on http://{addr}");
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = tokio::signal::ctrl_c().await;
                })
                .await?;
            anyhow::Ok(())
        })
}

async fn index() -> &'static str {
    "menagerie-agent: GET /status (JSON) or /metrics (Prometheus)\n"
}

async fn status(State(shared): State<Shared>) -> impl IntoResponse {
    Json(shared.status())
}

async fn metrics_text(State(shared): State<Shared>) -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        metrics::render(&shared.status()),
    )
}
