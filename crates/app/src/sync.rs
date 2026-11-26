//! Syncing a machine to the latest commit, and rolling it back.
//!
//! `plan` turns a machine's `sync` settings into the commands to run; it is
//! pure, so what Sync will do can be tested without touching a machine.
//! `run` executes the steps one after another, streaming every line of
//! output, and stops at the first failure.
//!
//! On this Mac, `sudo` runs with `-A` and the app itself as the askpass
//! helper (see `askpass`), so a password prompt is a native dialog, and
//! Touch ID works when it is enabled for sudo. Over SSH there is no one to
//! ask, so remote commands that need root use `sudo -n` and fail with a
//! clear message unless the machine allows them without a password.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::Sender;

use menagerie_fleet::{Fleet, Machine, SyncPreset};

use crate::store::Checkout;

/// Set on the app's own executable when sudo runs it as the askpass helper.
pub const ASKPASS_ENV: &str = "MENAGERIE_ASKPASS";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Sync,
    Rollback,
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Action::Sync => "Sync",
            Action::Rollback => "Roll back",
        }
    }
}

/// One command of a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// Shown in the log before the command's output.
    pub label: String,
    pub program: String,
    pub args: Vec<String>,
    /// Run sudo with the app as its askpass helper.
    pub askpass: bool,
}

impl Step {
    fn new(label: impl Into<String>, program: &str, args: &[&str]) -> Self {
        Self {
            label: label.into(),
            program: program.to_owned(),
            args: args.iter().map(|a| a.to_string()).collect(),
            askpass: false,
        }
    }
}

/// Where the machine's commands run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    /// This Mac.
    Here,
    /// Over SSH, to this `user@host`.
    Ssh(String),
}

/// Whether `machine` is the one the app runs on: its agent is on this
/// machine's loopback, or its name is this machine's hostname.
pub fn is_here(name: &str, machine: &Machine, hostname: &str) -> bool {
    let loopback = machine.agent.as_deref().is_some_and(|url| {
        let authority = url.split("://").nth(1).unwrap_or(url);
        let authority = authority.split('/').next().unwrap_or("");
        ["localhost", "127.", "[::1]"]
            .iter()
            .any(|prefix| authority.starts_with(prefix))
    });
    loopback || name.eq_ignore_ascii_case(hostname)
}

/// This machine's short hostname.
pub fn hostname() -> String {
    Command::new("hostname")
        .arg("-s")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default()
}

