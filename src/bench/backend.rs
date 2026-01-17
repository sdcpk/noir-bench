//! Bench-specific backend helpers.
//!
//! This module provides specialized backend support for the bench runner.
//! The main proving workflow is now handled by `crate::engine::workflow` using
//! the unified `crate::backend::Backend` trait.
//!
//! This module only contains:
//! - `EvmBackend`: For EVM gas verification via Foundry (distinct from proving)

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::{BenchError, BenchResult};

/// Output from EVM verification.
#[derive(Debug, Clone)]
pub struct EvmVerifyOutput {
    /// Verification time in milliseconds (if available)
    pub verify_time_ms: Option<u128>,
    /// Whether verification succeeded
    pub success: bool,
    /// Gas used for verification
    pub gas_used: Option<u64>,
}

/// EVM backend for gas measurement via Foundry.
///
/// This backend is specialized for measuring EVM verification gas costs.
/// It runs `forge test --gas-report` and parses the output.
///
/// Note: This does not implement `crate::backend::Backend` because it operates
/// on Foundry projects, not Noir artifacts.
pub struct EvmBackend {
    /// Path to the Foundry project directory
    pub foundry_dir: PathBuf,
    /// Path to forge binary (defaults to "forge" from PATH)
    pub forge_bin: Option<PathBuf>,
    /// Test pattern to match
    pub test_pattern: Option<String>,
    /// Gas per second for latency estimation
    pub gas_per_second: Option<u64>,
}

impl EvmBackend {
    /// Create a new EVM backend.
    pub fn new(foundry_dir: impl Into<PathBuf>) -> Self {
        EvmBackend {
            foundry_dir: foundry_dir.into(),
            forge_bin: None,
            test_pattern: None,
            gas_per_second: Some(1_250_000),
        }
    }

    /// Set the forge binary path.
    pub fn with_forge_bin(mut self, forge_bin: impl Into<PathBuf>) -> Self {
        self.forge_bin = Some(forge_bin.into());
        self
    }

    /// Set the test pattern to match.
    pub fn with_test_pattern(mut self, pattern: impl Into<String>) -> Self {
        self.test_pattern = Some(pattern.into());
        self
    }

    /// Run EVM verification and measure gas usage.
    pub fn verify(&self) -> BenchResult<EvmVerifyOutput> {
        let forge = self
            .forge_bin
            .clone()
            .unwrap_or_else(|| PathBuf::from("forge"));
        let mut cmd = Command::new(&forge);
        cmd.arg("test").arg("--gas-report");
        if let Some(pat) = &self.test_pattern {
            cmd.arg("-m").arg(pat);
        }
        cmd.current_dir(&self.foundry_dir);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let output = cmd
            .output()
            .map_err(|e| BenchError::Message(format!("failed to run forge: {e}")))?;
        let stdout_s = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr_s = String::from_utf8_lossy(&output.stderr).to_string();

        if !output.status.success() {
            return Err(BenchError::Message(format!(
                "forge test failed. stderr=\n{}",
                stderr_s
            )));
        }

        // Prefer .gas-snapshot, fallback to stdout heuristic
        let snapshot_path = self.foundry_dir.join(".gas-snapshot");
        let gas_used = read_gas_from_snapshot(&snapshot_path, &self.test_pattern)
            .or_else(|| read_gas_from_stdout(&stdout_s))
            .ok_or_else(|| {
                BenchError::Message("failed to parse gas used from Foundry outputs".into())
            })?;

        Ok(EvmVerifyOutput {
            verify_time_ms: None,
            success: true,
            gas_used: Some(gas_used),
        })
    }
}

/// Parse gas usage from .gas-snapshot file.
fn read_gas_from_snapshot(snapshot_path: &Path, match_pattern: &Option<String>) -> Option<u64> {
    let contents = std::fs::read_to_string(snapshot_path).ok()?;
    let lines = contents.lines();
    let mut best: Option<u64> = None;

    for line in lines {
        if let Some(pat) = match_pattern {
            if !line.contains(pat) {
                continue;
            }
        }
        if let Some(start_idx) = line.find("(gas:") {
            let slice = &line[start_idx + 5..];
            if let Some(end_idx) = slice.find(')') {
                let num_s = slice[..end_idx].trim().trim_start_matches(':').trim();
                let num_s: String = num_s.chars().filter(|c| c.is_ascii_digit()).collect();
                if let Ok(v) = num_s.parse::<u64>() {
                    best = Some(v);
                    break;
                }
            }
        }
    }
    best
}

/// Parse gas usage from forge stdout output.
fn read_gas_from_stdout(stdout: &str) -> Option<u64> {
    if let Some(idx) = stdout.find("gas:") {
        let mut s = &stdout[idx + 4..];
        s = s.trim_start();
        let num: String = s
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '_')
            .collect();
        let num = num.replace('_', "");
        if let Ok(v) = num.parse::<u64>() {
            return Some(v);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_gas_from_stdout() {
        let stdout = "Test output\ngas: 123456\nMore output";
        assert_eq!(read_gas_from_stdout(stdout), Some(123456));
    }

    #[test]
    fn test_read_gas_from_stdout_with_underscores() {
        let stdout = "gas: 1_234_567";
        assert_eq!(read_gas_from_stdout(stdout), Some(1234567));
    }

    #[test]
    fn test_read_gas_from_stdout_no_match() {
        let stdout = "No gas info here";
        assert_eq!(read_gas_from_stdout(stdout), None);
    }
}
