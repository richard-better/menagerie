//! The individual measurements. Each returns its value or a short reason it
//! could not be read; scheduling and caching live in `snapshot`.

use menagerie_protocol::{Battery, Load, SystemInfo, Temperature, Usage};
use sysinfo::{Components, Disks, System};

pub fn load() -> Result<Load, String> {
    let avg = System::load_average().map_err(|e| e.to_string())?;
    let cpus = std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(1);
    Ok(Load {
        one: avg.one,
        five: avg.five,
        fifteen: avg.fifteen,
        cpus,
    })
}

pub fn memory(system: &mut System) -> Result<Usage, String> {
    system.refresh_memory();
    let total = system.total_memory();
    if total == 0 {
        return Err("total memory reads as zero".into());
    }
    Ok(Usage {
        used_bytes: system.used_memory(),
        total_bytes: total,
    })
}

/// The disk holding `/`. On macOS that is the read-only system volume's
/// container, so the data volume is preferred when it is mounted.
pub fn disk() -> Result<Usage, String> {
    let disks = Disks::new_with_refreshed_list().map_err(|e| e.to_string())?;
    let pick = |mount: &str| {
        disks
            .iter()
            .find(|d| d.mount_point().to_str() == Some(mount))
    };
    let disk = pick("/System/Volumes/Data")
        .or_else(|| pick("/"))
        .ok_or("no disk mounted at /")?;
    let total = disk.total_space();
    Ok(Usage {
        used_bytes: total.saturating_sub(disk.available_space()),
        total_bytes: total,
    })
}

/// `Ok(None)` when the machine has no battery.
pub fn battery() -> Result<Option<Battery>, String> {
    use starship_battery::State;
    let manager = starship_battery::Manager::new().map_err(|e| e.to_string())?;
    let mut batteries = manager.batteries().map_err(|e| e.to_string())?;
    let Some(battery) = batteries.next() else {
        return Ok(None);
    };
    let battery = battery.map_err(|e| e.to_string())?;
    Ok(Some(Battery {
        percent: f64::from(battery.state_of_charge().value) * 100.0,
        charging: matches!(battery.state(), State::Charging | State::Full),
        health_percent: Some(f64::from(battery.state_of_health().value) * 100.0),
        cycles: battery.cycle_count(),
    }))
}

/// The hottest readable sensor, or `Ok(None)` when none can be read.
pub fn temperature() -> Result<Option<Temperature>, String> {
    let components = Components::new_with_refreshed_list().map_err(|e| e.to_string())?;
    let hottest = components
        .iter()
        .filter_map(|c| c.temperature())
        .filter(|t| t.is_finite() && *t > 0.0 && *t < 150.0)
        .fold(None, |max: Option<f32>, t| {
            Some(max.map_or(t, |m| m.max(t)))
        });
    Ok(hottest.map(|t| Temperature {
        celsius: f64::from(t),
    }))
}

pub fn system_info() -> Result<SystemInfo, String> {
    Ok(SystemInfo {
        os: System::name().unwrap_or_else(|_| std::env::consts::OS.to_owned()),
        os_version: System::os_version().ok(),
        uptime_s: System::uptime().map_err(|e| e.to_string())?,
    })
}

/// The hostname without macOS's `.local` mDNS suffix.
pub fn host_name() -> String {
    let name = System::host_name().unwrap_or_else(|_| "unknown".to_owned());
    name.strip_suffix(".local")
        .map(str::to_owned)
        .unwrap_or(name)
}
