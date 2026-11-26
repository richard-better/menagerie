//! The state both views read: the fleet file, what each machine's agent
//! last reported, service health, how each machine's configuration compares
//! with your checkout, and which machine the window is showing.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use futures::channel::mpsc::UnboundedSender;
use menagerie_fleet::{Fleet, Machine};
use menagerie_protocol as protocol;

/// A machine whose agent has not answered for this long counts as offline.
pub const STALE_AFTER: Duration = Duration::from_secs(35);

/// What the app knows about one machine right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Running the latest commit. `dirty`: built with uncommitted changes.
    InSync {
        dirty: bool,
    },
    Drift {
        ahead: u32,
        behind: u32,
        dirty: bool,
    },
    /// The agent answers, but how its configuration compares is unknown.
    Online,
    NoAgent,
    Offline,
}

impl Status {
    pub fn is_online(self) -> bool {
        matches!(
            self,
            Status::InSync { .. } | Status::Drift { .. } | Status::Online
        )
    }

    /// Behind, ahead, or built from changes no commit has.
    pub fn needs_sync(self) -> bool {
        matches!(self, Status::Drift { .. } | Status::InSync { dirty: true })
    }

    /// The words shown after a machine's name, the same in both views.
    pub fn label(self) -> String {
        match self {
            Status::InSync { dirty: false } => "in sync".into(),
            Status::InSync { dirty: true } => "modified".into(),
            Status::Drift { ahead, behind, .. } => drift_label(ahead, behind),
            Status::Online => "online".into(),
            Status::NoAgent => "no agent".into(),
            Status::Offline => "offline".into(),
        }
    }
}

/// `↑2 ↓4`, leaving out a direction that is zero.
pub fn drift_label(ahead: u32, behind: u32) -> String {
    match (ahead, behind) {
        (0, b) => format!("↓{b}"),
        (a, 0) => format!("↑{a}"),
        (a, b) => format!("↑{a} ↓{b}"),
    }
}

/// The last word from one machine's agent.
#[derive(Debug, Clone, Default)]
pub struct Live {
    pub status: Option<protocol::Status>,
    /// When `status` arrived, by this app's clock.
    pub received: Option<Instant>,
    /// Why the latest poll failed, if it did.
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Health {
    Up { millis: u32 },
    Down(String),
}

/// Your local checkout, against `origin/<branch>`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Checkout {
    /// Commits on HEAD not yet pushed.
    pub unpushed: u32,
    /// Commits on origin not yet pulled.
    pub unpulled: u32,
    pub dirty: bool,
}

/// How far a revision is `(ahead, behind)` your upstream branch, or why
/// that could not be worked out.
pub type Drift = Result<(u32, u32), String>;

/// One poll's findings, applied to the store in one go.
#[derive(Debug, Default)]
pub struct Update {
    pub live: Vec<(String, Result<protocol::Status, String>)>,
    pub health: Vec<((String, usize), Health)>,
    pub drift: Vec<(String, Drift)>,
    pub checkout: Option<Result<Checkout, String>>,
}

pub struct Store {
    pub path: PathBuf,
    pub fleet: Option<Fleet>,
    /// Why the fleet file could not be used, if it could not.
    pub error: Option<String>,
    pub selected: Option<String>,
    pub live: HashMap<String, Live>,
    pub health: HashMap<(String, usize), Health>,
    pub drift: HashMap<String, Drift>,
    pub checkout: Option<Result<Checkout, String>>,
    /// When the last poll finished.
    pub updated: Option<Instant>,
    /// Asks the poller to poll now.
    pub poke: Option<UnboundedSender<()>>,
}

impl Store {
    pub fn load(path: PathBuf) -> Self {
        let mut store = Self {
            path,
            fleet: None,
            error: None,
            selected: None,
            live: HashMap::new(),
            health: HashMap::new(),
            drift: HashMap::new(),
            checkout: None,
            updated: None,
            poke: None,
        };
        store.reload();
        store
    }

    /// Read the fleet file again, keeping the selection if that machine
    /// still exists, and ask for a fresh poll.
    pub fn reload(&mut self) {
        match Fleet::load(&self.path) {
            Ok(fleet) => {
                self.error = None;
                self.fleet = Some(fleet);
            }
            Err(err) => {
                self.error = Some(err.to_string());
                self.fleet = None;
            }
        }
        let still_there = self
            .selected
            .as_ref()
            .is_some_and(|name| self.machine(name).is_some());
        if !still_there {
            self.selected = self.machines().first().map(|(name, _)| name.to_string());
        }
        self.refresh();
    }

    /// Ask the poller to poll now rather than at its next tick.
    pub fn refresh(&self) {
        if let Some(poke) = &self.poke {
            let _ = poke.unbounded_send(());
        }
    }

    pub fn file_exists(&self) -> bool {
        self.path.exists()
    }

    pub fn machines(&self) -> Vec<(&str, &Machine)> {
        self.fleet
            .as_ref()
            .map(|f| f.machines.iter().map(|(n, m)| (n.as_str(), m)).collect())
            .unwrap_or_default()
    }

    pub fn machine(&self, name: &str) -> Option<&Machine> {
        self.fleet.as_ref()?.machines.get(name)
    }

    /// The agent's latest report, if it is recent enough to trust.
    pub fn report(&self, name: &str) -> Option<&protocol::Status> {
        let live = self.live.get(name)?;
        let fresh = live.received.is_some_and(|t| t.elapsed() < STALE_AFTER);
        fresh.then_some(live.status.as_ref()).flatten()
    }

    pub fn revision(&self, name: &str) -> Option<&protocol::Revision> {
        self.report(name)?.probes.revision.value.as_ref()
    }

