//! Update bookkeeping that doesn't depend on the app framework: channels, and the start-up
//! health check that rolls back a version that can't stay running.
//!
//! How rollback works: before installing an update the app saves the installer for the version
//! it is currently running. Every launch writes "starting" to `boot.json`, and a launch that
//! stays up for a few seconds writes "healthy". If the same version starts twice in a row
//! without ever becoming healthy, the next launch runs the saved installer to go back, and that
//! version is added to the skip list so it isn't offered again.

use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const REPO: &str = "larosa023/Marquee";

/// Where each channel's update manifest lives. Beta also looks at stable and takes the newer.
pub fn endpoints(channel: &str) -> Vec<String> {
    let stable = format!("https://github.com/{REPO}/releases/latest/download/latest.json");
    match channel {
        "beta" => vec![format!("https://github.com/{REPO}/releases/download/beta-channel/latest.json"), stable],
        _ => vec![stable],
    }
}

/// The installer for a given version on GitHub Releases (kept for rolling back to it).
pub fn installer_url(version: &str) -> String {
    format!("https://github.com/{REPO}/releases/download/v{version}/Marquee_{version}_x64-setup.exe")
}

pub fn is_prerelease(version: &str) -> bool {
    version.contains('-')
}

/// Compares "1.2.3" and "1.2.3-beta.1" style versions. A release is newer than its own pre-releases.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    fn parse(v: &str) -> (Vec<u64>, Option<String>) {
        let v = v.trim().trim_start_matches('v');
        let (core, pre) = match v.split_once('-') {
            Some((c, p)) => (c, Some(p.to_string())),
            None => (v, None),
        };
        (core.split('.').map(|p| p.parse().unwrap_or(0)).collect(), pre)
    }
    let (a, a_pre) = parse(candidate);
    let (b, b_pre) = parse(current);
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    match (a_pre, b_pre) {
        (None, Some(_)) => true,
        (Some(x), Some(y)) => {
            // Compare dotted identifiers, numerically where both are numbers ("beta.10" > "beta.9").
            let (xs, ys): (Vec<&str>, Vec<&str>) = (x.split('.').collect(), y.split('.').collect());
            for i in 0..xs.len().max(ys.len()) {
                match (xs.get(i), ys.get(i)) {
                    (Some(p), Some(q)) if p != q => {
                        return match (p.parse::<u64>(), q.parse::<u64>()) {
                            (Ok(m), Ok(n)) => m > n,
                            _ => p > q,
                        }
                    }
                    (Some(_), None) => return true,
                    (None, Some(_)) => return false,
                    _ => {}
                }
            }
            false
        }
        _ => false,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct BootState {
    pub version: String,
    /// "starting" | "healthy"
    pub state: String,
    /// Launches of `version` that never became healthy.
    pub failures: u32,
    pub previous_version: Option<String>,
    pub previous_installer: Option<String>,
    /// Versions that were rolled back; never offered again.
    pub skip: Vec<String>,
}

#[derive(Debug, PartialEq)]
pub enum Boot {
    Normal,
    /// This version keeps failing: run this installer to go back.
    RollBack { to_version: String, installer: PathBuf },
}

fn file(data_dir: &Path) -> PathBuf {
    data_dir.join("boot.json")
}

pub fn load(data_dir: &Path) -> BootState {
    std::fs::read_to_string(file(data_dir)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save(data_dir: &Path, s: &BootState) -> Result<()> {
    std::fs::create_dir_all(data_dir)?;
    std::fs::write(file(data_dir), serde_json::to_string_pretty(s)?)?;
    Ok(())
}

/// Call first thing at launch.
pub fn on_start(data_dir: &Path, version: &str) -> Boot {
    let mut s = load(data_dir);
    if s.version == version && s.state == "starting" {
        s.failures += 1;
    } else if s.version != version {
        s.failures = 0;
    }
    s.version = version.to_string();
    s.state = "starting".into();
    let decision = match (&s.previous_version, &s.previous_installer) {
        (Some(prev), Some(installer)) if s.failures >= 2 && prev != version && Path::new(installer).is_file() => {
            let d = Boot::RollBack { to_version: prev.clone(), installer: PathBuf::from(installer) };
            if !s.skip.iter().any(|v| v == version) {
                s.skip.push(version.to_string());
            }
            // Don't loop: the next launch (of the old version) starts clean.
            s.failures = 0;
            s.previous_installer = None;
            d
        }
        _ => Boot::Normal,
    };
    let _ = save(data_dir, &s);
    decision
}

/// Call once the window has been up for a few seconds.
pub fn mark_healthy(data_dir: &Path, version: &str) {
    let mut s = load(data_dir);
    if s.version == version {
        s.state = "healthy".into();
        s.failures = 0;
        let _ = save(data_dir, &s);
    }
}

/// Call just before installing an update, with the installer for the version running now.
pub fn before_update(data_dir: &Path, current_version: &str, installer: Option<&Path>) {
    let mut s = load(data_dir);
    s.previous_version = Some(current_version.to_string());
    s.previous_installer = installer.map(|p| p.to_string_lossy().to_string());
    let _ = save(data_dir, &s);
}

pub fn skipped(data_dir: &Path) -> Vec<String> {
    load(data_dir).skip
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_ordering() {
        assert!(is_newer("0.1.0", "0.0.9"));
        assert!(is_newer("v0.10.0", "0.9.9"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.0.9", "0.1.0"));
        assert!(is_newer("0.2.0", "0.2.0-beta.3"), "a release beats its own betas");
        assert!(!is_newer("0.2.0-beta.3", "0.2.0"));
        assert!(is_newer("0.2.0-beta.10", "0.2.0-beta.9"));
        assert!(is_newer("0.2.0-beta.1", "0.1.5"));
        assert!(is_prerelease("0.2.0-beta.1") && !is_prerelease("0.2.0"));
    }

    #[test]
    fn channels_point_at_github_releases() {
        assert_eq!(endpoints("stable"), vec!["https://github.com/larosa023/Marquee/releases/latest/download/latest.json"]);
        assert_eq!(endpoints("beta").len(), 2);
        assert_eq!(installer_url("0.0.9"), "https://github.com/larosa023/Marquee/releases/download/v0.0.9/Marquee_0.0.9_x64-setup.exe");
    }

    #[test]
    fn a_healthy_version_never_rolls_back() {
        let dir = tempfile::tempdir().unwrap();
        for _ in 0..5 {
            assert_eq!(on_start(dir.path(), "0.1.0"), Boot::Normal);
            mark_healthy(dir.path(), "0.1.0");
        }
        assert_eq!(load(dir.path()).failures, 0);
    }

    #[test]
    fn a_version_that_never_gets_healthy_rolls_back_once_and_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let installer = dir.path().join("Marquee_0.0.9_x64-setup.exe");
        std::fs::write(&installer, b"installer").unwrap();
        // 0.0.9 runs fine, then updates to 0.1.0.
        on_start(dir.path(), "0.0.9");
        mark_healthy(dir.path(), "0.0.9");
        before_update(dir.path(), "0.0.9", Some(&installer));
        // 0.1.0 crashes on launch, twice.
        assert_eq!(on_start(dir.path(), "0.1.0"), Boot::Normal);
        assert_eq!(on_start(dir.path(), "0.1.0"), Boot::Normal, "one bad start could be a fluke");
        assert_eq!(on_start(dir.path(), "0.1.0"), Boot::RollBack { to_version: "0.0.9".into(), installer: installer.clone() });
        assert_eq!(skipped(dir.path()), vec!["0.1.0"]);
        // Back on 0.0.9: no loop.
        assert_eq!(on_start(dir.path(), "0.0.9"), Boot::Normal);
        mark_healthy(dir.path(), "0.0.9");
        assert_eq!(on_start(dir.path(), "0.0.9"), Boot::Normal);
    }

    #[test]
    fn without_a_saved_installer_there_is_nothing_to_roll_back_to() {
        let dir = tempfile::tempdir().unwrap();
        before_update(dir.path(), "0.0.9", None);
        for _ in 0..4 {
            assert_eq!(on_start(dir.path(), "0.1.0"), Boot::Normal);
        }
    }
}
