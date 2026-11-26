//! The cache the HTTP handlers read, and the loop that keeps it fresh.
//!
//! Each probe refreshes on its own interval, so a request never waits on a
//! measurement. After the machine sleeps, every probe runs at once rather
//! than serving values from before the sleep.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use menagerie_protocol::{AgentInfo, Millis, Probe, Probes, Sample, Status, VERSION};
use sysinfo::System;

use crate::probes;
use crate::revision::{self, Roots};

const HISTORY_EVERY: Duration = Duration::from_secs(60);
/// 24 hours of one-minute samples.
const HISTORY_LEN: usize = 24 * 60;
/// A gap this much longer than the tick means the machine slept.
const SLEEP_GAP: Duration = Duration::from_secs(30);
const TICK: Duration = Duration::from_secs(1);

pub fn now_ms() -> Millis {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as Millis)
        .unwrap_or(0)
}

#[derive(Clone)]
pub struct Shared(Arc<Mutex<Snapshot>>);

struct Snapshot {
    host: String,
    probes: Probes,
    history: VecDeque<Sample>,
}

impl Shared {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(Snapshot {
            host: probes::host_name(),
            probes: Probes::default(),
            history: VecDeque::with_capacity(HISTORY_LEN),
        })))
    }

    pub fn status(&self) -> Status {
        let snapshot = self.0.lock().unwrap_or_else(|e| e.into_inner());
        Status {
            version: VERSION,
            host: snapshot.host.clone(),
            agent: AgentInfo {
                version: env!("CARGO_PKG_VERSION").to_owned(),
                now: now_ms(),
            },
            probes: snapshot.probes.clone(),
            load_history: snapshot.history.iter().cloned().collect(),
        }
    }

    fn update(&self, f: impl FnOnce(&mut Snapshot)) {
        let mut snapshot = self.0.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut snapshot);
    }
}

impl Default for Shared {
    fn default() -> Self {
        Self::new()
    }
}

fn record<T>(probe: &mut Probe<T>, every: Duration, result: Result<T, String>) {
    probe.every_s = every.as_secs() as u32;
    match result {
        Ok(value) => {
            probe.value = Some(value);
            probe.at = Some(now_ms());
            probe.error = None;
        }
        Err(error) => probe.error = Some(error),
    }
}

