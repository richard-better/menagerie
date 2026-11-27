//! Syncing a machine to the latest commit, and rolling it back.
//!
//! `plan` turns a machine's `sync` settings into the commands to run; it is
//! pure, so what Sync will do can be tested without touching a machine.
//! `run` executes the steps one after another, streaming every line of
//! output, and stops at the first failure.
//!
//! On this Mac, the rebuild runs as root by itself, after the pull has run
//! as you: with `sudo -n` when sudoers lets you run it without a password,
//! else behind the system's administrator prompt (see `admin`). Over SSH
//! there is no one to ask, so remote commands that need root use `sudo -n`
//! and fail with a clear message unless the machine allows them without a
//! password.
//!
//! Sync deploys commits, never a working tree: every machine builds the
//! commit its checkout is on, so uncommitted edits (often a tool rewriting
//! its own settings) neither block a sync nor sneak into one.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use menagerie_fleet::{Fleet, Machine, SyncPreset};

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
    /// Run as root on this Mac. Holds the reason the administrator prompt
    /// gives, if one is needed.
    pub admin: Option<String>,
}

impl Step {
    fn new(label: impl Into<String>, program: &str, args: &[&str]) -> Self {
        Self {
            label: label.into(),
            program: program.to_owned(),
            args: args.iter().map(|a| a.to_string()).collect(),
            admin: None,
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
    let loopback = machine
        .agent
        .as_deref()
        .and_then(url_host)
        .is_some_and(|host| {
            host.eq_ignore_ascii_case("localhost")
                || host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });
    loopback || name.eq_ignore_ascii_case(hostname)
}

/// The host of a URL such as `http://user@[::1]:9184/x`, without brackets.
fn url_host(url: &str) -> Option<&str> {
    let authority = url.split_once("://")?.1.split(['/', '?', '#']).next()?;
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    match host_port.strip_prefix('[') {
        Some(rest) => rest.split_once(']').map(|(host, _)| host),
        None => host_port.split(':').next(),
    }
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
pub fn plan(action: Action, fleet: &Fleet, name: &str, place: &Place) -> Result<Vec<Step>, String> {
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
    // machine can pull them. Checked when the sync runs, not from the last
    // poll, so nothing committed since is missed. Uncommitted changes are
    // only mentioned: they aren't part of any sync.
    if action == Action::Sync
        && let Some(repo) = fleet.repo_path()
    {
        let head = sh_quote(&format!("refs/heads/{}", fleet.branch()));
        let refspec = sh_quote(&format!("HEAD:refs/heads/{}", fleet.branch()));
        let script = format!(
            "cd {} || exit 1\n\
             if [ \"$(git symbolic-ref --quiet HEAD)\" != {head} ]; then\n\
             \x20 echo \"Your checkout isn't on the configured branch. Switch to it first.\" >&2; exit 1\n\
             fi\n\
             n=$(git status --porcelain --untracked-files=all --ignore-submodules=none | wc -l | tr -d ' ')\n\
             if [ \"$n\" = 1 ]; then\n\
             \x20 echo '1 uncommitted file stays behind: Sync deploys commits only.'\n\
             elif [ \"$n\" -gt 1 ] 2>/dev/null; then\n\
             \x20 echo \"$n uncommitted files stay behind: Sync deploys commits only.\"\n\
             fi\n\
             git fetch -q origin {head} || exit 1\n\
             ahead=$(git rev-list --count FETCH_HEAD..HEAD) || exit 1\n\
             if [ \"$ahead\" -eq 0 ]; then echo 'Nothing to push.'; exit 0; fi\n\
             if ! git merge-base --is-ancestor FETCH_HEAD HEAD; then\n\
             \x20 echo \"Rebasing your $ahead commit(s) onto origin.\"\n\
             \x20 if ! git rebase -q --autostash FETCH_HEAD; then\n\
             \x20   git rebase --abort 2>/dev/null\n\
             \x20   echo \"Your commits don't rebase cleanly onto origin. Rebase them yourself, then sync.\" >&2; exit 1\n\
             \x20 fi\n\
             fi\n\
             git push origin {refspec}",
            sh_quote(&repo.to_string_lossy()),
        );
        steps.push(Step::new(
            "Check your checkout and push its commits",
            "/bin/sh",
            &["-c", &script],
        ));
    }

    let repo = sync.repo.clone().unwrap_or_else(|| ".".to_owned());
    let attr = sync.flake.clone().unwrap_or_else(|| match sync.preset {
        SyncPreset::HomeManager => machine.ssh.clone().unwrap_or_else(|| name.to_owned()),
        _ => name.to_owned(),
    });

    // This Mac's system rebuild needs root: the pull runs as you, then the
    // rebuild runs by itself as root, so that passwordless sudo can allow
    // exactly that command, and the administrator prompt covers only it.
    if *place == Place::Here
        && let Some(rebuild) = match sync.preset {
            SyncPreset::NixDarwin => Some("/run/current-system/sw/bin/darwin-rebuild"),
            SyncPreset::Nixos => Some("/run/current-system/sw/bin/nixos-rebuild"),
            _ => None,
        }
    {
        let dir = menagerie_fleet::expand_home(&repo);
        let mut step = match action {
            Action::Sync => {
                let pull = sh_quote(&format!("refs/heads/{}", fleet.branch()));
                let script = format!(
                    "cd {} || exit 1\n\
                     if [ \"$(git symbolic-ref --quiet HEAD)\" != {pull} ]; then\n\
                     \x20 echo \"This Mac's checkout isn't on the configured branch.\" >&2; exit 1\n\
                     fi\n\
                     {}",
                    sh_quote(&dir.to_string_lossy()),
                    fast_forward(&pull),
                );
                steps.push(Step::new(
                    "Pull this Mac's checkout",
                    "/bin/sh",
                    &["-c", &script],
                ));
                // The branch's commit, which the pull just brought up to
                // date, never the working tree.
                let flake = format!(
                    "git+file://{}?ref=refs/heads/{}#{attr}",
                    dir.to_string_lossy()
                        .replace('%', "%25")
                        .replace(' ', "%20"),
                    fleet.branch()
                );
                Step::new("Switch this Mac", rebuild, &["switch", "--flake", &flake])
            }
            Action::Rollback => match sync.preset {
                SyncPreset::NixDarwin => Step::new("Roll this Mac back", rebuild, &["--rollback"]),
                _ => Step::new("Roll this Mac back", rebuild, &["switch", "--rollback"]),
            },
        };
        step.admin = Some(
            match action {
                Action::Sync => "Menagerie needs to switch this Mac to its new configuration.",
                Action::Rollback => {
                    "Menagerie needs to roll this Mac back to its previous configuration."
                }
            }
            .to_owned(),
        );
        steps.push(step);
        return Ok(steps);
    }
    // The commit the checkout is on, not its working tree: set by
    // `FLAKE_AT_HEAD` in the script.
    let target = format!("\"$flake\"{}", sh_quote(&format!("#{attr}")));

    // The machine's own commands, written as one shell script so they run
    // in its checkout, here or over SSH.
    let sudo = "sudo -n";
    let rebuild = match (action, sync.preset) {
        (Action::Sync, SyncPreset::NixDarwin) => {
            format!("{sudo} darwin-rebuild switch --flake {target}")
        }
        (Action::Sync, SyncPreset::Nixos) => {
            format!("{sudo} nixos-rebuild switch --flake {target}")
        }
        (Action::Sync, SyncPreset::HomeManager) => {
            format!("home-manager switch --flake {target}")
        }
        // In a subshell, so none of it runs if the pull fails.
        (Action::Sync, SyncPreset::Custom) => {
            format!("(\n{}\n)", sync.command.clone().unwrap_or_default())
        }
        (Action::Rollback, SyncPreset::NixDarwin) => format!("{sudo} darwin-rebuild --rollback"),
        (Action::Rollback, SyncPreset::Nixos) => {
            format!("{sudo} nixos-rebuild switch --rollback")
        }
        (Action::Rollback, SyncPreset::HomeManager) => "home-manager switch --rollback".to_owned(),
        (Action::Rollback, SyncPreset::Custom) => {
            return Err(format!(
                "{name} syncs with a custom command, so Menagerie doesn't know how to roll it back"
            ));
        }
    };
    let script = match action {
        // The machine has to be on the configured branch too, or a pull
        // would quietly update some other branch.
        Action::Sync => {
            let pull = sh_quote(&format!("refs/heads/{}", fleet.branch()));
            let head = pull.clone();
            format!(
                "cd {} || exit 1\n\
                 if [ \"$(git symbolic-ref --quiet HEAD)\" != {head} ]; then\n\
                 \x20 echo \"The checkout on this machine isn't on the configured branch.\" >&2; exit 1\n\
                 fi\n\
                 {}\n\
                 {FLAKE_AT_HEAD}\n\
                 {rebuild}",
                sh_path(&repo),
                fast_forward(&pull),
            )
        }
        Action::Rollback => format!("cd {} && {rebuild}", sh_path(&repo)),
    };
    let label = match (action, place) {
        (Action::Sync, Place::Here) => "Pull and switch this Mac".to_owned(),
        (Action::Sync, Place::Ssh(to)) => format!("Pull and switch {to}"),
        (Action::Rollback, Place::Here) => "Roll this Mac back".to_owned(),
        (Action::Rollback, Place::Ssh(to)) => format!("Roll back {to}"),
    };
    let step = match place {
        // A login shell, so the machine's Nix tools are on PATH.
        Place::Here => Step::new(label, "/bin/sh", &["-lc", &script]),
        Place::Ssh(to) => Step::new(
            label,
            "ssh",
            &[
                // A terminal on the far side, so cancelling (which hangs up
                // the connection) also stops the remote commands.
                "-tt",
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
    steps.push(step);
    Ok(steps)
}

/// Fast-forward the checkout to `refspec` on origin. Not `git pull`: that
/// follows the machine's `pull.rebase`, and a rebase refuses to run with
/// uncommitted changes even when there is nothing to pull. A fast-forward
/// merge only minds changes to the files it updates.
fn fast_forward(refspec: &str) -> String {
    format!("git fetch origin {refspec} || exit 1\ngit merge --ff-only FETCH_HEAD || exit 1")
}

/// Sets `$flake` to the checkout (the current directory) at its HEAD
/// commit, so a build ignores uncommitted changes. A `git+file` URL, so
/// spaces in the path are escaped.
const FLAKE_AT_HEAD: &str =
    "flake=\"git+file://$(pwd -P | sed 's/ /%20/g')?rev=$(git rev-parse HEAD)\" || exit 1";

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

/// How long a cancelled step gets to exit before it is killed.
const CANCEL_GRACE: std::time::Duration = std::time::Duration::from_secs(3);

fn signal_group(signal: &str, group: u32) {
    let _ = Command::new("kill")
        .args([signal, "--", &format!("-{group}")])
        .status();
}

/// Stops a running plan: the current step's whole process group is killed
/// and no further step starts. Starting a step and cancelling take the same
/// lock, so a cancel can't slip in between the check and the spawn.
#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<Mutex<CancelState>>);

#[derive(Debug, Default)]
struct CancelState {
    cancelled: bool,
    /// The running step's process group, or 0 between steps.
    group: u32,
    /// Stops the running step when it runs as root behind the prompt.
    stopper: Option<crate::admin::Stopper>,
}

impl Cancel {
    /// Ask the running step to stop, and kill it if it hasn't after a
    /// grace period. The job stays running until the step has exited.
    pub fn cancel(&self) {
        let group = {
            let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
            state.cancelled = true;
            if let Some(stopper) = &state.stopper {
                stopper.stop();
            }
            state.group
        };
        if group == 0 {
            return;
        }
        signal_group("-TERM", group);
        let state = self.0.clone();
        std::thread::spawn(move || {
            std::thread::sleep(CANCEL_GRACE);
            // Still the same step: it ignored SIGTERM.
            if state.lock().unwrap_or_else(|e| e.into_inner()).group == group {
                signal_group("-KILL", group);
            }
        });
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).cancelled
    }
}

/// Run `steps` in order, sending every output line and then the result.
/// Blocks; call it off the main thread.
pub fn run(steps: Vec<Step>, events: Sender<Event>, cancel: Cancel) {
    let result = steps
        .iter()
        .try_for_each(|step| run_step(step, &events, &cancel));
    let result = match result {
        Err(_) if cancel.is_cancelled() => Err("Cancelled".to_owned()),
        other => other,
    };
    let _ = events.send(Event::Done(result));
}

fn run_step(step: &Step, events: &Sender<Event>, cancel: &Cancel) -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    let _ = events.send(Event::Line(format!("▸ {}", step.label)));
    let mut command = match &step.admin {
        // Without a prompt when sudoers allows it, else behind one.
        Some(_) if sudo_allows(&step.program, &step.args) => {
            let mut c = Command::new("sudo");
            c.arg("-n").arg(&step.program);
            c
        }
        Some(prompt) => return run_admin(step, prompt, events, cancel),
        None => Command::new(&step.program),
    };
    command
        .args(&step.args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("GIT_TERMINAL_PROMPT", "0");
    let mut child = {
        let mut state = cancel.0.lock().unwrap_or_else(|e| e.into_inner());
        if state.cancelled {
            return Err("Cancelled".to_owned());
        }
        let child = command
            // Its own process group, so Cancel stops everything it started.
            .process_group(0)
            .spawn()
            .map_err(|e| format!("{}: could not start {}: {e}", step.label, step.program))?;
        state.group = child.id();
        child
    };
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    // The last thing the step said on stderr (else stdout) is usually why
    // it failed, so it goes in the failure message.
    let (last_out, last_err) = std::thread::scope(|scope| {
        let out = stdout.map(|out| scope.spawn(|| forward(out, events)));
        let err = stderr.map(|err| scope.spawn(|| forward(err, events)));
        let join = |h: Option<std::thread::ScopedJoinHandle<'_, Option<String>>>| {
            h.and_then(|h| h.join().ok()).flatten()
        };
        (join(out), join(err))
    });
    let group = child.id();
    let status = child.wait().map_err(|e| format!("{}: {e}", step.label));
    {
        let mut state = cancel.0.lock().unwrap_or_else(|e| e.into_inner());
        // The leader can exit on SIGTERM while a child that ignores it lives
        // on in the group: finish the group off before letting go of it.
        if state.cancelled {
            signal_group("-KILL", group);
        }
        state.group = 0;
    }
    let status = status?;
    if status.success() {
        Ok(())
    } else {
        Err(match last_err.or(last_out) {
            Some(why) => format!("{} failed: {why}", step.label),
            None => format!("{} failed ({status})", step.label),
        })
    }
}

/// Send each line on, and return the last non-empty one.
fn forward(stream: impl std::io::Read, events: &Sender<Event>) -> Option<String> {
    let mut last = None;
    for line in BufReader::new(stream).lines().map_while(Result::ok) {
        let line = strip_ansi(&line);
        if !line.trim().is_empty() {
            last = Some(line.trim().to_owned());
        }
        let _ = events.send(Event::Line(line));
    }
    last
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

    #[test]
    fn cancel_kills_a_step_that_ignores_sigterm() {
        let (tx, rx) = std::sync::mpsc::channel();
        let cancel = Cancel::default();
        let handle = {
            let cancel = cancel.clone();
            std::thread::spawn(move || {
                run(
                    vec![Step::new(
                        "stubborn",
                        "/bin/sh",
                        &["-c", "trap '' TERM; echo ready; sleep 30; echo late"],
                    )],
                    tx,
                    cancel,
                )
            })
        };
        assert_eq!(rx.recv().unwrap(), Event::Line("▸ stubborn".into()));
        assert_eq!(rx.recv().unwrap(), Event::Line("ready".into()));
        let started = std::time::Instant::now();
        cancel.cancel();
        let rest: Vec<Event> = rx.iter().collect();
        handle.join().unwrap();
        let took = started.elapsed();
        assert!(took >= CANCEL_GRACE && took < CANCEL_GRACE * 3, "{took:?}");
        assert_eq!(rest.last(), Some(&Event::Done(Err("Cancelled".into()))));
        assert!(!rest.contains(&Event::Line("late".into())));
    }

    #[test]
    fn cancel_kills_children_left_behind_by_the_leader() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("late");
        let (tx, rx) = std::sync::mpsc::channel();
        let cancel = Cancel::default();
        let script = format!(
            "(trap '' TERM; exec >/dev/null 2>&1; sleep 4; touch {}) & echo ready; wait",
            marker.display()
        );
        let handle = {
            let cancel = cancel.clone();
            std::thread::spawn(move || {
                run(
                    vec![Step::new("leader", "/bin/sh", &["-c", &script])],
                    tx,
                    cancel,
                )
            })
        };
        assert_eq!(rx.recv().unwrap(), Event::Line("▸ leader".into()));
        assert_eq!(rx.recv().unwrap(), Event::Line("ready".into()));
        cancel.cancel();
        let _: Vec<Event> = rx.iter().collect();
        handle.join().unwrap();
        std::thread::sleep(std::time::Duration::from_secs(5));
        assert!(!marker.exists(), "the stubborn child outlived Cancel");
    }

    #[test]
    fn a_cancel_before_a_step_starts_stops_it() {
        let (tx, rx) = std::sync::mpsc::channel();
        let cancel = Cancel::default();
        cancel.cancel();
        run(
            vec![Step::new("never", "/bin/sh", &["-c", "echo ran"])],
            tx,
            cancel,
        );
        let events: Vec<Event> = rx.iter().collect();
        assert!(!events.contains(&Event::Line("ran".into())));
        assert_eq!(events.last(), Some(&Event::Done(Err("Cancelled".into()))));
    }
}

/// Whether sudoers lets you run exactly `program args` as root without a
/// password.
fn sudo_allows(program: &str, args: &[String]) -> bool {
    Command::new("sudo")
        .args(["-n", "-l", program])
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Run a step as root behind the system's administrator prompt.
fn run_admin(
    step: &Step,
    prompt: &str,
    events: &Sender<Event>,
    cancel: &Cancel,
) -> Result<(), String> {
    if cancel.is_cancelled() {
        return Err("Cancelled".to_owned());
    }
    // What sudo would set, so the command can tell who asked.
    let uid = unsafe { libc::getuid() };
    let gid = unsafe { libc::getgid() };
    let mut env = vec![
        ("SUDO_UID".to_owned(), uid.to_string()),
        ("SUDO_GID".to_owned(), gid.to_string()),
    ];
    for name in ["USER", "HOME"] {
        if let Ok(value) = std::env::var(name) {
            let key = if name == "USER" { "SUDO_USER" } else { name };
            env.push((key.to_owned(), value));
        }
    }
    let running = match crate::admin::start(prompt, &step.program, &step.args, &env) {
        Ok(running) => running,
        Err(None) => return Err("Cancelled at the administrator prompt".to_owned()),
        Err(Some(why)) => return Err(format!("{} failed: {why}", step.label)),
    };
    let stopper = running.stopper()?;
    {
        let mut state = cancel.0.lock().unwrap_or_else(|e| e.into_inner());
        if state.cancelled {
            stopper.stop();
        }
        state.stopper = Some(stopper);
    }
    let mut last = None;
    let result = running.wait(|line| {
        let line = strip_ansi(&line);
        if !line.trim().is_empty() {
            last = Some(line.clone());
        }
        let _ = events.send(Event::Line(line));
    });
    cancel.0.lock().unwrap_or_else(|e| e.into_inner()).stopper = None;
    match result? {
        0 => Ok(()),
        code => Err(match last {
            Some(why) => format!("{} failed: {why}", step.label),
            None => format!("{} failed (exit status {code})", step.label),
        }),
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

    fn collect(steps: Vec<Step>) -> (Vec<String>, Result<(), String>) {
        let (tx, rx) = std::sync::mpsc::channel();
        run(steps, tx, Cancel::default());
        let mut lines = Vec::new();
        for event in rx {
            match event {
                Event::Line(line) => lines.push(line),
                Event::Done(result) => return (lines, result),
            }
        }
        unreachable!("run always ends with Done")
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
        assert_eq!(url_host("http://[::1]:9184/x"), Some("::1"));
        assert_eq!(url_host("http://me@127.0.0.1:9184"), Some("127.0.0.1"));
        assert_eq!(
            url_host("http://localhost.example.com:9184"),
            Some("localhost.example.com")
        );
        let mut m = f.machines["richard-hp-server"].clone();
        m.agent = Some("http://localhost.example.com:9184".into());
        assert!(!is_here("richard-hp-server", &m, "other"));
    }

    #[test]
    fn syncs_this_mac_by_pulling_as_you_then_switching_as_root() {
        let steps = plan(Action::Sync, &fleet(), "richard-m4-max", &Place::Here).unwrap();
        let labels: Vec<&str> = steps.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Check your checkout and push its commits",
                "Pull this Mac's checkout",
                "Switch this Mac"
            ]
        );
        let pull = &steps[1];
        assert!(pull.admin.is_none(), "git never runs as root");
        let script = &pull.args[1];
        let home = std::env::var("HOME").unwrap();
        assert!(
            script.starts_with(&format!(
                "cd '{home}/dev/gh/richard-better/mac' || exit 1\n"
            )),
            "{script}"
        );
        assert!(script.contains("!= 'refs/heads/main' ]"), "{script}");
        assert!(
            script.ends_with(&fast_forward("'refs/heads/main'")),
            "{script}"
        );

        // Only the rebuild runs as root, as one exact command that a
        // sudoers rule can allow, building the branch's commit.
        let switch = &steps[2];
        assert_eq!(switch.program, "/run/current-system/sw/bin/darwin-rebuild");
        assert_eq!(
            switch.args,
            [
                "switch".to_owned(),
                "--flake".to_owned(),
                format!(
                    "git+file://{}/dev/gh/richard-better/mac?ref=refs/heads/main#richard-m4-max",
                    home.replace(' ', "%20")
                ),
            ]
        );
        assert!(switch.admin.is_some());

        let rollback = plan(Action::Rollback, &fleet(), "richard-m4-max", &Place::Here).unwrap();
        assert_eq!(rollback.len(), 1);
        assert_eq!(rollback[0].args, ["--rollback"]);
        assert!(rollback[0].admin.is_some());
    }

    #[test]
    fn syncs_a_home_manager_machine_over_ssh_with_a_terminal() {
        let place = Place::Ssh("richard@richard-hp-server".into());
        let steps = plan(Action::Sync, &fleet(), "richard-hp-server", &place).unwrap();
        let step = &steps[1];
        assert_eq!(step.program, "ssh");
        assert_eq!(step.args[0], "-tt");
        assert_eq!(step.args[6], "richard@richard-hp-server");
        let remote = &step.args[7];
        assert!(remote.starts_with("sh -lc 'cd ~/"), "{remote}");
        assert!(
            remote.contains("git merge --ff-only FETCH_HEAD"),
            "{remote}"
        );
        assert!(remote.contains("home-manager switch --flake"), "{remote}");
        assert!(remote.contains("#richard@richard-hp-server"), "{remote}");
        assert!(remote.contains("?rev=$(git rev-parse HEAD)"), "{remote}");
        assert!(step.admin.is_none());
        let rollback = plan(Action::Rollback, &fleet(), "richard-hp-server", &place).unwrap();
        assert_eq!(rollback.len(), 1, "rolling back doesn't touch the checkout");
        assert!(rollback[0].args[7].contains("home-manager switch --rollback"));
    }

    #[test]
    fn a_custom_command_runs_only_after_a_successful_pull() {
        let mut f = fleet();
        let machine = f.machines.get_mut("richard-m4-max").unwrap();
        machine.sync = Some(menagerie_fleet::Sync {
            preset: SyncPreset::Custom,
            repo: Some("/nonexistent".into()),
            command: Some("echo one; echo two".into()),
            flake: None,
        });
        f.repo = None;
        let steps = plan(Action::Sync, &f, "richard-m4-max", &Place::Here).unwrap();
        let (lines, result) = collect(steps);
        assert!(result.is_err());
        assert!(!lines.iter().any(|l| l == "two"), "{lines:?}");
    }

    #[test]
    fn machines_without_sync_or_a_way_in_are_refused() {
        let f = fleet();
        assert!(plan(Action::Sync, &f, "homeassistant", &Place::Here).is_err());
        assert!(place("homeassistant", &f.machines["homeassistant"], "other").is_err());
    }

    #[test]
    fn quoting_survives_quotes() {
        assert_eq!(sh_quote("it's"), "'it'\\''s'");
        assert_eq!(sh_path("/srv/a b"), "'/srv/a b'");
    }

    fn result_says(result: &Result<(), String>, text: &str) -> bool {
        result.as_ref().is_err_and(|why| why.contains(text))
    }

    fn git(dir: &Path, args: &[&str]) {
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

    #[test]
    fn the_checkout_step_mentions_dirty_trees_and_pushes_new_commits() {
        let dir = tempfile::tempdir().unwrap();
        let origin = dir.path().join("origin.git");
        let work = dir.path().join("work");
        git(
            dir.path(),
            &[
                "init",
                "-q",
                "--bare",
                "-b",
                "main",
                origin.to_str().unwrap(),
            ],
        );
        git(
            dir.path(),
            &[
                "clone",
                "-q",
                origin.to_str().unwrap(),
                work.to_str().unwrap(),
            ],
        );
        git(&work, &["config", "commit.gpgsign", "false"]);
        git(&work, &["commit", "-q", "--allow-empty", "-m", "one"]);
        git(&work, &["push", "-q", "origin", "HEAD:main"]);
        git(&work, &["fetch", "-q"]);

        // On another branch: refused, even though it is clean.
        git(&work, &["switch", "-q", "-c", "experiment"]);
        let mut wrong = fleet();
        wrong.repo = Some(work.to_string_lossy().into_owned());
        let mut steps = plan(Action::Sync, &wrong, "richard-m4-max", &Place::Here).unwrap();
        steps.truncate(1);
        let (lines, result) = collect(steps);
        assert!(result.is_err());
        assert!(lines.iter().any(|l| l.contains("isn't on")), "{lines:?}");
        assert!(result_says(&result, "isn't on"), "{result:?}");
        git(&work, &["switch", "-q", "main"]);

        // A tag named like the branch makes git abbreviate HEAD to
        // `heads/main`; a branch configured as `heads/main` must not match.
        git(&work, &["tag", "main"]);
        let mut tricky = fleet();
        tricky.repo = Some(work.to_string_lossy().into_owned());
        tricky.branch = Some("heads/main".into());
        let mut steps = plan(Action::Sync, &tricky, "richard-m4-max", &Place::Here).unwrap();
        steps.truncate(1);
        let (lines, result) = collect(steps);
        assert!(result.is_err(), "{lines:?}");
        git(&work, &["tag", "-d", "main"]);

        // Even with untracked files hidden by the user's config.
        git(&work, &["config", "status.showUntrackedFiles", "no"]);
        let mut f = fleet();
        f.repo = Some(work.to_string_lossy().into_owned());
        let check = |f: &Fleet| {
            let mut steps = plan(Action::Sync, f, "richard-m4-max", &Place::Here).unwrap();
            steps.truncate(1);
            collect(steps)
        };

        let (lines, result) = check(&f);
        assert_eq!(result, Ok(()), "{lines:?}");
        assert!(lines.iter().any(|l| l == "Nothing to push."), "{lines:?}");

        // Uncommitted changes don't block: they are left out, and said so.
        std::fs::write(work.join("new.txt"), "untracked").unwrap();
        let (lines, result) = check(&f);
        assert_eq!(result, Ok(()), "{lines:?}");
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("1 uncommitted file stays behind")),
            "{lines:?}"
        );
        std::fs::write(work.join("other.txt"), "untracked").unwrap();
        let (lines, _) = check(&f);
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("2 uncommitted files stay behind")),
            "{lines:?}"
        );

        // The build is of HEAD's commit, from a path with a space in it.
        let spaced = dir.path().join("a b");
        git(
            dir.path(),
            &[
                "clone",
                "-q",
                work.to_str().unwrap(),
                spaced.to_str().unwrap(),
            ],
        );
        let out = Command::new("/bin/sh")
            .current_dir(&spaced)
            .args(["-c", &format!("{FLAKE_AT_HEAD}\necho \"$flake\"")])
            .output()
            .unwrap();
        let head = Command::new("git")
            .arg("-C")
            .arg(&spaced)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        let real = spaced.canonicalize().unwrap();
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            format!(
                "git+file://{}?rev={}",
                real.to_string_lossy().replace(' ', "%20"),
                String::from_utf8_lossy(&head.stdout).trim()
            )
        );
        std::fs::remove_file(work.join("new.txt")).unwrap();
        std::fs::remove_file(work.join("other.txt")).unwrap();