/// The commands that sync (or roll back) `name`.
pub fn plan(
    action: Action,
    fleet: &Fleet,
    name: &str,
    place: &Place,
    checkout: Option<&Checkout>,
) -> Result<Vec<Step>, String> {
    let machine = fleet
        .machines
        .get(name)
        .ok_or_else(|| format!("{name} is not in the fleet file"))?;
    let sync = machine
        .sync
        .as_ref()
        .ok_or_else(|| format!("{name} has no `sync` settings in the fleet file"))?;
    let mut steps = Vec::new();

    // Sync is two-way: commits made in your checkout go up first, so the
    // machine can pull them.
    if action == Action::Sync
        && let (Some(repo), Some(checkout)) = (fleet.repo_path(), checkout)
    {
        if checkout.dirty {
            return Err(
                "Your checkout has uncommitted changes. Commit or stash them first.".to_owned(),
            );
        }
        if checkout.unpushed > 0 {
            let repo = repo.to_string_lossy();
            steps.push(Step::new(
                format!("Push {} commit(s) from your checkout", checkout.unpushed),
                "git",
                &[
                    "-C",
                    &repo,
                    "push",
                    "origin",
                    &format!("HEAD:{}", fleet.branch()),
                ],
            ));
        }
    }

    let repo = sync.repo.clone().unwrap_or_else(|| ".".to_owned());
    let attr = sync.flake.clone().unwrap_or_else(|| match sync.preset {
        SyncPreset::HomeManager => machine.ssh.clone().unwrap_or_else(|| name.to_owned()),
        _ => name.to_owned(),
    });
    let target = format!(".#{attr}");

    // The machine's own commands, written as one shell script so they run
    // in its checkout, here or over SSH.
    let sudo = match place {
        Place::Here => "sudo -A",
        Place::Ssh(_) => "sudo -n",
    };
    let rebuild = match (action, sync.preset) {
        (Action::Sync, SyncPreset::NixDarwin) => {
            format!("{sudo} darwin-rebuild switch --flake {}", sh_quote(&target))
        }
        (Action::Sync, SyncPreset::Nixos) => {
            format!("{sudo} nixos-rebuild switch --flake {}", sh_quote(&target))
        }
        (Action::Sync, SyncPreset::HomeManager) => {
            format!("home-manager switch --flake {}", sh_quote(&target))
        }
        (Action::Sync, SyncPreset::Custom) => sync.command.clone().unwrap_or_default(),
        (Action::Rollback, SyncPreset::NixDarwin) => format!("{sudo} darwin-rebuild --rollback"),
        (Action::Rollback, SyncPreset::Nixos) => {
            format!("{sudo} nixos-rebuild switch --rollback")
        }
        (Action::Rollback, SyncPreset::HomeManager) => {
            // The generation before the current one, activated directly.
            "\"$(home-manager generations | sed -n 2p | awk '{print $NF}')/activate\"".to_owned()
        }
        (Action::Rollback, SyncPreset::Custom) => {
            return Err(format!(
                "{name} syncs with a custom command, so Menagerie doesn't know how to roll it back"
            ));
        }
    };
    let script = match action {
        Action::Sync => format!("cd {} && git pull --ff-only && {rebuild}", sh_path(&repo)),
        Action::Rollback => format!("cd {} && {rebuild}", sh_path(&repo)),
    };
    let label = match (action, place) {
        (Action::Sync, Place::Here) => "Pull and switch this Mac".to_owned(),
        (Action::Sync, Place::Ssh(to)) => format!("Pull and switch {to}"),
        (Action::Rollback, Place::Here) => "Roll this Mac back".to_owned(),
        (Action::Rollback, Place::Ssh(to)) => format!("Roll back {to}"),
    };
    let mut step = match place {
        // A login shell, so the machine's Nix tools are on PATH.
        Place::Here => Step::new(label, "/bin/sh", &["-lc", &script]),
        Place::Ssh(to) => Step::new(
            label,
            "ssh",
            &[
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=10",
                "--",
                to,
                &format!("sh -lc {}", sh_quote(&script)),
            ],
        ),
    };
    step.askpass = *place == Place::Here && rebuild.contains("sudo -A");
    steps.push(step);
    Ok(steps)
}

/// Where to run `name`'s commands, or why they can't run anywhere.
pub fn place(name: &str, machine: &Machine, hostname: &str) -> Result<Place, String> {
    if is_here(name, machine, hostname) {
        Ok(Place::Here)
    } else if let Some(ssh) = &machine.ssh {
        Ok(Place::Ssh(ssh.clone()))
    } else {
        Err(format!(
            "{name} isn't this Mac and has no `ssh` target in the fleet file"
        ))
    }
}

/// Quote for a POSIX shell.
fn sh_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// Quote a path, leaving a leading `~/` for the shell to expand on the
/// machine it runs on.
fn sh_path(path: &str) -> String {
    match path.strip_prefix("~/") {
        Some(rest) => format!("~/{}", sh_quote(rest)),
        None => sh_quote(path),
    }
}

/// What a running step reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Line(String),
    /// The whole plan finished; `Err` says which step failed and why.
    Done(Result<(), String>),
}

/// Stops a running plan: the current step's whole process group is killed
/// and no further step starts.
#[derive(Debug, Clone, Default)]
pub struct Cancel {
    cancelled: Arc<AtomicBool>,
    /// The running step's process group, or 0 between steps.
    group: Arc<AtomicU32>,
}