/// For probes where "nothing to measure" is a valid answer (no battery).
fn record_optional<T>(probe: &mut Probe<T>, every: Duration, result: Result<Option<T>, String>) {
    probe.every_s = every.as_secs() as u32;
    match result {
        Ok(value) => {
            probe.value = value;
            probe.at = Some(now_ms());
            probe.error = None;
        }
        Err(error) => probe.error = Some(error),
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Load,
    Memory,
    Disk,
    Battery,
    Temperature,
    Revision,
    System,
    History,
}

const SCHEDULE: [(Kind, Duration); 8] = [
    (Kind::Load, Duration::from_secs(5)),
    (Kind::Memory, Duration::from_secs(5)),
    (Kind::Disk, Duration::from_secs(300)),
    (Kind::Battery, Duration::from_secs(30)),
    (Kind::Temperature, Duration::from_secs(30)),
    // Reading two small files; cheap enough to catch a switch within seconds.
    (Kind::Revision, Duration::from_secs(10)),
    (Kind::System, Duration::from_secs(60)),
    (Kind::History, HISTORY_EVERY),
];

pub struct Prober {
    shared: Shared,
    roots: Roots,
    /// Kept between runs: sysinfo refreshes memory figures in place.
    system: Result<System, String>,
}

impl Prober {
    pub fn new(shared: Shared, roots: Roots) -> Self {
        Self {
            shared,
            roots,
            system: System::new().map_err(|e| e.to_string()),
        }
    }

    fn run(&mut self, kind: Kind, every: Duration) {
        match kind {
            Kind::Load => {
                let r = probes::load();
                self.shared.update(|s| record(&mut s.probes.load, every, r));
            }
            Kind::Memory => {
                let r = match &mut self.system {
                    Ok(system) => probes::memory(system),
                    Err(e) => Err(e.clone()),
                };
                self.shared
                    .update(|s| record(&mut s.probes.memory, every, r));
            }
            Kind::Disk => {
                let r = probes::disk();
                self.shared.update(|s| record(&mut s.probes.disk, every, r));
            }
            Kind::Battery => {
                let r = probes::battery();
                self.shared
                    .update(|s| record_optional(&mut s.probes.battery, every, r));
            }
            Kind::Temperature => {
                let r = probes::temperature();
                self.shared
                    .update(|s| record_optional(&mut s.probes.temperature, every, r));
            }
            Kind::Revision => {
                let r = revision::read(&self.roots);
                self.shared
                    .update(|s| record_optional(&mut s.probes.revision, every, r));
            }
            Kind::System => {
                let r = probes::system_info();
                self.shared
                    .update(|s| record(&mut s.probes.system, every, r));
            }
            Kind::History => {
                if let Ok(load) = probes::load() {
                    self.shared.update(|s| {
                        if s.history.len() == HISTORY_LEN {
                            s.history.pop_front();
                        }
                        s.history.push_back(Sample {
                            at: now_ms(),
                            value: load.one,
                        });
                    });
                }
            }
        }
    }

    /// Run every probe once, now.
    pub fn run_all(&mut self) {
        for (kind, every) in SCHEDULE {
            self.run(kind, every);
        }
    }

    /// Keep the cache fresh for ever, starting one interval after the
    /// `run_all` that filled it. Wall-clock time drives the schedule,
    /// because the monotonic clock stops while macOS sleeps.
    pub fn run_forever(mut self) -> ! {
        let start = SystemTime::now();
        let mut next: Vec<SystemTime> = SCHEDULE.iter().map(|(_, every)| start + *every).collect();
        let mut last_tick = SystemTime::now();
        loop {
            let now = SystemTime::now();
            // A long gap means the machine slept; time going backwards means
            // the clock was set back and every deadline is now too far
            // away. Either way, run everything now.
            let slept = now
                .duration_since(last_tick)
                .is_ok_and(|gap| gap > TICK + SLEEP_GAP);
            if slept || now < last_tick {
                next.iter_mut().for_each(|t| *t = now);
            }
            last_tick = now;
            for (i, (kind, every)) in SCHEDULE.into_iter().enumerate() {
                if now >= next[i] {
                    self.run(kind, every);
                    next[i] = now + every;
                }
            }
            std::thread::sleep(TICK);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failure_keeps_the_last_good_value() {
        let mut probe = Probe::default();
        record(&mut probe, Duration::from_secs(5), Ok(1.5));
        let at = probe.at;
        record(
            &mut probe,
            Duration::from_secs(5),
            Err::<f64, _>("boom".into()),
        );
        assert_eq!(probe.value, Some(1.5));
        assert_eq!(probe.at, at);
        assert_eq!(probe.error.as_deref(), Some("boom"));
        assert_eq!(probe.every_s, 5);
    }

    #[test]
    fn a_full_run_fills_the_always_available_probes() {
        let dir = std::env::temp_dir();
        let shared = Shared::new();
        let mut prober = Prober::new(
            shared.clone(),
            Roots {
                current_system: dir.join("menagerie-no-such-system"),
                system_profile: dir.join("menagerie-no-such-profile"),
                home: None,
            },
        );
        prober.run_all();
        let status = shared.status();
        assert!(status.probes.load.value.is_some());
        assert!(status.probes.memory.value.is_some());
        assert!(status.probes.disk.value.is_some());
        assert!(status.probes.system.value.is_some());
        assert_eq!(status.probes.revision.value, None);
        assert!(status.probes.revision.error.is_none());
        assert_eq!(status.load_history.len(), 1);
    }
}
