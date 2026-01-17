use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use bn254_blackbox_solver::Bn254BlackBoxSolver;
use nargo::foreign_calls::DefaultForeignCallBuilder;
use noir_artifact_cli::execution::execute as execute_program_artifact;
use noir_artifact_cli::fs::artifact::read_program_from_file;
use noir_artifact_cli::fs::witness::save_witness_to_dir;

use crate::{
    BackendInfo, BenchError, BenchResult, CommonMeta, IterationStats, ProveReport,
    collect_system_info, compute_iteration_stats,
};
// New unified backend abstraction
use crate::backend::{Backend, BarretenbergBackend, BarretenbergConfig};
// New engine workflow
use crate::engine::{self, NargoToolchain, ProveInputs, Toolchain};
use shlex::Shlex;

/// Provider trait for proving operations.
///
/// **Deprecated**: This trait will be replaced by `crate::backend::Backend` in a future version.
/// New code should use `BarretenbergBackend` from the `crate::backend` module.
pub trait ProverProvider {
    fn prove(
        &self,
        artifact: &Path,
        inputs: Option<&Path>,
        timeout: Duration,
    ) -> BenchResult<ProveReport>;
    fn backend_info(&self) -> BackendInfo;
}

pub struct NotImplementedProver {
    pub backend_name: String,
}

impl ProverProvider for NotImplementedProver {
    fn prove(
        &self,
        _artifact: &Path,
        _inputs: Option<&Path>,
        _timeout: Duration,
    ) -> BenchResult<ProveReport> {
        Err(BenchError::Message(format!(
            "prove not implemented for backend '{}'",
            self.backend_name
        )))
    }
    fn backend_info(&self) -> BackendInfo {
        BackendInfo {
            name: self.backend_name.clone(),
            version: None,
        }
    }
}

pub struct BarretenbergProverProvider {
    pub backend_path: PathBuf,
    pub extra_args: Vec<String>,
}

