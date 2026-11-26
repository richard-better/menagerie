//! Render the window and the popover and save a PNG of each. No menu bar
//! item, no clicking: a quick way to look at layout and type with real
//! fonts and pixels.
//!
//!     cargo run -p menagerie --bin preview -- [--live] [out-dir] [fleet.yaml]
//!
//! Without `--live` the machines show invented data from `fixture()`, so
//! every state can be looked at without running any agents. With `--live`
//! the real poller runs against the fleet's agents first. Defaults:
//! `target/preview` and the bundled example fleet. Capturing uses
//! `screencapture`, which needs the Screen Recording permission for the
//! terminal that runs it.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    AppContext, Application, Bounds, WindowBackgroundAppearance, WindowBounds, WindowKind,
    WindowOptions, point, px, size,
};
use menagerie_protocol::{
    AgentInfo, Battery, Load, Probe, Probes, Revision, RevisionSource, Sample, Status, SystemInfo,
    Temperature, Usage,
};

use menagerie_app::store::{Checkout, Health, Store, Update};
use menagerie_app::ui::popover::{self, Hooks, Popover};
use menagerie_app::ui::window::FleetWindow;
use menagerie_app::{WINDOW_HEIGHT, WINDOW_WIDTH, assets, live, main_window_options};

/// Where the two windows sit while they are photographed, in points from
/// the primary display's top-left corner.
const WINDOW_AT: (f32, f32) = (60., 80.);
const POPOVER_AT: (f32, f32) = (WINDOW_AT.0 + WINDOW_WIDTH + 40., 80.);

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let live_mode = args.iter().any(|a| a == "--live");
    args.retain(|a| a != "--live");
    let mut args = args.into_iter();
    let out = PathBuf::from(args.next().unwrap_or_else(|| "target/preview".into()));
    let fleet = args.next().map(PathBuf::from).unwrap_or_else(example_fleet);
    std::fs::create_dir_all(&out).expect("create the output directory");

    Application::new()
        .with_assets(assets::Assets)
        .run(move |cx| {
            let store = cx.new(|_| Store::load(fleet.clone()));
            if live_mode {
                live::start(store.clone(), cx);
            } else {
                store.update(cx, |store, _| store.apply(fixture()));
            }
            let height = popover::height(store.read(cx));

            let window_bounds = Bounds {
                origin: point(px(WINDOW_AT.0), px(WINDOW_AT.1)),
                size: size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)),
            };
            let window_store = store.clone();
            cx.open_window(main_window_options(window_bounds), |_, cx| {
                cx.new(|cx| FleetWindow::new(window_store, cx))
            })
            .expect("open the window");

            let hooks = Hooks {
                open_window: Rc::new(|_, _| {}),
            };
            let popover_store = store.clone();
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: point(px(POPOVER_AT.0), px(POPOVER_AT.1)),
                        size: size(px(popover::WIDTH), px(height)),
                    })),
                    titlebar: None,
                    focus: false,
                    kind: WindowKind::PopUp,
                    window_background: WindowBackgroundAppearance::Transparent,
                    ..Default::default()
                },
                |_, cx| cx.new(|cx| Popover::new(popover_store, hooks, cx)),
            )
            .expect("open the popover");
            // Bring both to the front, or the window is photographed behind
            // whatever app was active.
            cx.activate(true);

            let wait = if live_mode { 5000 } else { 900 };
            cx.spawn(async move |cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(wait))
                    .await;
                capture(
                    WINDOW_AT,
                    (WINDOW_WIDTH, WINDOW_HEIGHT),
                    &out.join("window.png"),
                );
                capture(
                    POPOVER_AT,
                    (popover::WIDTH, height),
                    &out.join("popover.png"),
                );
                let _ = cx.update(|cx| cx.quit());
            })
            .detach();
        });
}

fn example_fleet() -> PathBuf {
    let path = std::env::temp_dir().join("menagerie-preview-fleet.yaml");
    std::fs::write(&path, menagerie_app::store::EXAMPLE).expect("write the example fleet");
    path
}

