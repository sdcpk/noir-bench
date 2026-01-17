//! Barretenberg backend implementation.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::{BenchError, BenchResult};

use super::traits::{Backend, Capabilities, GateInfo, ProveOutput, VerifyOutput};

/// Configuration for the Barretenberg backend.
#[derive(Debug, Clone)]
pub struct BarretenbergConfig {
    /// Path to the bb binary
    pub bb_path: PathBuf,
    /// Extra arguments to pass to bb commands
    pub extra_args: Vec<String>,
    /// Default timeout for operations
    pub default_timeout: Duration,
}

impl Default for BarretenbergConfig {
    fn default() -> Self {
        BarretenbergConfig {
            bb_path: PathBuf::from("bb"),
            extra_args: Vec::new(),
            default_timeout: Duration::from_secs(24 * 60 * 60), // 24 hours
        }
    }
}

impl BarretenbergConfig {
    /// Create a new config with the given bb path.
    pub fn new(bb_path: impl Into<PathBuf>) -> Self {
        BarretenbergConfig {
            bb_path: bb_path.into(),
            ..Default::default()
        }
    }

    /// Add extra arguments.
    pub fn with_args(mut self, args: Vec<String>) -> Self {
        self.extra_args = args;
        self
    }

    /// Set the default timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.default_timeout = timeout;
        self
    }
}

/// Barretenberg proving backend.
pub struct BarretenbergBackend {
    config: BarretenbergConfig,
    version_cache: Option<String>,
}

impl BarretenbergBackend {
    /// Create a new Barretenberg backend with the given configuration.
    pub fn new(config: BarretenbergConfig) -> Self {
        BarretenbergBackend {
            config,
            version_cache: None,
        }
    }

    /// Create a backend with just the bb path.
    pub fn from_path(bb_path: impl Into<PathBuf>) -> Self {
        Self::new(BarretenbergConfig::new(bb_path))
    }

