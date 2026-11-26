//! `/metrics` in the Prometheus text format. System metrics use
//! node_exporter's names, so existing Grafana dashboards work unchanged;
//! Menagerie's own are prefixed `menagerie_`.

use std::fmt::Write;

use menagerie_protocol::{Probe, Status};

pub fn render(status: &Status) -> String {
    let mut out = String::new();
    let p = &status.probes;
    let mut gauge = |name: &str, help: &str, samples: &[(String, f64)]| {
        if samples.is_empty() {
            return;
        }
        let _ = writeln!(out, "# HELP {name} {help}");
        let _ = writeln!(out, "# TYPE {name} gauge");
        for (labels, value) in samples {
            let _ = writeln!(out, "{name}{labels} {value}");
        }
    };
    let plain = |v: f64| vec![(String::new(), v)];

    if let Some(load) = &p.load.value {
        gauge("node_load1", "1m load average.", &plain(load.one));
        gauge("node_load5", "5m load average.", &plain(load.five));
        gauge("node_load15", "15m load average.", &plain(load.fifteen));
    }
    if let Some(memory) = &p.memory.value {
        // node_exporter names total memory differently per platform.
        let total = if cfg!(target_os = "linux") {
            "node_memory_MemTotal_bytes"
        } else {
            "node_memory_total_bytes"
        };
        gauge(
            total,
            "Total memory in bytes.",
            &plain(memory.total_bytes as f64),
        );
        gauge(
            "menagerie_memory_used_bytes",
            "Memory in use.",
            &plain(memory.used_bytes as f64),
        );
    }
    if let Some(disk) = &p.disk.value {
        let root = r#"{mountpoint="/"}"#.to_owned();
        gauge(
            "node_filesystem_size_bytes",
            "Filesystem size in bytes.",
            &[(root.clone(), disk.total_bytes as f64)],
        );
        gauge(
            "node_filesystem_avail_bytes",
            "Filesystem space available in bytes.",
            &[(root, (disk.total_bytes - disk.used_bytes) as f64)],
        );
    }
    if let Some(battery) = &p.battery.value {
        let label = r#"{power_supply="battery"}"#.to_owned();
        gauge(
            "node_power_supply_current_capacity",
            "Battery charge in percent.",
            &[(label.clone(), battery.percent)],
        );
        gauge(
            "node_power_supply_charging",
            "Whether the battery is charging.",
            &[(label, f64::from(u8::from(battery.charging)))],
        );
    }
    if let Some(t) = &p.temperature.value {
        gauge(
            "menagerie_temperature_celsius",
            "Hottest readable sensor.",
            &plain(t.celsius),
        );
    }
    if let (Some(system), Some(at)) = (&p.system.value, p.system.at) {
        // From when uptime was measured, so the value holds still between
        // refreshes.
        let boot = (at / 1000) as f64 - system.uptime_s as f64;
        gauge(
            "node_boot_time_seconds",
            "Node boot time, in unixtime.",
            &plain(boot),
        );
    }
    if let Some(rev) = &p.revision.value {
        let labels = format!(
            r#"{{rev="{}",dirty="{}",source="{}"}}"#,
            rev.rev,
            rev.dirty,
            serde_json::to_value(rev.source)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default()
        );
        gauge(
            "menagerie_config_info",
            "The configuration revision this machine runs.",
            &[(labels, 1.0)],
        );
        if let Some(generation) = rev.generation {
            gauge(
                "menagerie_config_generation",
                "Generation number of the running configuration.",
                &plain(generation as f64),
            );
        }
    }

    let ages: Vec<(String, f64)> = [
        ("load", age(&p.load, status)),
        ("memory", age(&p.memory, status)),
        ("disk", age(&p.disk, status)),
        ("battery", age(&p.battery, status)),
        ("temperature", age(&p.temperature, status)),
        ("revision", age(&p.revision, status)),
        ("system", age(&p.system, status)),
    ]
    .into_iter()
    .filter_map(|(name, age)| age.map(|a| (format!(r#"{{probe="{name}"}}"#), a)))
    .collect();
    gauge(
        "menagerie_probe_age_seconds",
        "Seconds since each probe last succeeded.",
        &ages,
    );
    out
}

fn age<T>(probe: &Probe<T>, status: &Status) -> Option<f64> {
    probe
        .at
        .map(|at| (status.agent.now - at).max(0) as f64 / 1000.0)
}

#[cfg(test)]
mod tests {
    use menagerie_protocol::{AgentInfo, Load, Probes, Revision, RevisionSource};

    use super::*;

    #[test]
    fn renders_node_exporter_names_and_the_revision() {
        let probes = Probes {
            load: Probe {
                value: Some(Load {
                    one: 0.5,
                    five: 0.25,
                    fifteen: 0.125,
                    cpus: 4,
                }),
                at: Some(9_000),
                every_s: 5,
                error: None,
            },
            revision: Probe {
                value: Revision::parse("9b06a19abc", Some(12), RevisionSource::NixDarwin),
                ..Probe::default()
            },
            ..Probes::default()
        };
        let status = Status {
            version: 1,
            host: "h".into(),
            agent: AgentInfo {
                version: "0".into(),
                now: 10_000,
            },
            probes,
            load_history: vec![],
        };
        let text = render(&status);
        assert!(text.contains("node_load1 0.5\n"), "{text}");
        assert!(text.contains(
            r#"menagerie_config_info{rev="9b06a19abc",dirty="false",source="nix-darwin"} 1"#
        ));
        assert!(text.contains("menagerie_config_generation 12\n"));
        assert!(text.contains(r#"menagerie_probe_age_seconds{probe="load"} 1"#));
        assert!(!text.contains("node_power_supply"));
    }
}