impl Cancel {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        let group = self.group.load(Ordering::SeqCst);
        if group != 0 {
            let _ = Command::new("kill")
                .args(["-TERM", "--", &format!("-{group}")])
                .status();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// Run `steps` in order, sending every output line and then the result.
/// Blocks; call it off the main thread.
pub fn run(steps: Vec<Step>, events: Sender<Event>, cancel: Cancel) {
    let result = steps.iter().try_for_each(|step| {
        if cancel.is_cancelled() {
            return Err("Cancelled".to_owned());
        }
        run_step(step, &events, &cancel)
    });
    let result = match result {
        Err(_) if cancel.is_cancelled() => Err("Cancelled".to_owned()),
        other => other,
    };
    let _ = events.send(Event::Done(result));
}

fn run_step(step: &Step, events: &Sender<Event>, cancel: &Cancel) -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    let _ = events.send(Event::Line(format!("▸ {}", step.label)));
    let mut command = Command::new(&step.program);
    command
        .args(&step.args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("GIT_TERMINAL_PROMPT", "0");
    if step.askpass
        && let Ok(exe) = std::env::current_exe()
    {
        command.env("SUDO_ASKPASS", exe).env(ASKPASS_ENV, "1");
    }
    let mut child = command
        // Its own process group, so Cancel stops everything it started.
        .process_group(0)
        .spawn()
        .map_err(|e| format!("{}: could not start {}: {e}", step.label, step.program))?;
    cancel.group.store(child.id(), Ordering::SeqCst);
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    std::thread::scope(|scope| {
        if let Some(out) = stdout {
            scope.spawn(|| forward(out, events));
        }
        if let Some(err) = stderr {
            scope.spawn(|| forward(err, events));
        }
    });
    let status = child.wait().map_err(|e| format!("{}: {e}", step.label));
    cancel.group.store(0, Ordering::SeqCst);
    let status = status?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{} failed ({status})", step.label))
    }
}

fn forward(stream: impl std::io::Read, events: &Sender<Event>) {
    for line in BufReader::new(stream).lines().map_while(Result::ok) {
        let _ = events.send(Event::Line(strip_ansi(&line)));
    }
}

/// Drop terminal colour codes, which would show as noise in the log.
fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        if c != '\r' {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod cancel_tests {
    use super::*;

    #[test]
    fn cancel_stops_a_hanging_step_and_its_children() {
        let (tx, rx) = std::sync::mpsc::channel();
        let cancel = Cancel::default();
        let handle = {
            let cancel = cancel.clone();
            std::thread::spawn(move || {
                run(
                    vec![
                        // A shell whose child sleeps, as ssh waiting on a check would.
                        Step::new(
                            "hang",
                            "/bin/sh",
                            &["-c", "echo waiting; sleep 30; echo late"],
                        ),
                        Step::new("next", "/bin/sh", &["-c", "echo next"]),
                    ],
                    tx,
                    cancel,
                )
            })
        };
        // Wait until the step is running, then cancel.
        assert_eq!(rx.recv().unwrap(), Event::Line("▸ hang".into()));
        assert_eq!(rx.recv().unwrap(), Event::Line("waiting".into()));
        let started = std::time::Instant::now();
        cancel.cancel();
        let rest: Vec<Event> = rx.iter().collect();
        handle.join().unwrap();
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        assert_eq!(rest.last(), Some(&Event::Done(Err("Cancelled".into()))));
        assert!(!rest.contains(&Event::Line("late".into())));
        assert!(!rest.contains(&Event::Line("▸ next".into())));
    }
}

/// Act as sudo's askpass helper: ask for the password in a native dialog
/// and print it. Returns the process exit code.
pub fn askpass() -> i32 {
    let script = r#"text returned of (display dialog "Menagerie needs your password to switch this Mac's configuration." default answer "" with hidden answer with title "Menagerie" with icon caution)"#;
    match Command::new("osascript").args(["-e", script]).output() {
        Ok(out) if out.status.success() => {
            print!(
                "{}",
                String::from_utf8_lossy(&out.stdout).trim_end_matches('\n')
            );
            0
        }
        _ => 1,
    }
}

/// Post a macOS notification.
pub fn notify(title: &str, body: &str) {
    let script = format!(
        "display notification {} with title {}",
        applescript_string(body),
        applescript_string(title)
    );
    let _ = Command::new("osascript").args(["-e", &script]).status();
}

fn applescript_string(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The commits `rev` is missing from `upstream`, newest first, as
/// `(short hash, subject)`, for the Changes view.
pub fn missing_commits(repo: &Path, upstream: &str, rev: &str) -> Vec<(String, String)> {
    let range = format!("{rev}..{upstream}");
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .args([
            "log",
            "--format=%h%x09%s",
            "-n",
            "50",
            "--end-of-options",
            &range,
        ])
        .stdin(Stdio::null())
        .output();
    match out {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| l.split_once('\t'))
            .map(|(h, s)| (h.to_owned(), s.to_owned()))
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fleet() -> Fleet {
        Fleet::from_yaml(include_str!("../../../examples/fleet.yaml")).unwrap()
    }

    fn clean() -> Checkout {
        Checkout::default()
    }

    #[test]
    fn this_mac_is_found_by_its_loopback_agent_or_name() {
        let f = fleet();
        assert!(is_here(
            "richard-m4-max",
            &f.machines["richard-m4-max"],
            "other"
        ));
        assert!(!is_here(
            "richard-hp-server",
            &f.machines["richard-hp-server"],
            "other"
        ));
        assert!(is_here(
            "richard-hp-server",
            &f.machines["richard-hp-server"],
            "richard-hp-server"
        ));
    }

    #[test]
    fn syncs_this_mac_with_darwin_rebuild_and_askpass() {
        let steps = plan(
            Action::Sync,
            &fleet(),
            "richard-m4-max",
            &Place::Here,
            Some(&clean()),
        )
        .unwrap();
        assert_eq!(steps.len(), 1);
        let step = &steps[0];
        assert_eq!(step.program, "/bin/sh");
        assert_eq!(
            step.args[1],
            "cd ~/'dev/gh/richard-better/mac' && git pull --ff-only && sudo -A darwin-rebuild switch --flake '.#richard-m4-max'"
        );
        assert!(step.askpass);
    }

    #[test]
    fn syncs_a_home_manager_machine_over_ssh() {
        let place = Place::Ssh("richard@richard-hp-server".into());
        let steps = plan(
            Action::Sync,
            &fleet(),
            "richard-hp-server",
            &place,
            Some(&clean()),
        )
        .unwrap();
        let step = &steps[0];
        assert_eq!(step.program, "ssh");
        assert_eq!(step.args[5], "richard@richard-hp-server");
        assert_eq!(
            step.args[6],
            r#"sh -lc 'cd ~/'\''dev/gh/richard-better/mac'\'' && git pull --ff-only && home-manager switch --flake '\''.#richard@richard-hp-server'\'''"#
        );
        assert!(!step.askpass);
    }

    #[test]
    fn pushes_unpushed_commits_first_and_refuses_a_dirty_checkout() {
        let checkout = Checkout {
            unpushed: 2,
            ..Checkout::default()
        };
        let steps = plan(
            Action::Sync,
            &fleet(),
            "richard-m4-max",
            &Place::Here,
            Some(&checkout),
        )
        .unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].program, "git");
        assert_eq!(steps[0].args.last().unwrap(), "HEAD:main");

        let dirty = Checkout {
            dirty: true,
            ..Checkout::default()
        };
        assert!(
            plan(
                Action::Sync,
                &fleet(),
                "richard-m4-max",
                &Place::Here,
                Some(&dirty)
            )
            .is_err()
        );
        // Rolling back doesn't touch the checkout.
        assert!(
            plan(
                Action::Rollback,
                &fleet(),
                "richard-m4-max",
                &Place::Here,
                Some(&dirty)
            )
            .is_ok()
        );
    }

    #[test]
    fn machines_without_sync_or_a_way_in_are_refused() {
        let f = fleet();
        assert!(plan(Action::Sync, &f, "homeassistant", &Place::Here, None).is_err());
        assert!(place("homeassistant", &f.machines["homeassistant"], "other").is_err());
    }

    #[test]
    fn quoting_survives_quotes() {
        assert_eq!(sh_quote("it's"), r"'it'\''s'");
        assert_eq!(sh_path("/srv/a b"), "'/srv/a b'");
    }

    #[test]
    fn runs_steps_in_order_and_stops_at_a_failure() {
        let (tx, rx) = std::sync::mpsc::channel();
        run(
            vec![
                Step::new("first", "/bin/sh", &["-c", "echo one; echo two >&2"]),
                Step::new(
                    "second",
                    "/bin/sh",
                    &["-c", "printf '\\033[1mbold\\033[0m\\n'; exit 3"],
                ),
                Step::new("never", "/bin/sh", &["-c", "echo nope"]),
            ],
            tx,
            Cancel::default(),
        );
        let events: Vec<Event> = rx.iter().collect();
        let lines: Vec<&str> = events
            .iter()
            .filter_map(|e| match e {
                Event::Line(l) => Some(l.as_str()),
                Event::Done(_) => None,
            })
            .collect();
        assert!(lines.contains(&"one") && lines.contains(&"two") && lines.contains(&"bold"));
        assert!(!lines.contains(&"nope"));
        match events.last().unwrap() {
            Event::Done(Err(why)) => assert!(why.starts_with("second failed"), "{why}"),
            other => panic!("expected a failure, got {other:?}"),
        }
    }
}
