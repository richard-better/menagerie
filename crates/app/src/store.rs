//! The state both views read: the fleet file, what is known about each
//! machine, and which machine the window is showing.

use std::path::PathBuf;

use menagerie_fleet::{Fleet, Machine};

/// What the app knows about one machine right now.
///
/// Until the agent exists every machine is either `NoAgent` (the fleet
/// file lists no agent for it) or `Offline` (it lists one, and nothing has
/// answered).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    InSync,
    Drift { ahead: u32, behind: u32 },
    NoAgent,
    Offline,
}

impl Status {
    pub fn is_online(self) -> bool {
        matches!(self, Status::InSync | Status::Drift { .. })
    }

    pub fn needs_sync(self) -> bool {
        matches!(self, Status::Drift { .. })
    }

    /// The words shown after a machine's name, the same in both views.
    pub fn label(self) -> String {
        match self {
            Status::InSync => "in sync".into(),
            Status::Drift { ahead, behind } => drift_label(ahead, behind),
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

pub struct Store {
    pub path: PathBuf,
    pub fleet: Option<Fleet>,
    /// Why the fleet file could not be used, if it could not.
    pub error: Option<String>,
    pub selected: Option<String>,
}

impl Store {
    pub fn load(path: PathBuf) -> Self {
        let mut store = Self {
            path,
            fleet: None,
            error: None,
            selected: None,
        };
        store.reload();
        store
    }

    /// Read the fleet file again, keeping the selection if that machine
    /// still exists.
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

    pub fn status(&self, name: &str) -> Status {
        match self.machine(name) {
            Some(machine) if machine.agent.is_some() => Status::Offline,
            _ => Status::NoAgent,
        }
    }

    pub fn select(&mut self, name: &str) {
        if self.machine(name).is_some() {
            self.selected = Some(name.to_owned());
        }
    }

    /// `1 needs sync · 3/4 online`, or just the online count when nothing
    /// needs a sync.
    pub fn summary(&self) -> String {
        let machines = self.machines();
        let statuses: Vec<_> = machines.iter().map(|(n, _)| self.status(n)).collect();
        let online = statuses.iter().filter(|s| s.is_online()).count();
        let needs = statuses.iter().filter(|s| s.needs_sync()).count();
        let total = machines.len();
        if needs > 0 {
            format!("{needs} needs sync · {online}/{total} online")
        } else {
            format!("{online}/{total} online")
        }
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
    use super::*;

    #[test]
    fn drift_label_drops_zero_directions() {
        assert_eq!(drift_label(0, 4), "↓4");
        assert_eq!(drift_label(2, 0), "↑2");
        assert_eq!(drift_label(2, 4), "↑2 ↓4");
    }

    #[test]
    fn statuses_without_an_agent_running() {
        let dir = std::env::temp_dir().join(format!("menagerie-store-{}", std::process::id()));
        let path = dir.join("fleet.yaml");
        let mut store = Store::load(path.clone());
        assert!(store.error.is_some());
        store.create_from_example().unwrap();
        assert_eq!(store.selected.as_deref(), Some("richard-m4-max"));
        assert_eq!(store.status("richard-hp-server"), Status::Offline);
        assert_eq!(store.status("homeassistant"), Status::NoAgent);
        assert_eq!(store.summary(), "0/4 online");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
