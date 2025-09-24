use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use std::process::{Command, Stdio};

use bn254_blackbox_solver::Bn254BlackBoxSolver;
use noir_artifact_cli::fs::artifact::read_program_from_file;
use noir_artifact_cli::fs::witness::save_witness_to_dir;
use noir_artifact_cli::execution::execute as execute_program_artifact;
use nargo::foreign_calls::DefaultForeignCallBuilder;

use crate::{BackendInfo, BenchError, BenchResult, CommonMeta, ProveReport, collect_system_info, compute_iteration_stats, IterationStats};
use shlex::Shlex;

pub trait ProverProvider {
    fn prove(&self, artifact: &Path, inputs: Option<&Path>, timeout: Duration) -> BenchResult<ProveReport>;
    fn backend_info(&self) -> BackendInfo;
}

pub struct NotImplementedProver {
    pub backend_name: String,
}

impl ProverProvider for NotImplementedProver {
    fn prove(&self, _artifact: &Path, _inputs: Option<&Path>, _timeout: Duration) -> BenchResult<ProveReport> {
        Err(BenchError::Message(format!("prove not implemented for backend '{}'", self.backend_name)))
    }
    fn backend_info(&self) -> BackendInfo { BackendInfo { name: self.backend_name.clone(), version: None } }
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
    ) -> BenchResult<std::process::ExitStatus> {
        #[cfg(feature = "mem")]
        use sysinfo::{PidExt, ProcessRefreshKind, RefreshKind, System, SystemExt};

        let start = Instant::now();
        let mut child = cmd.spawn().map_err(|e| BenchError::Message(e.to_string()))?;

        #[cfg(feature = "mem")]
        let mut sys = System::new_with_specifics(RefreshKind::new().with_processes(ProcessRefreshKind::everything()));
        #[cfg(feature = "mem")]
        let mut peak_rss: u64 = 0;

        loop {
            if let Some(status) = child.try_wait().map_err(|e| BenchError::Message(e.to_string()))? {
                #[cfg(feature = "mem")]
                {
                    // final sample
                    if let Some(pid) = child.id().try_into().ok().map(sysinfo::Pid::from_u32) {
                        sys.refresh_process(pid);
                        if let Some(p) = sys.process(pid) {
                            peak_rss = peak_rss.max(p.memory());
                        }
                    }
                }
                return Ok(status);
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
                        peak_rss = peak_rss.max(p.memory());
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl ProverProvider for BarretenbergProverProvider {
    fn prove(&self, artifact: &Path, inputs: Option<&Path>, timeout: Duration) -> BenchResult<ProveReport> {
        // Read artifact
        let program = read_program_from_file(artifact).map_err(|e| BenchError::Message(e.to_string()))?;

        // Generate witness from inputs using in-process execution
        let compiled: noirc_driver::CompiledProgram = program.clone().into();
        let prover_file = inputs.map(|p| p.with_extension("toml"));
        let prover_file = prover_file.as_ref().map(|p| p.as_path()).unwrap_or_else(|| Path::new("Prover.toml"));
        let exec_res = execute_program_artifact(&compiled, &Bn254BlackBoxSolver(false), &mut DefaultForeignCallBuilder::default().build(), prover_file)
            .map_err(|e| BenchError::Message(format!("execution for witness failed: {e}")))?;

        let tempdir = tempfile::tempdir().map_err(|e| BenchError::Message(e.to_string()))?;
        let witness_path = save_witness_to_dir(&exec_res.witness_stack, "witness", tempdir.path())
            .map_err(|e| BenchError::Message(e.to_string()))?;
        // Barretenberg v0.84.0 writes multiple files when proving; pass a directory to -o
        let out_dir = tempfile::tempdir().map_err(|e| BenchError::Message(e.to_string()))?;

        // Build command
        let mut cmd = Command::new(&self.backend_path);
        cmd.arg("prove")
            .arg("-b").arg(artifact)
            .arg("-w").arg(&witness_path)
            .arg("-o").arg(out_dir.path());
        for a in &self.extra_args { cmd.arg(a); }
        cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());

        let start = Instant::now();
        let status = self.run_bb_with_timeout(cmd, timeout)?;
        let prove_time_ms = start.elapsed().as_millis();
        if !status.success() {
            return Err(BenchError::Message(format!("backend prove failed: status={status}")));
        }

        // Prefer size of barretenberg's default proof file inside the output directory
        let proof_size_bytes = {
            let proof_file = out_dir.path().join("proof");
            std::fs::metadata(&proof_file)
                .ok()
                .map(|m| m.len() as u64)
        };

        let meta = CommonMeta {
            name: "prove".into(),
            timestamp: time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339).unwrap_or_default(),
            noir_version: program.noir_version.clone(),
            artifact_path: artifact.to_path_buf(),
            cli_args: std::env::args().collect(),
        };
        let report = ProveReport {
            meta,
            prove_time_ms,
            peak_memory_bytes: None,
            proof_size_bytes,
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
        BackendInfo { name: "barretenberg".into(), version }
    }
}

pub struct GenericProverProvider {
    pub command_template: String,
    pub extra_args: Vec<String>,
}

impl GenericProverProvider {
    fn build_command(&self, artifact: &Path, witness: &Path, proof: &Path) -> BenchResult<std::process::Command> {
        let mut parts: Vec<String> = Shlex::new(&self.command_template).collect();
        if parts.is_empty() { return Err(BenchError::Message("empty command template".into())); }
        let artifact_s = artifact.to_string_lossy();
        let witness_s = witness.to_string_lossy();
        let proof_s = proof.to_string_lossy();
        for p in &mut parts {
            *p = p.replace("{artifact}", &artifact_s).replace("{witness}", &witness_s).replace("{proof}", &proof_s);
        }
        let mut cmd = std::process::Command::new(&parts[0]);
        for p in &parts[1..] { cmd.arg(p); }
        for a in &self.extra_args { cmd.arg(a); }
        Ok(cmd)
    }
}

impl ProverProvider for GenericProverProvider {
    fn prove(&self, artifact: &Path, inputs: Option<&Path>, _timeout: Duration) -> BenchResult<ProveReport> {
        // Load artifact to get version and build witness using in-process, like Barretenberg flow
        let program = read_program_from_file(artifact).map_err(|e| BenchError::Message(e.to_string()))?;
        let compiled: noirc_driver::CompiledProgram = program.clone().into();
        let prover_file = inputs.map(|p| p.with_extension("toml"));
        let prover_file = prover_file.as_ref().map(|p| p.as_path()).unwrap_or_else(|| Path::new("Prover.toml"));
        let exec_res = execute_program_artifact(&compiled, &Bn254BlackBoxSolver(false), &mut DefaultForeignCallBuilder::default().build(), prover_file)
            .map_err(|e| BenchError::Message(format!("execution for witness failed: {e}")))?;

        let tempdir = tempfile::tempdir().map_err(|e| BenchError::Message(e.to_string()))?;
        let witness_path = save_witness_to_dir(&exec_res.witness_stack, "witness", tempdir.path())
            .map_err(|e| BenchError::Message(e.to_string()))?;
        let proof_path = tempdir.path().join("proof.bin");

        let mut cmd = self.build_command(artifact, &witness_path, &proof_path)?;
        cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());

        // crude timeout handling
        let start = Instant::now();
        let status = cmd.status().map_err(|e| BenchError::Message(e.to_string()))?;
        let prove_time_ms = start.elapsed().as_millis();
        if !status.success() {
            return Err(BenchError::Message(format!("generic prove failed: status={status}")));
        }
        let proof_size_bytes = std::fs::metadata(&proof_path).ok().map(|m| m.len() as u64);
        let meta = CommonMeta {
            name: "prove".into(),
            timestamp: time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339).unwrap_or_default(),
            noir_version: program.noir_version.clone(),
            artifact_path: artifact.to_path_buf(),
            cli_args: std::env::args().collect(),
        };
        Ok(ProveReport {
            meta,
            prove_time_ms,
            peak_memory_bytes: None,
            proof_size_bytes,
            gate_count: None,
            backend: BackendInfo { name: "generic".into(), version: None },
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
        BackendInfo { name: "generic".into(), version }
    }
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
    let timeout = if timeout_secs == 0 { Duration::from_secs(24 * 60 * 60) } else { Duration::from_secs(timeout_secs) };

    let iter_n = iterations.unwrap_or(1);
    let warmup_n = warmup.unwrap_or(0);
    let mut last_report: Option<ProveReport> = None;
    let mut times: Vec<u128> = Vec::new();

    for i in 0..(warmup_n + iter_n) {
        let res = match (backend_name.as_str(), command_template.as_ref()) {
            ("barretenberg", None) => {
                let Some(path) = backend_path.clone() else { return Err(BenchError::Message("barretenberg prover requires --backend-path".into())); };
                let provider = BarretenbergProverProvider { backend_path: path, extra_args: backend_args.clone() };
                provider.prove(&artifact, prover_toml.as_deref(), timeout)
            }
            (_, Some(tpl)) => {
                let provider = GenericProverProvider { command_template: tpl.clone(), extra_args: backend_args.clone() };
                provider.prove(&artifact, prover_toml.as_deref(), timeout)
            }
            (other, None) => {
                let provider = NotImplementedProver { backend_name: other.to_string() };
                provider.prove(&artifact, prover_toml.as_deref(), timeout)
            }
        }?;
        if i >= warmup_n { times.push(res.prove_time_ms); }
        last_report = Some(res);
    }

    let mut result = last_report.expect("at least one iteration");
    if iter_n > 1 || warmup_n > 0 {
        let stats: IterationStats = compute_iteration_stats(times, iter_n, warmup_n);
        result.iterations = Some(stats);
    }

    if let Some(json) = json_out {
        if let Some(dir) = json.parent() { std::fs::create_dir_all(dir).ok(); }
        std::fs::write(&json, serde_json::to_vec_pretty(&result).unwrap()).ok();
    }
    println!("prove: backend={} time={}ms size={:?}", result.backend.name, result.prove_time_ms, result.proof_size_bytes);
    Ok(())
} 