const NOW: i64 = 1_795_700_000_000;

/// Invented reports for the example fleet: the workstation in sync, the
/// server four commits behind, Jellyfin up and Jellyseerr down.
fn fixture() -> Update {
    let in_sync = "9b06a19f0aafcad2b703c52bd24328b6992c85f9";
    let behind = "f460dc1f0aafcad2b703c52bd24328b6992c85f9";
    Update {
        live: vec![
            (
                "richard-m4-max".into(),
                Ok(report(
                    in_sync,
                    124,
                    2.1,
                    14,
                    Some((58.0, true)),
                    0.78,
                    0.74,
                    61.0,
                )),
            ),
            (
                "richard-hp-server".into(),
                Ok(report(
                    behind,
                    37,
                    0.4,
                    8,
                    Some((87.0, false)),
                    0.41,
                    0.66,
                    54.0,
                )),
            ),
        ],
        health: vec![(("richard-hp-server".into(), 0), Health::Up { millis: 12 })],
        drift: vec![(in_sync.into(), Ok((0, 0))), (behind.into(), Ok((0, 4)))],
        checkout: Some(Ok(Checkout {
            unpushed: 1,
            unpulled: 0,
            dirty: false,
        })),
    }
}

#[allow(clippy::too_many_arguments)]
fn report(
    rev: &str,
    generation: u64,
    load: f64,
    cpus: u32,
    battery: Option<(f64, bool)>,
    memory: f64,
    disk: f64,
    celsius: f64,
) -> Status {
    let history = (0..24 * 60)
        .map(|i| {
            let t = i as f64 / (24.0 * 60.0);
            let wave = (t * 9.0).sin().abs() * 0.6 + ((t - 0.62) * 14.0).cos().max(0.0) * 0.9;
            Sample {
                at: NOW - (24 * 60 - i) * 60_000,
                value: load * (0.4 + wave),
            }
        })
        .collect();
    Status {
        version: 1,
        host: "fixture".into(),
        agent: AgentInfo {
            version: "0.1.0".into(),
            now: NOW,
        },
        probes: Probes {
            load: with(
                Load {
                    one: load,
                    five: load,
                    fifteen: load,
                    cpus,
                },
                5,
            ),
            memory: with(
                Usage {
                    used_bytes: (memory * 36.0 * (1u64 << 30) as f64) as u64,
                    total_bytes: 36 * (1 << 30),
                },
                5,
            ),
            disk: with(
                Usage {
                    used_bytes: (disk * 994e9) as u64,
                    total_bytes: 994_000_000_000,
                },
                300,
            ),
            battery: Probe {
                value: battery.map(|(percent, charging)| Battery {
                    percent,
                    charging,
                    health_percent: Some(91.0),
                    cycles: Some(336),
                }),
                at: Some(NOW),
                every_s: 30,
                error: None,
            },
            temperature: with(Temperature { celsius }, 30),
            revision: Probe {
                value: Revision::parse(rev, Some(generation), RevisionSource::NixDarwin),
                at: Some(NOW),
                every_s: 10,
                error: None,
            },
            system: with(
                SystemInfo {
                    os: "Darwin".into(),
                    os_version: Some("27.2".into()),
                    uptime_s: 12 * 86_400,
                },
                60,
            ),
        },
        load_history: history,
    }
}

fn with<T>(value: T, every_s: u32) -> Probe<T> {
    Probe {
        value: Some(value),
        at: Some(NOW - 2000),
        every_s,
        error: None,
    }
}

fn capture(origin: (f32, f32), size: (f32, f32), path: &Path) {
    let region = format!("{},{},{},{}", origin.0, origin.1, size.0, size.1);
    let status = std::process::Command::new("screencapture")
        .args(["-x", "-o", "-R", &region])
        .arg(path)
        .status();
    match status {
        Ok(s) if s.success() => println!("{}", path.display()),
        _ => eprintln!("failed to capture {}", path.display()),
    }
}