impl BarretenbergProverProvider {
    fn run_bb_with_timeout(
        &self,
        mut cmd: Command,
        timeout: Duration,
    ) -> BenchResult<(std::process::ExitStatus, Option<u64>)> {
        #[cfg(feature = "mem")]
        use sysinfo::{ProcessRefreshKind, RefreshKind, System};

        let start = Instant::now();
        let mut child = cmd
            .spawn()
            .map_err(|e| BenchError::Message(e.to_string()))?;

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
                #[cfg(feature = "mem")]
                {
                    // final sample
                    if let Some(pid) = child.id().try_into().ok().map(sysinfo::Pid::from_u32) {
                        sys.refresh_process(pid);
                        if let Some(p) = sys.process(pid) {
                            peak_rss = peak_rss.max(p.memory() * 1024);
                        }
                    }
                }
                return Ok((status, {
                    #[cfg(feature = "mem")]
                    {
                        Some(peak_rss)
                    }
                    #[cfg(not(feature = "mem"))]
                    {
                        None
                    }
                }));
            }
            if timeout.as_secs() > 0 && start.elapsed() >= timeout {
                let _ = child.kill();
                let _ = child.wait();
                return Err(BenchError::Message("prove timed out".into()));
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
}

impl ProverProvider for BarretenbergProverProvider {
    fn prove(
        &self,
        artifact: &Path,
        inputs: Option<&Path>,
        timeout: Duration,
    ) -> BenchResult<ProveReport> {
        // Read artifact
        let program =
            read_program_from_file(artifact).map_err(|e| BenchError::Message(e.to_string()))?;

        // Generate witness from inputs using in-process execution
        let compiled: noirc_driver::CompiledProgram = program.clone().into();
        let prover_file = inputs.map(|p| p.with_extension("toml"));
        let prover_file = prover_file
            .as_ref()
            .map(|p| p.as_path())
            .unwrap_or_else(|| Path::new("Prover.toml"));
        let witness_start = Instant::now();
        let exec_res = execute_program_artifact(
            &compiled,
            &Bn254BlackBoxSolver(false),
            &mut DefaultForeignCallBuilder::default().build(),
            prover_file,
        )
        .map_err(|e| BenchError::Message(format!("execution for witness failed: {e}")))?;
        let witness_ms = witness_start.elapsed().as_millis();

        let tempdir = tempfile::tempdir().map_err(|e| BenchError::Message(e.to_string()))?;
        let witness_path = save_witness_to_dir(&exec_res.witness_stack, "witness", tempdir.path())
            .map_err(|e| BenchError::Message(e.to_string()))?;
        // Barretenberg v0.84.0 writes multiple files when proving; pass a directory to -o
        let out_dir = tempfile::tempdir().map_err(|e| BenchError::Message(e.to_string()))?;

        // Build command
        let mut cmd = Command::new(&self.backend_path);
        cmd.arg("prove")
            .arg("-b")
            .arg(artifact)
            .arg("-w")
            .arg(&witness_path)
            .arg("-o")
            .arg(out_dir.path());
        for a in &self.extra_args {
            cmd.arg(a);
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let backend_start = Instant::now();
        let (status, peak_rss) = self.run_bb_with_timeout(cmd, timeout)?;
        let backend_ms = backend_start.elapsed().as_millis();
        let prove_time_ms = witness_ms + backend_ms;
        if !status.success() {
            return Err(BenchError::Message(format!(
                "backend prove failed: status={status}"
            )));
        }

        // Measure sizes of barretenberg's output files
        let proof_file = out_dir.path().join("proof");
        let vk_file = out_dir.path().join("vk");
        let pk_file = out_dir.path().join("pk");

        let proof_size_bytes = std::fs::metadata(&proof_file).ok().map(|m| m.len());
        let verification_key_size_bytes = std::fs::metadata(&vk_file).ok().map(|m| m.len());
        let proving_key_size_bytes = std::fs::metadata(&pk_file).ok().map(|m| m.len());

        let artifact_bytes = std::fs::read(artifact).ok();
        let inputs_bytes = inputs.and_then(|p| std::fs::read(p).ok());
        let meta = CommonMeta {
            name: "prove".into(),
            timestamp: time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default(),
            noir_version: program.noir_version.clone(),
            artifact_path: artifact.to_path_buf(),
            cli_args: std::env::args().collect(),
            artifact_sha256: artifact_bytes.as_ref().map(|b| crate::sha256_hex(b)),
            inputs_sha256: inputs_bytes.as_ref().map(|b| crate::sha256_hex(b)),
        };
        let report = ProveReport {
            meta,
            prove_time_ms,
            witness_gen_time_ms: Some(witness_ms),
            backend_prove_time_ms: Some(backend_ms),
            peak_memory_bytes: peak_rss,
            proof_size_bytes,
            proving_key_size_bytes,
            verification_key_size_bytes,
            gate_count: None,
            backend: self.backend_info(),
            system: Some(collect_system_info()),
            iterations: None,
        };
        Ok(report)
    }

    fn backend_info(&self) -> BackendInfo {
        // Try `bb --version`
        let version = std::process::Command::new(&self.backend_path)
            .arg("--version")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        BackendInfo {
            name: "barretenberg".into(),
            version,
        }
    }
}

pub struct GenericProverProvider {
    pub command_template: String,
    pub extra_args: Vec<String>,
}

impl GenericProverProvider {
    fn build_command(
        &self,
        artifact: &Path,
        witness: &Path,
        proof: &Path,
    ) -> BenchResult<std::process::Command> {
        let mut parts: Vec<String> = Shlex::new(&self.command_template).collect();
        if parts.is_empty() {
            return Err(BenchError::Message("empty command template".into()));
        }
        let artifact_s = artifact.to_string_lossy();
        let witness_s = witness.to_string_lossy();
        let proof_s = proof.to_string_lossy();
        for p in &mut parts {
            *p = p
                .replace("{artifact}", &artifact_s)
                .replace("{witness}", &witness_s)
                .replace("{proof}", &proof_s);
        }
        let mut cmd = std::process::Command::new(&parts[0]);
        for p in &parts[1..] {
            cmd.arg(p);
        }
        for a in &self.extra_args {
            cmd.arg(a);
        }
        Ok(cmd)
    }
}

impl ProverProvider for GenericProverProvider {
    fn prove(
        &self,
        artifact: &Path,
        inputs: Option<&Path>,
        _timeout: Duration,
    ) -> BenchResult<ProveReport> {
        // Load artifact to get version and build witness using in-process, like Barretenberg flow
        let program =
            read_program_from_file(artifact).map_err(|e| BenchError::Message(e.to_string()))?;
        let compiled: noirc_driver::CompiledProgram = program.clone().into();
        let prover_file = inputs.map(|p| p.with_extension("toml"));
        let prover_file = prover_file
            .as_ref()
            .map(|p| p.as_path())
            .unwrap_or_else(|| Path::new("Prover.toml"));
        let exec_res = execute_program_artifact(
            &compiled,
            &Bn254BlackBoxSolver(false),
            &mut DefaultForeignCallBuilder::default().build(),
            prover_file,
        )
        .map_err(|e| BenchError::Message(format!("execution for witness failed: {e}")))?;

        let tempdir = tempfile::tempdir().map_err(|e| BenchError::Message(e.to_string()))?;
        let witness_path = save_witness_to_dir(&exec_res.witness_stack, "witness", tempdir.path())
            .map_err(|e| BenchError::Message(e.to_string()))?;
        let proof_path = tempdir.path().join("proof.bin");

        let mut cmd = self.build_command(artifact, &witness_path, &proof_path)?;
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // crude timeout handling
        let start = Instant::now();
        let status = cmd
            .status()
            .map_err(|e| BenchError::Message(e.to_string()))?;
        let prove_time_ms = start.elapsed().as_millis();
        if !status.success() {
            return Err(BenchError::Message(format!(
                "generic prove failed: status={status}"
            )));
        }
        let proof_size_bytes = std::fs::metadata(&proof_path).ok().map(|m| m.len() as u64);
        let artifact_bytes = std::fs::read(artifact).ok();
        let inputs_bytes = inputs.and_then(|p| std::fs::read(p).ok());
        let meta = CommonMeta {
            name: "prove".into(),
            timestamp: time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default(),
            noir_version: program.noir_version.clone(),
            artifact_path: artifact.to_path_buf(),
            cli_args: std::env::args().collect(),
            artifact_sha256: artifact_bytes.as_ref().map(|b| crate::sha256_hex(b)),
            inputs_sha256: inputs_bytes.as_ref().map(|b| crate::sha256_hex(b)),
        };
        Ok(ProveReport {
            meta,
            prove_time_ms,
            witness_gen_time_ms: None,
            backend_prove_time_ms: None,
            peak_memory_bytes: None,
            proof_size_bytes,
            proving_key_size_bytes: None,
            verification_key_size_bytes: None,
            gate_count: None,
            backend: BackendInfo {
                name: "generic".into(),
                version: None,
            },
            system: Some(collect_system_info()),
            iterations: None,
        })
    }