    /// Run a bb command with timeout and optional memory tracking.
    fn run_with_timeout(
        &self,
        mut cmd: Command,
        timeout: Duration,
    ) -> BenchResult<(std::process::ExitStatus, Option<u64>, u128)> {
        #[cfg(feature = "mem")]
        use sysinfo::{ProcessRefreshKind, RefreshKind, System};

        let start = Instant::now();
        let mut child = cmd
            .spawn()
            .map_err(|e| BenchError::Message(format!("failed to spawn bb: {e}")))?;

        #[cfg(feature = "mem")]
        let mut sys = System::new_with_specifics(
            RefreshKind::new().with_processes(ProcessRefreshKind::everything()),
        );
        #[cfg(feature = "mem")]
        let mut peak_rss: u64 = 0;

        loop {
            if let Some(status) = child
                .try_wait()
                .map_err(|e| BenchError::Message(e.to_string()))?
            {
                let elapsed_ms = start.elapsed().as_millis();
                #[cfg(feature = "mem")]
                {
                    if let Some(pid) = child.id().try_into().ok().map(sysinfo::Pid::from_u32) {
                        sys.refresh_process(pid);
                        if let Some(p) = sys.process(pid) {
                            peak_rss = peak_rss.max(p.memory() * 1024);
                        }
                    }
                }
                return Ok((
                    status,
                    {
                        #[cfg(feature = "mem")]
                        {
                            Some(peak_rss)
                        }
                        #[cfg(not(feature = "mem"))]
                        {
                            None
                        }
                    },
                    elapsed_ms,
                ));
            }

            if timeout.as_secs() > 0 && start.elapsed() >= timeout {
                let _ = child.kill();
                let _ = child.wait();
                return Err(BenchError::Message("operation timed out".into()));
            }

            #[cfg(feature = "mem")]
            {
                if let Some(pid) = child.id().try_into().ok().map(sysinfo::Pid::from_u32) {
                    sys.refresh_process(pid);
                    if let Some(p) = sys.process(pid) {
                        peak_rss = peak_rss.max(p.memory() * 1024);
                    }
                }
            }

            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Detect bb version.
    fn detect_version(&self) -> Option<String> {
        Command::new(&self.config.bb_path)
            .arg("--version")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }
}

impl Backend for BarretenbergBackend {
    fn name(&self) -> &str {
        "barretenberg"
    }

    fn version(&self) -> Option<String> {
        // Use cached version or detect
        if let Some(ref v) = self.version_cache {
            return Some(v.clone());
        }
        self.detect_version()
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::barretenberg()
    }

    fn prove(
        &self,
        artifact: &Path,
        witness: Option<&Path>,
        timeout: Duration,
    ) -> BenchResult<ProveOutput> {
        // If no witness provided, we'd need to generate one - for now require witness
        let witness_path = witness.ok_or_else(|| {
            BenchError::Message("BarretenbergBackend::prove requires a witness file".into())
        })?;

        let out_dir = tempfile::tempdir()
            .map_err(|e| BenchError::Message(format!("failed to create temp dir: {e}")))?;

        let mut cmd = Command::new(&self.config.bb_path);
        cmd.arg("prove")
            .arg("-b")
            .arg(artifact)
            .arg("-w")
            .arg(witness_path)
            .arg("-o")
            .arg(out_dir.path());

        for arg in &self.config.extra_args {
            cmd.arg(arg);
        }

        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let (status, peak_memory_bytes, prove_time_ms) = self.run_with_timeout(cmd, timeout)?;

        if !status.success() {
            return Err(BenchError::Message(format!(
                "bb prove failed: status={status}"
            )));
        }

        // Read output file sizes
        let proof_path = out_dir.path().join("proof");
        let vk_path = out_dir.path().join("vk");
        let pk_path = out_dir.path().join("pk");

        let proof_size_bytes = std::fs::metadata(&proof_path).ok().map(|m| m.len());
        let verification_key_size_bytes = std::fs::metadata(&vk_path).ok().map(|m| m.len());
        let proving_key_size_bytes = std::fs::metadata(&pk_path).ok().map(|m| m.len());

        Ok(ProveOutput {
            prove_time_ms,
            witness_gen_time_ms: None, // Witness was pre-generated
            backend_prove_time_ms: Some(prove_time_ms),
            peak_memory_bytes,
            proof_size_bytes,
            proving_key_size_bytes,
            verification_key_size_bytes,
            proof_path: if proof_path.exists() {
                Some(proof_path)
            } else {
                None
            },
            vk_path: if vk_path.exists() {
                Some(vk_path)
            } else {
                None
            },
        })
    }

    fn verify(&self, proof: &Path, vk: &Path) -> BenchResult<VerifyOutput> {
        let mut cmd = Command::new(&self.config.bb_path);
        cmd.arg("verify").arg("-p").arg(proof).arg("-k").arg(vk);

        for arg in &self.config.extra_args {
            cmd.arg(arg);
        }

        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let start = Instant::now();
        let output = cmd
            .output()
            .map_err(|e| BenchError::Message(format!("failed to run bb verify: {e}")))?;
        let verify_time_ms = start.elapsed().as_millis();

        Ok(VerifyOutput {
            verify_time_ms,
            success: output.status.success(),
        })
    }

    fn gate_info(&self, artifact: &Path) -> BenchResult<GateInfo> {
        let mut cmd = Command::new(&self.config.bb_path);
        cmd.arg("gates").arg("-b").arg(artifact);

        for arg in &self.config.extra_args {
            cmd.arg(arg);
        }

        let output = cmd
            .output()
            .map_err(|e| BenchError::Message(format!("failed to run bb gates: {e}")))?;

        if !output.status.success() {
            return Err(BenchError::Message(format!(
                "bb gates failed: status={} stderr={}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        // Parse the JSON output
        #[derive(Deserialize)]
        struct GatesReport {
            acir_opcodes: usize,
            #[serde(alias = "circuit_size")]
            total_gates: usize,
            #[serde(default)]
            gates_per_opcode: Vec<usize>,
        }

        #[derive(Deserialize)]
        struct GatesResponse {
            functions: Vec<GatesReport>,
        }

        let response: GatesResponse = serde_json::from_slice(&output.stdout)
            .map_err(|e| BenchError::Message(format!("failed to parse bb gates output: {e}")))?;

        let func = response
            .functions
            .first()
            .ok_or_else(|| BenchError::Message("no functions in gates output".into()))?;

        let backend_gates = func.total_gates as u64;
        let subgroup_size = if backend_gates > 0 {
            Some(backend_gates.next_power_of_two())
        } else {
            None
        };

        let per_opcode = if !func.gates_per_opcode.is_empty() {
            let mut map = HashMap::new();
            for (i, gates) in func.gates_per_opcode.iter().enumerate() {
                map.insert(format!("opcode_{}", i), *gates as u64);
            }
            Some(map)
        } else {
            None
        };

        Ok(GateInfo {
            backend_gates,
            subgroup_size,
            acir_opcodes: Some(func.acir_opcodes as u64),
            per_opcode,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = BarretenbergConfig::default();
        assert_eq!(config.bb_path, PathBuf::from("bb"));
        assert!(config.extra_args.is_empty());
    }

    #[test]
    fn test_config_builder() {
        let config = BarretenbergConfig::new("/usr/local/bin/bb")
            .with_args(vec!["--scheme".into(), "ultra_honk".into()])
            .with_timeout(Duration::from_secs(60));

        assert_eq!(config.bb_path, PathBuf::from("/usr/local/bin/bb"));
        assert_eq!(config.extra_args, vec!["--scheme", "ultra_honk"]);
        assert_eq!(config.default_timeout, Duration::from_secs(60));
    }

    #[test]
    fn test_backend_name() {
        let backend = BarretenbergBackend::from_path("bb");
        assert_eq!(backend.name(), "barretenberg");
    }

    #[test]
    fn test_backend_capabilities() {
        let backend = BarretenbergBackend::from_path("bb");
        let caps = backend.capabilities();
        assert!(caps.can_prove);
        assert!(caps.can_verify);
        assert!(caps.has_gate_count);
    }
}
