//! Provenance collection for benchmark reproducibility.
//!
//! This module provides utilities to collect metadata about the environment,
//! tools, and configuration used for a benchmark run. This enables:
//! - Reproducibility: Re-run with same tool versions
//! - Debugging: Identify environment differences causing regressions
//! - Auditing: Track what tools were used for a given benchmark

use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

/// Comprehensive provenance information for a benchmark run.
///
/// This is a sidecar structure that can be attached to reports without
/// modifying the BenchRecord v1 schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provenance {
    /// noir-bench tool version/git info
    pub noir_bench: ToolInfo,
    /// nargo toolchain info
    pub nargo: Option<ToolInfo>,
    /// Backend (bb) info
    pub backend: Option<ToolInfo>,
    /// System information
    pub system: SystemInfo,
    /// Command line arguments used
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cli_args: Vec<String>,
    /// ISO 8601 timestamp when provenance was collected
    pub collected_at: String,
}

/// Information about a tool/binary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    /// Tool name
    pub name: String,
    /// Version string (from --version or similar)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Git commit SHA (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_sha: Option<String>,
    /// Whether the git working directory was dirty
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_dirty: Option<bool>,
    /// Path to the tool binary
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// System/environment information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    /// Operating system name
    pub os: String,
    /// Architecture (e.g., x86_64, aarch64)
    pub arch: String,
    /// CPU model/brand string
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_brand: Option<String>,
    /// Number of CPU cores
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_cores: Option<u32>,
    /// Total RAM in bytes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ram_bytes: Option<u64>,
    /// Hostname
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
}

impl Default for SystemInfo {
    fn default() -> Self {
        SystemInfo {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            cpu_brand: None,
            cpu_cores: None,
            ram_bytes: None,
            hostname: None,
        }
    }
}

/// Collect comprehensive provenance information.
///
/// This function gathers information from:
/// - noir-bench itself (git SHA if in a repo)
/// - nargo (version from --version)
/// - Backend (bb --version)
/// - System (OS, arch, CPU, RAM)
///
/// # Arguments
/// * `bb_path` - Optional path to bb binary (defaults to "bb" in PATH)
///
/// # Example
/// ```ignore
/// let provenance = provenance::collect(Some(Path::new("/path/to/bb")));
/// ```
pub fn collect(bb_path: Option<&Path>) -> Provenance {
    let collected_at = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default();

    Provenance {
        noir_bench: collect_noir_bench_info(),
        nargo: collect_nargo_info(),
        backend: collect_backend_info(bb_path),
        system: collect_system_info(),
        cli_args: std::env::args().collect(),
        collected_at,
    }
}

/// Collect provenance with minimal shell-outs (for testing).
pub fn collect_minimal() -> Provenance {
    let collected_at = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default();

    Provenance {
        noir_bench: ToolInfo {
            name: "noir-bench".to_string(),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
            git_sha: None,
            git_dirty: None,
            path: None,
        },
        nargo: None,
        backend: None,
        system: SystemInfo::default(),
        cli_args: Vec::new(),
        collected_at,
    }
}

/// Collect noir-bench tool information.
fn collect_noir_bench_info() -> ToolInfo {
    let version = Some(env!("CARGO_PKG_VERSION").to_string());
    let git_sha = detect_git_sha();
    let git_dirty = detect_git_dirty();

    ToolInfo {
        name: "noir-bench".to_string(),
        version,
        git_sha,
        git_dirty,
        path: std::env::current_exe()
            .ok()
            .and_then(|p| p.to_str().map(String::from)),
    }
}

/// Collect nargo toolchain information.
fn collect_nargo_info() -> Option<ToolInfo> {
    let version = run_command("nargo", &["--version"]);
    let path = which_binary("nargo");

    if version.is_none() && path.is_none() {
        return None;
    }

    Some(ToolInfo {
        name: "nargo".to_string(),
        version,
        git_sha: None,
        git_dirty: None,
        path,
    })
}

/// Collect backend (bb) information.
fn collect_backend_info(bb_path: Option<&Path>) -> Option<ToolInfo> {
    let bb = bb_path
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "bb".to_string());

    let version = run_command(&bb, &["--version"]);
    let path = if bb_path.is_some() {
        bb_path.and_then(|p| p.to_str().map(String::from))
    } else {
        which_binary("bb")
    };

    if version.is_none() && path.is_none() {
        return None;
    }

    Some(ToolInfo {
        name: "barretenberg".to_string(),
        version,
        git_sha: None,
        git_dirty: None,
        path,
    })
}

/// Collect system information.
fn collect_system_info() -> SystemInfo {
    use sysinfo::System;

    let mut sys = System::new_all();
    sys.refresh_all();

    SystemInfo {
        os: System::name().unwrap_or_else(|| std::env::consts::OS.to_string()),
        arch: std::env::consts::ARCH.to_string(),
        cpu_brand: sys.cpus().first().map(|c| c.brand().to_string()),
        cpu_cores: sys.physical_core_count().map(|c| c as u32),
        ram_bytes: Some(sys.total_memory()),
        hostname: System::host_name(),
    }
}

