//! Keeps the store fresh: polls every agent and service health URL, and
//! compares each machine's revision with your checkout using git.
//!
//! The network and git work runs on gpui's background executor, all
//! machines at once, so one offline machine costs a single timeout rather
//! than one per machine. Results are applied to the store in one update.
//! `git fetch` runs in its own loop with a deadline, so a slow remote never
//! holds up the machines' health.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use futures::channel::mpsc::unbounded;
use futures::{FutureExt, StreamExt};
use gpui::{App, AppContext, Entity};
use menagerie_protocol as protocol;

use crate::store::{Checkout, Drift, Health, Store, Update};

pub const POLL_EVERY: Duration = Duration::from_secs(10);
const FETCH_EVERY: Duration = Duration::from_secs(300);
const HTTP_TIMEOUT: Duration = Duration::from_secs(3);
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);

/// What one poll needs to know, read from the store on the main thread.
struct Targets {
    /// The fleet file revision these targets came from.
    generation: u64,
    agents: Vec<(String, String)>,
    services: Vec<((String, usize), String)>,
    repo: Option<PathBuf>,
    branch: String,
    /// Revisions already reported, whose drift is worth working out again.
    known_revs: Vec<String>,
}

pub fn start(store: Entity<Store>, cx: &mut App) {
    let (tx, mut pokes) = unbounded();
    store.update(cx, |store, _| store.poke = Some(tx));

    let poll_store = store.clone();
    cx.spawn(async move |cx| {
        loop {
            let Ok(targets) = cx.read_entity(&poll_store, |store, _| targets(store)) else {
                return;
            };
            let update = cx
                .background_executor()
                .spawn(async move { poll(targets) })
                .await;
            if poll_store
                .update(cx, |store, cx| {
                    store.apply(update);
                    cx.notify();
                })
                .is_err()
            {
                return;
            }
            let timer = cx.background_executor().timer(POLL_EVERY).fuse();
            futures::pin_mut!(timer);
            futures::select! {
                _ = timer => {}
                _ = pokes.next() => {}
            }
        }
    })
    .detach();

    // Fetch on its own schedule, then ask for a poll so new commits show.
    cx.spawn(async move |cx| {
        loop {
            let Ok(target) = cx.read_entity(&store, |store, _| {
                let fleet = store.fleet.as_ref()?;
                Some((fleet.repo_path()?, fleet.branch().to_owned()))
            }) else {
                return;
            };
            if let Some((repo, branch)) = target {
                cx.background_executor()
                    .spawn(async move { fetch(&repo, &branch) })
                    .await;
                if store.update(cx, |store, _| store.refresh()).is_err() {
                    return;
                }
            }
            cx.background_executor().timer(FETCH_EVERY).await;
        }
    })
    .detach();
}

fn targets(store: &Store) -> Targets {
    let machines = store.machines();
    Targets {
        generation: store.generation,
        agents: machines
            .iter()
            .filter_map(|(name, m)| m.agent.clone().map(|a| (name.to_string(), a)))
            .collect(),
        services: machines
            .iter()
            .flat_map(|(name, m)| {
                m.services.iter().enumerate().filter_map(move |(i, s)| {
                    s.health.clone().map(|url| ((name.to_string(), i), url))
                })
            })
            .collect(),
        repo: store.fleet.as_ref().and_then(|f| f.repo_path()),
        branch: store
            .fleet
            .as_ref()
            .map(|f| f.branch().to_owned())
            .unwrap_or_else(|| "main".to_owned()),
        known_revs: store
            .live
            .values()
            .filter_map(|l| l.status.as_ref()?.probes.revision.value.as_ref())
            .map(|r| r.rev.clone())
            .collect(),
    }
}

