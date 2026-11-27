//! The fleet file: which machines exist, how to reach them, how to sync
//! them, and which services they expose.
//!
//! The file is YAML. JSON is valid YAML, so a Nix-generated JSON file
//! parses the same way. The format is versioned from the first release;
//! see [`CURRENT_VERSION`].

use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// The fleet file version this build reads.
pub const CURRENT_VERSION: u32 = 1;

/// A parsed, validated fleet file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fleet {
    pub version: u32,
    /// Your local checkout of the configuration repo, which machines are
    /// compared against. `~` expands to the home directory.
    #[serde(default)]
    pub repo: Option<String>,
    /// The branch machines should be on. Defaults to `main`.
    #[serde(default)]
    pub branch: Option<String>,
    /// A shell command that drafts commit messages for your checkout's
    /// changes, such as a coding agent's CLI. It runs in the checkout, gets
    /// the diff on stdin, and prints the message; the first line is used.
    #[serde(default)]
    pub commit_message: Option<String>,
    /// Machines in the order the file lists them, keyed by hostname.
    #[serde(default)]
    pub machines: IndexMap<String, Machine>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Machine {
    /// What the machine is for, in a few words: "Media server, exit node".
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub device: Device,
    /// Base URL of the machine's Menagerie agent. Absent for machines
    /// that don't run one; those still show their services.
    #[serde(default)]
    pub agent: Option<String>,
    /// SSH destination used for syncing, as `user@host`.
    #[serde(default)]
    pub ssh: Option<String>,
    #[serde(default)]
    pub sync: Option<Sync>,
    #[serde(default)]
    pub services: Vec<Service>,
}

