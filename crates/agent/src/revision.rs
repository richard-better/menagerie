//! Which commit of the configuration repo built what this machine runs.
//!
//! nix-darwin and NixOS record it as `system.configurationRevision`. Home
//! Manager has no equivalent, so a Home Manager configuration has to write
//! the revision to `configuration-revision` inside its generation itself
//! (for example with `home.extraBuilderCommands`). The system's answer wins
//! when both exist, since it is what a sync rebuilds.

use std::path::{Path, PathBuf};
use std::process::Command;

use menagerie_protocol::{Revision, RevisionSource};

/// Where to look. Real paths in production, a temporary tree in tests.
#[derive(Debug, Clone)]
pub struct Roots {
    pub current_system: PathBuf,
    pub system_profile: PathBuf,
    pub home: Option<PathBuf>,
}

impl Roots {
    pub fn system() -> Self {
        Self {
            current_system: PathBuf::from("/run/current-system"),
            system_profile: PathBuf::from("/nix/var/nix/profiles/system"),
            home: std::env::var_os("HOME").map(PathBuf::from),
        }
    }
}

pub fn read(roots: &Roots) -> Result<Option<Revision>, String> {
    if let Some(rev) = nix_darwin(roots)? {
        return Ok(Some(rev));
    }
    if let Some(rev) = nixos(roots)? {
        return Ok(Some(rev));
    }
    home_manager(roots)
}

fn nix_darwin(roots: &Roots) -> Result<Option<Revision>, String> {
    let path = roots.current_system.join("darwin-version.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(None);
    };
    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(json
        .get("configurationRevision")
        .and_then(|v| v.as_str())
        .and_then(|stamp| {
            Revision::parse(
                stamp,
                generation(&roots.system_profile, &roots.current_system),
                RevisionSource::NixDarwin,
            )
        }))
}

fn nixos(roots: &Roots) -> Result<Option<Revision>, String> {
    let tool = roots.current_system.join("sw/bin/nixos-version");
    if !tool.exists() {
        return Ok(None);
    }
    let output = Command::new(&tool)
        .arg("--json")
        .output()
        .map_err(|e| format!("{}: {e}", tool.display()))?;
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|e| format!("nixos-version --json: {e}"))?;
    Ok(json
        .get("configurationRevision")
        .and_then(|v| v.as_str())
        .and_then(|stamp| {
            Revision::parse(
                stamp,
                generation(&roots.system_profile, &roots.current_system),
                RevisionSource::Nixos,
            )
        }))
}

fn home_manager(roots: &Roots) -> Result<Option<Revision>, String> {
    let Some(home) = &roots.home else {
        return Ok(None);
    };
    let current = home.join(".local/state/home-manager/gcroots/current-home");
    let Ok(stamp) = std::fs::read_to_string(current.join("configuration-revision")) else {
        return Ok(None);
    };
    let profile = home.join(".local/state/nix/profiles/home-manager");
    Ok(Revision::parse(
        &stamp,
        generation(&profile, &current),
        RevisionSource::HomeManager,
    ))
}

/// The number of the generation that is running: the `<profile>-N-link`
/// beside `profile` that resolves to the same store path as `current`. The
/// profile's own link is not enough, since it can point at a newer
/// generation that was built but not activated.
fn generation(profile: &Path, current: &Path) -> Option<u64> {
    let running = std::fs::canonicalize(current).ok()?;
    let prefix = format!("{}-", profile.file_name()?.to_str()?);
    std::fs::read_dir(profile.parent()?)
        .ok()?
        .flatten()
        .find_map(|entry| {
            let name = entry.file_name();
            let number = name
                .to_str()?
                .strip_prefix(&prefix)?
                .strip_suffix("-link")?
                .parse()
                .ok()?;
            (std::fs::canonicalize(entry.path()).ok()? == running).then_some(number)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots(dir: &Path) -> Roots {
        Roots {
            current_system: dir.join("current-system"),
            system_profile: dir.join("profiles/system"),
            home: Some(dir.join("home")),
        }
    }

    /// Profile generations `numbers` beside `profile`, each a link to its own
    /// directory, with the running one (`current`) pointing at `running`.
    fn generations(profile: &Path, current: &Path, numbers: &[u64], running: u64) -> PathBuf {
        let dir = profile.parent().unwrap();
        let name = profile.file_name().unwrap().to_str().unwrap();
        std::fs::create_dir_all(dir).unwrap();
        let store = dir.join("store");
        for n in numbers {
            std::fs::create_dir_all(store.join(n.to_string())).unwrap();
            std::os::unix::fs::symlink(
                store.join(n.to_string()),
                dir.join(format!("{name}-{n}-link")),
            )
            .unwrap();
        }
        let newest = numbers.iter().max().unwrap();
        std::os::unix::fs::symlink(format!("{name}-{newest}-link"), profile).unwrap();
        std::fs::create_dir_all(current.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(store.join(running.to_string()), current).unwrap();
        store.join(running.to_string())
    }

    #[test]
    fn reads_nix_darwin_with_the_running_generation() {
        let dir = tempfile::tempdir().unwrap();
        let r = roots(dir.path());
        // 413 was built but never activated; 412 is running.
        let running = generations(&r.system_profile, &r.current_system, &[411, 412, 413], 412);
        std::fs::write(
            running.join("darwin-version.json"),
            r#"{"configurationRevision":"9b06a19abcdef-dirty","darwinLabel":"26.05"}"#,
        )
        .unwrap();

        let rev = read(&r).unwrap().unwrap();
        assert_eq!(rev.rev, "9b06a19abcdef");
        assert!(rev.dirty);
        assert_eq!(rev.generation, Some(412));
        assert_eq!(rev.source, RevisionSource::NixDarwin);
    }

    #[test]
    fn falls_back_to_home_manager() {
        let dir = tempfile::tempdir().unwrap();
        let r = roots(dir.path());
        let home = r.home.clone().unwrap();
        let running = generations(
            &home.join(".local/state/nix/profiles/home-manager"),
            &home.join(".local/state/home-manager/gcroots/current-home"),
            &[36, 37],
            37,
        );
        std::fs::write(running.join("configuration-revision"), "f460dc1f0aaf").unwrap();

        let rev = read(&r).unwrap().unwrap();
        assert_eq!(rev.rev, "f460dc1f0aaf");
        assert_eq!(rev.generation, Some(37));
        assert_eq!(rev.source, RevisionSource::HomeManager);
    }

    #[test]
    fn nothing_stamped_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read(&roots(dir.path())).unwrap(), None);
    }
}