fn poll(targets: Targets) -> Update {
    let http: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(HTTP_TIMEOUT))
        .build()
        .into();

    let (live, health) = std::thread::scope(|scope| {
        let agents: Vec<_> = targets
            .agents
            .iter()
            .map(|(name, url)| {
                let http = http.clone();
                scope.spawn(move || (name.clone(), fetch_status(&http, url)))
            })
            .collect();
        let services: Vec<_> = targets
            .services
            .iter()
            .map(|(key, url)| {
                let http = http.clone();
                scope.spawn(move || (key.clone(), check_health(&http, url)))
            })
            .collect();
        (
            agents
                .into_iter()
                .filter_map(|h| h.join().ok())
                .collect::<Vec<_>>(),
            services
                .into_iter()
                .filter_map(|h| h.join().ok())
                .collect::<Vec<_>>(),
        )
    });

    let mut update = Update {
        generation: Some(targets.generation),
        live,
        health,
        ..Update::default()
    };

    if let Some(repo) = &targets.repo {
        let upstream = format!("origin/{}", targets.branch);
        let mut revs: Vec<String> = update
            .live
            .iter()
            .filter_map(|(_, r)| r.as_ref().ok()?.probes.revision.value.as_ref())
            .map(|r| r.rev.clone())
            .chain(targets.known_revs)
            .collect();
        revs.sort();
        revs.dedup();
        update.drift = revs
            .into_iter()
            .map(|rev| {
                let drift = ahead_behind(repo, &upstream, &rev);
                (rev, drift)
            })
            .collect();
        update.changes = update
            .drift
            .iter()
            .filter(|(_, d)| matches!(d, Ok((_, behind)) if *behind > 0))
            .map(|(rev, _)| {
                (
                    rev.clone(),
                    crate::sync::missing_commits(repo, &upstream, rev),
                )
            })
            .collect();
        update.checkout = Some(checkout(repo, &upstream));
    }
    update
}

fn fetch_status(http: &ureq::Agent, base: &str) -> Result<protocol::Status, String> {
    let url = format!("{}/status", base.trim_end_matches('/'));
    let status: protocol::Status = http
        .get(&url)
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_json()
        .map_err(|e| format!("unreadable reply: {e}"))?;
    if let Some(rev) = &status.probes.revision.value
        && !rev.is_valid()
    {
        return Err("agent reported a revision that is not a commit hash".to_owned());
    }
    if status.version != protocol::VERSION {
        return Err(format!(
            "agent speaks protocol {}, this app speaks {}",
            status.version,
            protocol::VERSION
        ));
    }
    Ok(status)
}

fn check_health(http: &ureq::Agent, url: &str) -> Health {
    let started = Instant::now();
    match http.get(url).call() {
        Ok(response) if !response.status().is_success() => {
            Health::Down(format!("HTTP {}", response.status()))
        }
        Ok(_) => Health::Up {
            millis: started.elapsed().as_millis().min(u32::MAX as u128) as u32,
        },
        Err(err) => Health::Down(err.to_string()),
    }
}

/// A git command that can never stop to ask for anything.
fn git_command(repo: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(repo)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null());
    command
}

/// Local git: never reaches the network, not even to fill in a partial
/// clone's missing objects.
fn git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let output = git_command(repo)
        .env("GIT_NO_LAZY_FETCH", "1")
        .args(args)
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