/// The kind of hardware, used to pick the machine's illustration.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Device {
    Laptop,
    Desktop,
    Server,
    /// A small appliance such as a Raspberry Pi.
    Box,
    Phone,
    #[default]
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sync {
    pub preset: SyncPreset,
    /// Path of the configuration repository on the machine.
    #[serde(default)]
    pub repo: Option<String>,
    /// The command to run when `preset` is `custom`.
    #[serde(default)]
    pub command: Option<String>,
    /// The flake output to build, such as `richard-m4-max` or
    /// `richard@richard-hp-server`. Defaults to the machine's name for
    /// nix-darwin and NixOS, and to the `ssh` target (or the machine's name)
    /// for Home Manager.
    #[serde(default)]
    pub flake: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SyncPreset {
    NixDarwin,
    Nixos,
    HomeManager,
    Custom,
}

impl SyncPreset {
    /// How the preset is named in prose.
    pub fn label(self) -> &'static str {
        match self {
            SyncPreset::NixDarwin => "nix-darwin",
            SyncPreset::Nixos => "NixOS",
            SyncPreset::HomeManager => "Home Manager",
            SyncPreset::Custom => "Custom command",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Service {
    pub name: String,
    /// A key into the bundled icon set, such as `jellyfin`.
    #[serde(default)]
    pub icon: Option<String>,
    /// Where clicking the service takes you.
    pub url: String,
    /// A URL that answers 2xx when the service is healthy.
    #[serde(default)]
    pub health: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{0}")]
    Parse(#[from] serde_yaml_ng::Error),
    #[error(
        "fleet file version {found} is not supported; this build reads version {CURRENT_VERSION}"
    )]
    UnsupportedVersion { found: u32 },
    #[error("machine {machine}: {problem}")]
    Invalid { machine: String, problem: String },
}

impl Fleet {
    /// The branch machines should be on.
    pub fn branch(&self) -> &str {
        self.branch.as_deref().unwrap_or("main")
    }

    /// `repo` with a leading `~` expanded.
    pub fn repo_path(&self) -> Option<PathBuf> {
        self.repo.as_deref().map(expand_home)
    }

    /// Parse and validate a fleet file's contents.
    pub fn from_yaml(text: &str) -> Result<Self, Error> {
        let fleet: Fleet = serde_yaml_ng::from_str(text)?;
        fleet.validate()?;
        Ok(fleet)
    }

    /// Read, parse and validate the fleet file at `path`.
    pub fn load(path: &Path) -> Result<Self, Error> {
        let text = std::fs::read_to_string(path).map_err(|source| Error::Read {
            path: path.to_owned(),
            source,
        })?;
        Self::from_yaml(&text)
    }

    fn validate(&self) -> Result<(), Error> {
        if self.version != CURRENT_VERSION {
            return Err(Error::UnsupportedVersion {
                found: self.version,
            });
        }
        for (name, machine) in &self.machines {
            let invalid = |problem: &str| Error::Invalid {
                machine: name.clone(),
                problem: problem.to_owned(),
            };
            if let Some(sync) = &machine.sync {
                if sync.preset == SyncPreset::Custom && sync.command.is_none() {
                    return Err(invalid("sync preset `custom` needs a `command`"));
                }
                if sync.preset != SyncPreset::Custom && sync.command.is_some() {
                    return Err(invalid("`command` is only used with sync preset `custom`"));
                }
            }
            for service in &machine.services {
                if service.name.trim().is_empty() {
                    return Err(invalid("a service has an empty name"));
                }
            }
        }
        Ok(())
    }
}

/// Expand a leading `~` or `~/` to the home directory.
pub fn expand_home(path: &str) -> PathBuf {
    let home = || std::env::var_os("HOME").map(PathBuf::from);
    match path.strip_prefix('~') {
        Some("") => home().unwrap_or_else(|| PathBuf::from(path)),
        Some(rest) if rest.starts_with('/') => home()
            .map(|h| h.join(&rest[1..]))
            .unwrap_or_else(|| PathBuf::from(path)),
        _ => PathBuf::from(path),
    }
}

/// Where the fleet file lives when nothing says otherwise:
/// `$XDG_CONFIG_HOME/menagerie/fleet.yaml`, else `~/.config/menagerie/fleet.yaml`.
pub fn default_path() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(config.join("menagerie").join("fleet.yaml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = include_str!("../../../examples/fleet.yaml");

    #[test]
    fn parses_the_example_in_order() {
        let fleet = Fleet::from_yaml(EXAMPLE).unwrap();
        let names: Vec<_> = fleet.machines.keys().map(String::as_str).collect();
        assert_eq!(
            names,
            [
                "richard-m4-max",
                "richard-hp-server",
                "homeassistant",
                "iphone-15-pro-max"
            ]
        );
        let server = &fleet.machines["richard-hp-server"];
        assert_eq!(server.device, Device::Laptop);
        assert_eq!(
            server.sync.as_ref().unwrap().preset,
            SyncPreset::HomeManager
        );
        assert_eq!(server.services.len(), 2);
        assert_eq!(server.services[0].icon.as_deref(), Some("jellyfin"));
        assert!(fleet.machines["homeassistant"].agent.is_none());
    }

    #[test]
    fn json_is_accepted() {
        let fleet = Fleet::from_yaml(
            r#"{"version": 1, "machines": {"box": {"device": "box", "services": []}}}"#,
        )
        .unwrap();
        assert_eq!(fleet.machines["box"].device, Device::Box);
    }

    #[test]
    fn rejects_other_versions() {
        let err = Fleet::from_yaml("version: 2\nmachines: {}\n").unwrap_err();
        assert!(matches!(err, Error::UnsupportedVersion { found: 2 }));
    }

    #[test]
    fn rejects_unknown_fields() {
        let err = Fleet::from_yaml("version: 1\nmachines:\n  a:\n    rol: typo\n").unwrap_err();
        assert!(err.to_string().contains("rol"), "{err}");
    }

    #[test]
    fn custom_sync_needs_a_command() {
        let err = Fleet::from_yaml("version: 1\nmachines:\n  a:\n    sync: { preset: custom }\n")
            .unwrap_err();
        assert!(matches!(err, Error::Invalid { .. }), "{err}");
    }

    #[test]
    fn repo_and_branch() {
        let fleet = Fleet::from_yaml(EXAMPLE).unwrap();
        assert_eq!(fleet.branch(), "main");
        let repo = fleet.repo_path().unwrap();
        assert!(
            repo.ends_with("dev/gh/richard-better/mac"),
            "{}",
            repo.display()
        );
        assert!(!repo.starts_with("~"));
        assert_eq!(expand_home("/abs"), PathBuf::from("/abs"));
        assert_eq!(expand_home("~user/x"), PathBuf::from("~user/x"));
    }

    #[test]
    fn device_defaults_to_other() {
        let fleet = Fleet::from_yaml("version: 1\nmachines:\n  a: {}\n").unwrap();
        assert_eq!(fleet.machines["a"].device, Device::Other);
    }
}
