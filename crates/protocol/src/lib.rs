//! What a Menagerie agent reports at `GET /status`, shared by the agent
//! that writes it and the app that reads it.
//!
//! Every probe carries the time it was measured and the agent's own clock
//! is included, so a reader can tell how old each value is without
//! trusting its own clock to match.

use serde::{Deserialize, Serialize};

/// The protocol version this build speaks.
pub const VERSION: u32 = 1;

/// Milliseconds since the Unix epoch.
pub type Millis = i64;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Status {
    pub version: u32,
    /// The machine's hostname, as it knows itself.
    pub host: String,
    pub agent: AgentInfo,
    pub probes: Probes,
    /// One-minute load average, sampled once a minute, oldest first,
    /// covering at most the last 24 hours.
    #[serde(default)]
    pub load_history: Vec<Sample>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentInfo {
    pub version: String,
    /// The agent's clock when it answered.
    pub now: Millis,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    pub at: Millis,
    pub value: f64,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Probes {
    pub load: Probe<Load>,
    pub memory: Probe<Usage>,
    pub disk: Probe<Usage>,
    /// `value: None` with no error means the machine has no battery.
    pub battery: Probe<Battery>,
    /// `value: None` with no error means no sensor could be read.
    pub temperature: Probe<Temperature>,
    pub revision: Probe<Revision>,
    pub system: Probe<SystemInfo>,
}

/// One probe's latest result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Probe<T> {
    /// The last good value. Kept when a later run fails.
    pub value: Option<T>,
    /// When `value` was measured.
    pub at: Option<Millis>,
    /// How often the agent refreshes this probe, in seconds.
    pub every_s: u32,
    /// Why the latest run failed, if it did.
    pub error: Option<String>,
}

impl<T> Default for Probe<T> {
    fn default() -> Self {
        Self {
            value: None,
            at: None,
            every_s: 0,
            error: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Load {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
    /// Logical CPUs, to put the averages in proportion.
    pub cpus: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub used_bytes: u64,
    pub total_bytes: u64,
}

impl Usage {
    pub fn fraction(&self) -> f64 {
        if self.total_bytes == 0 {
            0.0
        } else {
            self.used_bytes as f64 / self.total_bytes as f64
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Battery {
    /// 0 to 100.
    pub percent: f64,
    pub charging: bool,
    /// Full-charge capacity as a percentage of design capacity.
    pub health_percent: Option<f64>,
    pub cycles: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Temperature {
    /// The hottest sensor the agent could read.
    pub celsius: f64,
}

/// Which commit of the configuration repo built what the machine runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Revision {
    /// The full commit hash, without any `-dirty` suffix.
    pub rev: String,
    /// Built from a tree with uncommitted changes.
    pub dirty: bool,
    /// The generation number of the profile it came from, when known.
    pub generation: Option<u64>,
    pub source: RevisionSource,
}

impl Revision {
    /// Parse a stamped revision such as `f460dc1…` or `f460dc1…-dirty`.
    pub fn parse(stamp: &str, generation: Option<u64>, source: RevisionSource) -> Option<Self> {
        let stamp = stamp.trim();
        let (rev, dirty) = match stamp.strip_suffix("-dirty") {
            Some(rev) => (rev, true),
            None => (stamp, false),
        };
        is_commit_hash(rev).then(|| Self {
            rev: rev.to_owned(),
            dirty,
            generation,
            source,
        })
    }

    /// Whether `rev` is a plausible commit hash. A reader must check this
    /// before using a revision that came over the network.
    pub fn is_valid(&self) -> bool {
        is_commit_hash(&self.rev)
    }

    pub fn short(&self) -> &str {
        self.rev.get(..7).unwrap_or(&self.rev)
    }
}

/// 7 to 64 hex digits: an abbreviated or full SHA-1 or SHA-256 hash.
fn is_commit_hash(rev: &str) -> bool {
    (7..=64).contains(&rev.len()) && rev.chars().all(|c| c.is_ascii_hexdigit())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RevisionSource {
    NixDarwin,
    Nixos,
    HomeManager,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemInfo {
    pub os: String,
    pub os_version: Option<String>,
    pub uptime_s: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_clean_and_dirty_stamps() {
        let clean = Revision::parse("9b06a19abc\n", Some(4), RevisionSource::NixDarwin).unwrap();
        assert_eq!((clean.rev.as_str(), clean.dirty), ("9b06a19abc", false));
        assert_eq!(clean.short(), "9b06a19");
        let dirty = Revision::parse("9b06a19abc-dirty", None, RevisionSource::HomeManager).unwrap();
        assert!(dirty.dirty);
        assert!(Revision::parse("unknown", None, RevisionSource::Nixos).is_none());
        assert!(Revision::parse("--output=x", None, RevisionSource::Nixos).is_none());
    }

    #[test]
    fn short_is_safe_on_any_string() {
        let odd = Revision {
            rev: "ééééé".into(),
            dirty: false,
            generation: None,
            source: RevisionSource::Nixos,
        };
        assert!(!odd.is_valid());
        assert_eq!(odd.short(), "ééééé");
    }

    #[test]
    fn missing_history_defaults_to_empty() {
        let json = serde_json::json!({
            "version": 1,
            "host": "h",
            "agent": { "version": "0.1.0", "now": 1 },
            "probes": Probes::default(),
        });
        let status: Status = serde_json::from_value(json).unwrap();
        assert!(status.load_history.is_empty());
    }
}
