//! Environment detection utilities for benchmark records.

use std::process::Command;

use serde::{Deserialize, Serialize};

/// Environment information for benchmark reproducibility
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_model: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_cores: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_ram_bytes: Option<u64>,

    pub os: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_sha: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_dirty: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub nargo_version: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bb_version: Option<String>,
}

impl Default for EnvironmentInfo {
    fn default() -> Self {
        EnvironmentInfo {
            cpu_model: None,
            cpu_cores: None,
            total_ram_bytes: None,
            os: std::env::consts::OS.to_string(),
            hostname: None,
            git_sha: None,
            git_dirty: None,
            nargo_version: None,
            bb_version: None,
        }
    }
}

impl EnvironmentInfo {
    /// Detect environment information from the current system
    pub fn detect() -> Self {
        use sysinfo::System;

        let mut sys = System::new_all();
        sys.refresh_all();

        let cpu_model = sys.cpus().first().map(|c| c.brand().to_string());
        let cpu_cores = sys.physical_core_count().map(|c| c as u32);
        let total_ram_bytes = Some(sys.total_memory());
        let os = System::name().unwrap_or_else(|| std::env::consts::OS.to_string());
        let hostname = System::host_name();

        let git_sha = detect_git_sha();
        let git_dirty = detect_git_dirty();
        let nargo_version = detect_nargo_version();
        let bb_version = detect_bb_version();

        EnvironmentInfo {
            cpu_model,
            cpu_cores,
            total_ram_bytes,
            os,
            hostname,
            git_sha,
            git_dirty,
            nargo_version,
            bb_version,
        }
    }

    /// Detect with custom bb path
    pub fn detect_with_bb_path(bb_path: Option<&std::path::Path>) -> Self {
        let mut env = Self::detect();
        if let Some(path) = bb_path {
            env.bb_version = detect_bb_version_from_path(path);
        }
        env
    }
}

/// Detect git SHA from `git rev-parse HEAD`
fn detect_git_sha() -> Option<String> {
    Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Detect if git working directory is dirty
fn detect_git_dirty() -> Option<bool> {
    Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| !o.stdout.is_empty())
}

/// Detect nargo version from `nargo --version`
fn detect_nargo_version() -> Option<String> {
    Command::new("nargo")
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Detect bb version from `bb --version`
fn detect_bb_version() -> Option<String> {
    detect_bb_version_from_path(std::path::Path::new("bb"))
}

/// Detect bb version from a specific path
fn detect_bb_version_from_path(path: &std::path::Path) -> Option<String> {
    Command::new(path)
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_environment_detect_has_os() {
        let env = EnvironmentInfo::detect();
        assert!(!env.os.is_empty());
    }

    #[test]
    fn test_environment_default() {
        let env = EnvironmentInfo::default();
        assert!(!env.os.is_empty());
        assert!(env.cpu_model.is_none());
    }
}
