//! Which commit of the configuration repo built what this machine runs.
//!
//! nix-darwin and NixOS record it as `system.configurationRevision`; Home
//! Manager generations built with Menagerie's Nix module (or any config that
//! writes it) carry a `configuration-revision` file. The system's answer
//! wins when both exist, since it is what a sync rebuilds.

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
                generation(&roots.system_profile),
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
                generation(&roots.system_profile),
                RevisionSource::Nixos,
            )
        }))
}

fn home_manager(roots: &Roots) -> Result<Option<Revision>, String> {
    let Some(home) = &roots.home else {
        return Ok(None);
    };
    let path = home.join(".local/state/home-manager/gcroots/current-home/configuration-revision");
    let Ok(stamp) = std::fs::read_to_string(&path) else {
        return Ok(None);
    };
    let profile = home.join(".local/state/nix/profiles/home-manager");
    Ok(Revision::parse(
        &stamp,
        generation(&profile),
        RevisionSource::HomeManager,
    ))
}

/// The number in a profile link's target: `system-412-link` is 412.
fn generation(profile: &Path) -> Option<u64> {
    let target = std::fs::read_link(profile).ok()?;
    let name = target.file_name()?.to_str()?;
    name.strip_suffix("-link")?.rsplit('-').next()?.parse().ok()
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

    #[test]
    fn reads_nix_darwin_with_its_generation() {
        let dir = tempfile::tempdir().unwrap();
        let r = roots(dir.path());
        std::fs::create_dir_all(&r.current_system).unwrap();
        std::fs::write(
            r.current_system.join("darwin-version.json"),
            r#"{"configurationRevision":"9b06a19abcdef-dirty","darwinLabel":"26.05"}"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("profiles")).unwrap();
        std::os::unix::fs::symlink("system-412-link", &r.system_profile).unwrap();

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
        let generation = home.join(".local/state/home-manager/gcroots/current-home");
        std::fs::create_dir_all(&generation).unwrap();
        std::fs::write(generation.join("configuration-revision"), "f460dc1f0aaf").unwrap();
        let profiles = home.join(".local/state/nix/profiles");
        std::fs::create_dir_all(&profiles).unwrap();
        std::os::unix::fs::symlink("home-manager-37-link", profiles.join("home-manager")).unwrap();

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