/// Run a command and capture stdout.
fn run_command(cmd: &str, args: &[&str]) -> Option<String> {
    Command::new(cmd)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Find binary path using `which`.
fn which_binary(name: &str) -> Option<String> {
    Command::new("which")
        .arg(name)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Detect git SHA from `git rev-parse HEAD`.
fn detect_git_sha() -> Option<String> {
    run_command("git", &["rev-parse", "HEAD"])
}

/// Detect if git working directory is dirty.
fn detect_git_dirty() -> Option<bool> {
    Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| !o.stdout.is_empty())
}

/// Check if two provenance records have matching tool versions.
///
/// Returns a list of mismatches for reporting.
pub fn check_version_mismatches(
    baseline: &Provenance,
    target: &Provenance,
) -> Vec<VersionMismatch> {
    let mut mismatches = Vec::new();

    // Check nargo versions
    if let (Some(b_nargo), Some(t_nargo)) = (&baseline.nargo, &target.nargo) {
        if b_nargo.version != t_nargo.version {
            mismatches.push(VersionMismatch {
                tool: "nargo".to_string(),
                baseline_version: b_nargo.version.clone(),
                target_version: t_nargo.version.clone(),
            });
        }
    }

    // Check backend versions
    if let (Some(b_bb), Some(t_bb)) = (&baseline.backend, &target.backend) {
        if b_bb.version != t_bb.version {
            mismatches.push(VersionMismatch {
                tool: "barretenberg".to_string(),
                baseline_version: b_bb.version.clone(),
                target_version: t_bb.version.clone(),
            });
        }
    }

    // Check OS/arch
    if baseline.system.os != target.system.os || baseline.system.arch != target.system.arch {
        mismatches.push(VersionMismatch {
            tool: "system".to_string(),
            baseline_version: Some(format!("{}/{}", baseline.system.os, baseline.system.arch)),
            target_version: Some(format!("{}/{}", target.system.os, target.system.arch)),
        });
    }

    mismatches
}

/// A version mismatch between baseline and target.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionMismatch {
    pub tool: String,
    pub baseline_version: Option<String>,
    pub target_version: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collect_minimal() {
        let prov = collect_minimal();
        assert_eq!(prov.noir_bench.name, "noir-bench");
        assert!(prov.noir_bench.version.is_some());
    }

    #[test]
    fn test_system_info_default() {
        let sys = SystemInfo::default();
        assert!(!sys.os.is_empty());
        assert!(!sys.arch.is_empty());
    }

    #[test]
    fn test_provenance_serialization() {
        let prov = collect_minimal();
        let json = serde_json::to_string(&prov);
        assert!(json.is_ok());

        let json_str = json.unwrap();
        let deserialized: Result<Provenance, _> = serde_json::from_str(&json_str);
        assert!(deserialized.is_ok());
    }

    #[test]
    fn test_version_mismatch_detection() {
        let baseline = Provenance {
            noir_bench: ToolInfo {
                name: "noir-bench".to_string(),
                version: Some("0.1.0".to_string()),
                git_sha: None,
                git_dirty: None,
                path: None,
            },
            nargo: Some(ToolInfo {
                name: "nargo".to_string(),
                version: Some("0.38.0".to_string()),
                git_sha: None,
                git_dirty: None,
                path: None,
            }),
            backend: Some(ToolInfo {
                name: "barretenberg".to_string(),
                version: Some("0.63.0".to_string()),
                git_sha: None,
                git_dirty: None,
                path: None,
            }),
            system: SystemInfo {
                os: "linux".to_string(),
                arch: "x86_64".to_string(),
                cpu_brand: None,
                cpu_cores: None,
                ram_bytes: None,
                hostname: None,
            },
            cli_args: vec![],
            collected_at: "2026-01-15T00:00:00Z".to_string(),
        };

        let target = Provenance {
            noir_bench: baseline.noir_bench.clone(),
            nargo: Some(ToolInfo {
                name: "nargo".to_string(),
                version: Some("0.39.0".to_string()), // Different version
                git_sha: None,
                git_dirty: None,
                path: None,
            }),
            backend: baseline.backend.clone(),
            system: baseline.system.clone(),
            cli_args: vec![],
            collected_at: "2026-01-15T00:00:00Z".to_string(),
        };

        let mismatches = check_version_mismatches(&baseline, &target);
        assert_eq!(mismatches.len(), 1);
        assert_eq!(mismatches[0].tool, "nargo");
    }

    #[test]
    fn test_no_mismatches_when_same() {
        let prov = collect_minimal();
        let mismatches = check_version_mismatches(&prov, &prov);
        assert!(mismatches.is_empty());
    }
}