/// `git fetch` with a deadline. Best effort: offline or without
/// credentials, the last fetch still gives a useful answer.
fn fetch(repo: &Path, branch: &str) {
    use std::os::unix::process::CommandExt;
    let mut command = git_command(repo);
    // Batch-mode SSH unless the user has their own SSH command configured.
    let configured = std::env::var_os("GIT_SSH_COMMAND").is_some()
        || std::env::var_os("GIT_SSH").is_some()
        || git(repo, &["config", "--get", "core.sshCommand"]).is_ok();
    if !configured {
        command.env(
            "GIT_SSH_COMMAND",
            "ssh -o BatchMode=yes -o ConnectTimeout=10",
        );
    }
    let Ok(mut child) = command
        .args(["fetch", "--quiet", "origin", "--end-of-options", branch])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        // Its own process group, so a timeout also stops the remote helpers
        // git starts.
        .process_group(0)
        .spawn()
    else {
        return;
    };
    let started = Instant::now();
    while started.elapsed() < FETCH_TIMEOUT {
        if !matches!(child.try_wait(), Ok(None)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = Command::new("kill")
        .args(["-KILL", "--", &format!("-{}", child.id())])
        .status();
    let _ = child.wait();
}

/// How far `rev` is ahead of and behind `upstream`.
fn ahead_behind(repo: &Path, upstream: &str, rev: &str) -> Drift {
    let range = format!("{upstream}...{rev}");
    let out = git(
        repo,
        &[
            "rev-list",
            "--left-right",
            "--count",
            "--end-of-options",
            &range,
        ],
    )
    .map_err(|_| format!("{} is not in your checkout", short(rev)))?;
    parse_counts(&out).map(|(behind, ahead)| (ahead, behind))
}

fn checkout(repo: &Path, upstream: &str) -> Result<Checkout, String> {
    let range = format!("{upstream}...HEAD");
    let (unpulled, unpushed) = parse_counts(&git(
        repo,
        &["rev-list", "--left-right", "--count", &range],
    )?)?;
    let dirty = !git(repo, &["status", "--porcelain", "--untracked-files=normal"])?.is_empty();
    Ok(Checkout {
        unpushed,
        unpulled,
        dirty,
    })
}

/// `rev-list --left-right --count` prints "<left>\t<right>".
fn parse_counts(text: &str) -> Result<(u32, u32), String> {
    let mut parts = text.split_whitespace().map(str::parse::<u32>);
    match (parts.next(), parts.next()) {
        (Some(Ok(left)), Some(Ok(right))) => Ok((left, right)),
        _ => Err(format!("unexpected git output: {text:?}")),
    }
}

fn short(rev: &str) -> &str {
    rev.get(..7).unwrap_or(rev)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }

    /// Serve one HTTP reply on a local port and return its base URL.
    fn serve_once(status: &str, body: String) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let status = status.to_owned();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 2048];
            let _ = stream.read(&mut buf);
            let reply = format!(
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(reply.as_bytes()).unwrap();
        });
        format!("http://{addr}")
    }

    fn http() -> ureq::Agent {
        ureq::Agent::config_builder()
            .timeout_global(Some(HTTP_TIMEOUT))
            .build()
            .into()
    }

    #[test]
    fn reads_an_agents_status() {
        let status = protocol::Status {
            version: protocol::VERSION,
            host: "box".into(),
            agent: protocol::AgentInfo {
                version: "0.1.0".into(),
                now: 1,
            },
            probes: protocol::Probes::default(),
            load_history: vec![],
        };
        let base = serve_once("200 OK", serde_json::to_string(&status).unwrap());
        assert_eq!(fetch_status(&http(), &format!("{base}/")), Ok(status));
    }

    #[test]
    fn rejects_other_protocol_versions() {
        let body = serde_json::json!({
            "version": 99, "host": "box",
            "agent": { "version": "9", "now": 1 },
            "probes": protocol::Probes::default(),
        });
        let base = serve_once("200 OK", body.to_string());
        let err = fetch_status(&http(), &base).unwrap_err();
        assert!(err.contains("protocol 99"), "{err}");
    }

    #[test]
    fn rejects_a_revision_that_is_not_a_hash() {
        let mut probes = protocol::Probes::default();
        probes.revision.value = Some(protocol::Revision {
            rev: "--upload-pack=touch /tmp/x".into(),
            dirty: false,
            generation: None,
            source: protocol::RevisionSource::Nixos,
        });
        let body = serde_json::json!({
            "version": protocol::VERSION, "host": "box",
            "agent": { "version": "9", "now": 1 },
            "probes": probes,
        });
        let base = serve_once("200 OK", body.to_string());
        assert!(fetch_status(&http(), &base).is_err());
    }

    #[test]
    fn health_is_down_on_an_error_status() {
        let base = serve_once("503 Service Unavailable", String::new());
        assert!(matches!(check_health(&http(), &base), Health::Down(_)));
        let base = serve_once("200 OK", String::new());
        assert!(matches!(check_health(&http(), &base), Health::Up { .. }));
    }

    #[test]
    fn counts_commits_against_upstream() {
        let dir = std::env::temp_dir().join(format!("menagerie-git-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        sh(&dir, &["init", "-q", "-b", "main"]);
        sh(&dir, &["config", "commit.gpgsign", "false"]);
        sh(&dir, &["commit", "-q", "--allow-empty", "-m", "one"]);
        let old = git(&dir, &["rev-parse", "HEAD"]).unwrap();
        sh(&dir, &["commit", "-q", "--allow-empty", "-m", "two"]);
        sh(&dir, &["commit", "-q", "--allow-empty", "-m", "three"]);
        // Stand in for a fetched remote branch.
        sh(&dir, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
        sh(&dir, &["commit", "-q", "--allow-empty", "-m", "local"]);

        assert_eq!(ahead_behind(&dir, "origin/main", &old), Ok((0, 2)));
        let head = git(&dir, &["rev-parse", "HEAD"]).unwrap();
        assert_eq!(ahead_behind(&dir, "origin/main", &head), Ok((1, 0)));
        assert!(ahead_behind(&dir, "origin/main", "deadbeefdeadbeef").is_err());
        let co = checkout(&dir, "origin/main").unwrap();
        assert_eq!((co.unpushed, co.unpulled, co.dirty), (1, 0, false));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