        git(&work, &["commit", "-q", "--allow-empty", "-m", "two"]);
        let (lines, result) = check(&f);
        assert_eq!(result, Ok(()), "{lines:?}");
        let pushed = Command::new("git")
            .arg("-C")
            .arg(&origin)
            .args(["log", "--format=%s", "-1", "main"])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&pushed.stdout).trim(), "two");

        // Origin moved on and you committed too: your commit is rebased onto
        // origin and pushed, never merged, and the dirty files survive.
        let other = dir.path().join("other");
        git(
            dir.path(),
            &[
                "clone",
                "-q",
                origin.to_str().unwrap(),
                other.to_str().unwrap(),
            ],
        );
        git(&other, &["config", "commit.gpgsign", "false"]);
        git(&other, &["commit", "-q", "--allow-empty", "-m", "theirs"]);
        git(&other, &["push", "-q", "origin", "HEAD:main"]);
        git(&work, &["commit", "-q", "--allow-empty", "-m", "mine"]);
        std::fs::write(work.join("new.txt"), "a tool's edit").unwrap();
        let (lines, result) = check(&f);
        assert_eq!(result, Ok(()), "{lines:?}");
        assert!(
            lines.iter().any(|l| l.starts_with("Rebasing your 1")),
            "{lines:?}"
        );
        let log = Command::new("git")
            .arg("-C")
            .arg(&origin)
            .args(["log", "--format=%s|%p", "main"])
            .output()
            .unwrap();
        let log = String::from_utf8_lossy(&log.stdout);
        let subjects: Vec<&str> = log.lines().map(|l| l.split('|').next().unwrap()).collect();
        assert_eq!(subjects[..2], ["mine", "theirs"], "{log}");
        assert!(
            log.lines()
                .all(|l| !l.rsplit('|').next().unwrap().contains(' ')),
            "a merge commit: {log}"
        );
        assert_eq!(
            std::fs::read_to_string(work.join("new.txt")).unwrap(),
            "a tool's edit"
        );
    }

    /// With `pull.rebase` set and a dirty tree, `git pull` refuses even when
    /// there is nothing to pull; the fast-forward goes ahead.
    #[test]
    fn fast_forwards_past_pull_rebase_and_a_dirty_tree() {
        let dir = tempfile::tempdir().unwrap();
        let origin = dir.path().join("origin.git");
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        let o = origin.to_str().unwrap();
        git(dir.path(), &["init", "-q", "--bare", "-b", "main", o]);
        for work in [&a, &b] {
            git(dir.path(), &["clone", "-q", o, work.to_str().unwrap()]);
            git(work, &["config", "commit.gpgsign", "false"]);
            git(work, &["config", "pull.rebase", "true"]);
        }
        std::fs::write(a.join("tool.json"), "{}\n").unwrap();
        git(&a, &["add", "-A"]);
        git(&a, &["commit", "-q", "-m", "one"]);
        git(&a, &["push", "-q", "origin", "HEAD:main"]);
        git(&b, &["pull", "-q", "origin", "main"]);
        // b's tool rewrites its settings; a pushes another change.
        std::fs::write(b.join("tool.json"), "{\"edited\": true}\n").unwrap();
        std::fs::write(a.join("other.nix"), "{ }\n").unwrap();
        git(&a, &["add", "-A"]);
        git(&a, &["commit", "-q", "-m", "two"]);
        git(&a, &["push", "-q", "origin", "HEAD:main"]);
        let run = || {
            Command::new("/bin/sh")
                .current_dir(&b)
                .args(["-c", &fast_forward("'refs/heads/main'")])
                .output()
                .unwrap()
        };
        let out = run();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(b.join("other.nix").exists());
        assert_eq!(
            std::fs::read_to_string(b.join("tool.json")).unwrap(),
            "{\"edited\": true}\n",
            "the uncommitted edit stays"
        );
        // Nothing new: still fine.
        assert!(run().status.success());
    }

    #[test]
    fn runs_steps_in_order_and_stops_at_a_failure() {
        let (lines, result) = collect(vec![
            Step::new("first", "/bin/sh", &["-c", "echo one; echo two >&2"]),
            Step::new(
                "second",
                "/bin/sh",
                &["-c", "printf '\\033[1mbold\\033[0m\\n'; exit 3"],
            ),
            Step::new("never", "/bin/sh", &["-c", "echo nope"]),
        ]);
        assert!(lines.iter().any(|l| l == "one") && lines.iter().any(|l| l == "two"));
        assert!(lines.iter().any(|l| l == "bold"));
        assert!(!lines.iter().any(|l| l == "nope"));
        let why = result.unwrap_err();
        assert_eq!(why, "second failed: bold");
    }
}