    fn backend_info(&self) -> BackendInfo {
        let mut sh = Shlex::new(&self.command_template);
        let program = sh.next().unwrap_or_else(|| "generic".into());
        let version = std::process::Command::new(&program)
            .arg("--version")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        BackendInfo {
            name: "generic".into(),
            version,
        }
    }
}

/// Prove using the new unified Backend trait.
///
/// This function demonstrates the new `Backend` abstraction. It generates a witness
/// from the inputs, then uses `BarretenbergBackend::prove()` to create the proof.
pub fn prove_with_backend<B: Backend>(
    backend: &B,
    artifact: &Path,
    inputs: Option<&Path>,
    timeout: Duration,
) -> BenchResult<ProveReport> {
    // Read artifact and generate witness
    let program =
        read_program_from_file(artifact).map_err(|e| BenchError::Message(e.to_string()))?;
    let compiled: noirc_driver::CompiledProgram = program.clone().into();
    let prover_file = inputs.map(|p| p.with_extension("toml"));
    let prover_file = prover_file
        .as_ref()
        .map(|p| p.as_path())
        .unwrap_or_else(|| Path::new("Prover.toml"));

    let witness_start = Instant::now();
    let exec_res = execute_program_artifact(
        &compiled,
        &Bn254BlackBoxSolver(false),
        &mut DefaultForeignCallBuilder::default().build(),
        prover_file,
    )
    .map_err(|e| BenchError::Message(format!("execution for witness failed: {e}")))?;
    let witness_ms = witness_start.elapsed().as_millis();

    let tempdir = tempfile::tempdir().map_err(|e| BenchError::Message(e.to_string()))?;
    let witness_path = save_witness_to_dir(&exec_res.witness_stack, "witness", tempdir.path())
        .map_err(|e| BenchError::Message(e.to_string()))?;

    // Use the unified Backend trait
    let output = backend.prove(artifact, Some(&witness_path), timeout)?;

    let artifact_bytes = std::fs::read(artifact).ok();
    let inputs_bytes = inputs.and_then(|p| std::fs::read(p).ok());
    let meta = CommonMeta {
        name: "prove".into(),
        timestamp: time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default(),
        noir_version: program.noir_version.clone(),
        artifact_path: artifact.to_path_buf(),
        cli_args: std::env::args().collect(),
        artifact_sha256: artifact_bytes.as_ref().map(|b| crate::sha256_hex(b)),
        inputs_sha256: inputs_bytes.as_ref().map(|b| crate::sha256_hex(b)),
    };

    let backend_info = BackendInfo {
        name: backend.name().to_string(),
        version: backend.version(),
    };

    Ok(ProveReport {
        meta,
        prove_time_ms: witness_ms + output.prove_time_ms,
        witness_gen_time_ms: Some(witness_ms),
        backend_prove_time_ms: output.backend_prove_time_ms,
        peak_memory_bytes: output.peak_memory_bytes,
        proof_size_bytes: output.proof_size_bytes,
        proving_key_size_bytes: output.proving_key_size_bytes,
        verification_key_size_bytes: output.verification_key_size_bytes,
        gate_count: None,
        backend: backend_info,
        system: Some(collect_system_info()),
        iterations: None,
    })
}

/// Prove using the new engine workflow (Toolchain + Backend composition).
///
/// This function demonstrates the engine abstraction that cleanly separates:
/// - Toolchain: Noir-specific operations (witness generation via NargoToolchain)
/// - Backend: Proving system operations (proof generation via BarretenbergBackend)
///
/// The output is converted to ProveReport for CLI compatibility.
pub fn prove_with_engine<T: Toolchain, B: Backend>(
    toolchain: &T,
    backend: &B,
    artifact: &Path,
    inputs: Option<&Path>,
    timeout: Duration,
) -> BenchResult<ProveReport> {
    // Read artifact to get noir version for CommonMeta
    let program =
        read_program_from_file(artifact).map_err(|e| BenchError::Message(e.to_string()))?;

    // Prepare workflow inputs
    let circuit_name = artifact
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let mut prove_inputs = ProveInputs::new(artifact, circuit_name).with_timeout(timeout);

    if let Some(prover_toml) = inputs {
        prove_inputs = prove_inputs.with_prover_toml(prover_toml);
    }

    // Run the engine workflow
    let bench_record = engine::prove_only(toolchain, backend, &prove_inputs)?;

    // Convert BenchRecord to ProveReport for CLI compatibility
    let artifact_bytes = std::fs::read(artifact).ok();
    let inputs_bytes = inputs.and_then(|p| std::fs::read(p).ok());

    let meta = CommonMeta {
        name: "prove".into(),
        timestamp: bench_record.timestamp.clone(),
        noir_version: program.noir_version.clone(),
        artifact_path: artifact.to_path_buf(),
        cli_args: std::env::args().collect(),
        artifact_sha256: artifact_bytes.as_ref().map(|b| crate::sha256_hex(b)),
        inputs_sha256: inputs_bytes.as_ref().map(|b| crate::sha256_hex(b)),
    };

    // Extract timing from BenchRecord's TimingStat
    let witness_ms = bench_record
        .witness_stats
        .as_ref()
        .map(|s| s.mean_ms as u128);

    let prove_ms = bench_record
        .prove_stats
        .as_ref()
        .map(|s| s.mean_ms as u128)
        .unwrap_or(0);

    let total_ms = witness_ms.unwrap_or(0) + prove_ms;

    let backend_info = BackendInfo {
        name: bench_record.backend.name.clone(),
        version: bench_record.backend.version.clone(),
    };

    // Convert peak_rss_mb back to bytes
    let peak_memory_bytes = bench_record
        .peak_rss_mb
        .map(|mb| (mb * 1024.0 * 1024.0).round() as u64);

    Ok(ProveReport {
        meta,
        prove_time_ms: total_ms,
        witness_gen_time_ms: witness_ms,
        backend_prove_time_ms: Some(prove_ms),
        peak_memory_bytes,
        proof_size_bytes: bench_record.proof_size_bytes,
        proving_key_size_bytes: bench_record.proving_key_size_bytes,
        verification_key_size_bytes: bench_record.verification_key_size_bytes,
        gate_count: bench_record.total_gates,
        backend: backend_info,
        system: Some(collect_system_info()),
        iterations: None,
    })
}

pub fn run(
    artifact: PathBuf,
    prover_toml: Option<PathBuf>,
    backend: Option<String>,
    backend_path: Option<PathBuf>,
    backend_args: Vec<String>,
    command_template: Option<String>,
    timeout_secs: u64,
    iterations: Option<usize>,
    warmup: Option<usize>,
    json_out: Option<PathBuf>,
) -> BenchResult<()> {
    let backend_name = backend.unwrap_or_else(|| "barretenberg".to_string());
    // Default to `bb` from PATH for the barretenberg backend when no path is provided.
    let backend_path = match backend_path {
        Some(p) => Some(p),
        None if backend_name == "barretenberg" && command_template.is_none() => {
            Some(PathBuf::from("bb"))
        }
        None => None,
    };
    let timeout = if timeout_secs == 0 {
        Duration::from_secs(24 * 60 * 60)
    } else {
        Duration::from_secs(timeout_secs)
    };

    let iter_n = iterations.unwrap_or(1);
    let warmup_n = warmup.unwrap_or(0);
    let mut last_report: Option<ProveReport> = None;
    let mut times: Vec<u128> = Vec::new();

    // Create the unified backend for barretenberg (used for the new code path)
    let unified_backend: Option<BarretenbergBackend> =
        if backend_name == "barretenberg" && command_template.is_none() {
            backend_path.as_ref().map(|path| {
                let config = BarretenbergConfig::new(path)
                    .with_args(backend_args.clone())
                    .with_timeout(timeout);
                BarretenbergBackend::new(config)
            })
        } else {
            None
        };

    // Create toolchain for engine workflow (uses nargo from PATH)
    let toolchain = NargoToolchain::new();

    for i in 0..(warmup_n + iter_n) {
        let res = match (
            backend_name.as_str(),
            command_template.as_ref(),
            &unified_backend,
        ) {
            // Engine workflow path: use Toolchain + Backend composition
            // This is the preferred path that cleanly separates concerns
            ("barretenberg", None, Some(bb)) => {
                prove_with_engine(&toolchain, bb, &artifact, prover_toml.as_deref(), timeout)
            }
            // Legacy code path: use BarretenbergProverProvider
            ("barretenberg", None, None) => {
                let Some(path) = backend_path.clone() else {
                    return Err(BenchError::Message(
                        "barretenberg prover requires --backend-path".into(),
                    ));
                };
                let provider = BarretenbergProverProvider {
                    backend_path: path,
                    extra_args: backend_args.clone(),
                };
                provider.prove(&artifact, prover_toml.as_deref(), timeout)
            }
            (_, Some(tpl), _) => {
                let provider = GenericProverProvider {
                    command_template: tpl.clone(),
                    extra_args: backend_args.clone(),
                };
                provider.prove(&artifact, prover_toml.as_deref(), timeout)
            }
            (other, None, _) => {
                let provider = NotImplementedProver {
                    backend_name: other.to_string(),
                };
                provider.prove(&artifact, prover_toml.as_deref(), timeout)
            }
        }?;
        if i >= warmup_n {
            times.push(res.prove_time_ms);
        }
        last_report = Some(res);
    }

    let mut result = last_report.expect("at least one iteration");
    if iter_n > 1 || warmup_n > 0 {
        let stats: IterationStats = compute_iteration_stats(times, iter_n, warmup_n);
        result.iterations = Some(stats);
    }

    if let Some(json) = json_out {
        if let Some(dir) = json.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        std::fs::write(&json, serde_json::to_vec_pretty(&result).unwrap()).ok();
    }
    println!(
        "prove: backend={} time={}ms size={:?}",
        result.backend.name, result.prove_time_ms, result.proof_size_bytes
    );
    Ok(())
}