    pub fn status(&self, name: &str) -> Status {
        match self.machine(name) {
            Some(machine) if machine.agent.is_some() => {}
            _ => return Status::NoAgent,
        }
        if self.report(name).is_none() {
            return Status::Offline;
        }
        let Some(revision) = self.revision(name) else {
            return Status::Online;
        };
        match self.drift.get(&revision.rev) {
            Some(Ok((0, 0))) => Status::InSync {
                dirty: revision.dirty,
            },
            Some(Ok((ahead, behind))) => Status::Drift {
                ahead: *ahead,
                behind: *behind,
                dirty: revision.dirty,
            },
            _ => Status::Online,
        }
    }

    pub fn select(&mut self, name: &str) {
        if self.machine(name).is_some() {
            self.selected = Some(name.to_owned());
        }
    }

    pub fn needs_sync_count(&self) -> usize {
        self.machines()
            .iter()
            .filter(|(name, _)| self.status(name).needs_sync())
            .count()
    }

    /// `1 needs sync · 3/4 online`, or just the online count when nothing
    /// needs a sync.
    pub fn summary(&self) -> String {
        let machines = self.machines();
        let online = machines
            .iter()
            .filter(|(n, _)| self.status(n).is_online())
            .count();
        let needs = self.needs_sync_count();
        let total = machines.len();
        if needs > 0 {
            format!("{needs} needs sync · {online}/{total} online")
        } else {
            format!("{online}/{total} online")
        }
    }

    /// "Updated 8s ago", or `None` before the first poll.
    pub fn updated_label(&self) -> Option<String> {
        let secs = self.updated?.elapsed().as_secs();
        Some(match secs {
            0..=4 => "Updated just now".to_owned(),
            5..=59 => format!("Updated {secs}s ago"),
            _ => format!("Updated {}m ago", secs / 60),
        })
    }

    pub fn apply(&mut self, update: Update) {
        let now = Instant::now();
        for (name, result) in update.live {
            let live = self.live.entry(name).or_default();
            match result {
                Ok(status) => {
                    live.status = Some(status);
                    live.received = Some(now);
                    live.error = None;
                }
                Err(error) => live.error = Some(error),
            }
        }
        for (key, health) in update.health {
            self.health.insert(key, health);
        }
        for (rev, drift) in update.drift {
            self.drift.insert(rev, drift);
        }
        if let Some(checkout) = update.checkout {
            self.checkout = Some(checkout);
        }
        self.updated = Some(now);
    }

    /// Write the bundled example to the fleet file's path, for a first run.
    pub fn create_from_example(&mut self) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&self.path, EXAMPLE)?;
        self.reload();
        Ok(())
    }
}

pub const EXAMPLE: &str = include_str!("../../../examples/fleet.yaml");

#[cfg(test)]
mod tests {
    use menagerie_protocol::{AgentInfo, Probe, Probes, Revision, RevisionSource};

    use super::*;

    fn report(rev: &str) -> protocol::Status {
        protocol::Status {
            version: 1,
            host: "h".into(),
            agent: AgentInfo {
                version: "0".into(),
                now: 0,
            },
            probes: Probes {
                revision: Probe {
                    value: Revision::parse(rev, None, RevisionSource::NixDarwin),
                    ..Probe::default()
                },
                ..Probes::default()
            },
            load_history: vec![],
        }
    }

    fn example_store() -> (Store, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "menagerie-store-{}-{}",
            std::process::id(),
            rand_suffix()
        ));
        let mut store = Store::load(dir.join("fleet.yaml"));
        store.create_from_example().unwrap();
        (store, dir)
    }

    fn rand_suffix() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }

    #[test]
    fn drift_label_drops_zero_directions() {
        assert_eq!(drift_label(0, 4), "↓4");
        assert_eq!(drift_label(2, 0), "↑2");
        assert_eq!(drift_label(2, 4), "↑2 ↓4");
    }

    #[test]
    fn statuses_follow_what_the_agents_report() {
        let (mut store, dir) = example_store();
        assert_eq!(store.selected.as_deref(), Some("richard-m4-max"));
        assert_eq!(store.status("richard-hp-server"), Status::Offline);
        assert_eq!(store.status("homeassistant"), Status::NoAgent);
        assert_eq!(store.summary(), "0/4 online");

        store.apply(Update {
            live: vec![
                ("richard-m4-max".into(), Ok(report("aaaaaaa1"))),
                ("richard-hp-server".into(), Ok(report("bbbbbbb2-dirty"))),
            ],
            drift: vec![
                ("aaaaaaa1".into(), Ok((0, 0))),
                ("bbbbbbb2".into(), Ok((0, 4))),
            ],
            ..Update::default()
        });
        assert_eq!(
            store.status("richard-m4-max"),
            Status::InSync { dirty: false }
        );
        assert_eq!(
            store.status("richard-hp-server"),
            Status::Drift {
                ahead: 0,
                behind: 4,
                dirty: true
            }
        );
        assert_eq!(store.summary(), "1 needs sync · 2/4 online");

        // A failed poll keeps the last report until it goes stale.
        store.apply(Update {
            live: vec![("richard-m4-max".into(), Err("timed out".into()))],
            ..Update::default()
        });
        assert!(store.status("richard-m4-max").is_online());
        assert_eq!(
            store.live["richard-m4-max"].error.as_deref(),
            Some("timed out")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unknown_revision_is_online_not_in_sync() {
        let (mut store, dir) = example_store();
        store.apply(Update {
            live: vec![("richard-m4-max".into(), Ok(report("ccccccc3")))],
            drift: vec![("ccccccc3".into(), Err("not in checkout".into()))],
            ..Update::default()
        });
        assert_eq!(store.status("richard-m4-max"), Status::Online);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
