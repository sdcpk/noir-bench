use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use bn254_blackbox_solver::Bn254BlackBoxSolver;
use noir_artifact_cli::execution::execute as execute_program_artifact;
use noir_artifact_cli::fs::artifact::read_program_from_file;
use noir_artifact_cli::fs::witness::save_witness_to_dir;
use nargo::foreign_calls::DefaultForeignCallBuilder;

use crate::{BenchError, BenchResult};

use super::config::CircuitSpec;

#[derive(Debug, Clone)]
pub struct CompileOutput {
    pub compile_time_ms: u128,
    pub constraints: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct ProofOutput {
    pub prove_time_ms: u128,
    pub backend_prove_time_ms: Option<u128>,
    pub witness_gen_time_ms: Option<u128>,
    pub peak_memory_bytes: Option<u64>,
    pub proof_size_bytes: Option<u64>,
    pub proof_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct VerifyOutput {
    pub verify_time_ms: Option<u128>,
    pub success: bool,
    pub gas_used: Option<u64>,
}

pub trait Backend {
    fn name(&self) -> &'static str;
    fn compile(&self, circuit: &CircuitSpec) -> BenchResult<CompileOutput>;
    fn prove(&self, circuit: &CircuitSpec) -> BenchResult<ProofOutput>;
    fn verify(&self, proof: &ProofOutput) -> BenchResult<VerifyOutput>;
    fn constraints(&self, circuit: &CircuitSpec) -> BenchResult<u64>;
}

pub struct BarretenbergBackend {
    pub bb_path: PathBuf,
    pub extra_args: Vec<String>,
}

impl BarretenbergBackend {
    fn run_bb_with_timeout(
        &self,
        mut cmd: Command,
        timeout: Duration,
    ) -> BenchResult<(std::process::ExitStatus, Option<u64>, u128)> {
        #[cfg(feature = "mem")]
        use sysinfo::{ProcessRefreshKind, RefreshKind, System};
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
                    if let Some(pid) = child.id().try_into().ok().map(sysinfo::Pid::from_u32) {
                        sys.refresh_process(pid);
                        if let Some(p) = sys.process(pid) { peak_rss = peak_rss.max(p.memory()); }
                    }
                }
                let elapsed = start.elapsed().as_millis();
                return Ok((status, {
                    #[cfg(feature = "mem")]
                    { Some(peak_rss) }
                    #[cfg(not(feature = "mem"))]
                    { None }
                }, elapsed));
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
                    if let Some(p) = sys.process(pid) { peak_rss = peak_rss.max(p.memory()); }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}

impl Backend for BarretenbergBackend {
    fn name(&self) -> &'static str { "barretenberg" }

    fn compile(&self, circuit: &CircuitSpec) -> BenchResult<CompileOutput> {
        // "Compile" is approximated by parsing the artifact and asking for constraints via gates
        let start = Instant::now();
        // parse artifact to validate it
        let _program = read_program_from_file(&circuit.path).map_err(|e| BenchError::Message(e.to_string()))?;
        let compile_ms = start.elapsed().as_millis();
        // constraints via gates
        let constraints = self.constraints(circuit).ok();
        Ok(CompileOutput { compile_time_ms: compile_ms, constraints })
    }

    fn prove(&self, circuit: &CircuitSpec) -> BenchResult<ProofOutput> {
        // Build witness in-process using artifact and optional Prover.toml near artifact
        let program = read_program_from_file(&circuit.path).map_err(|e| BenchError::Message(e.to_string()))?;
        let compiled: noirc_driver::CompiledProgram = program.clone().into();
        let prover_file = {
            // try alongside artifact or parent of target/
            let mut p = circuit.path.clone();
            p.set_extension("toml");
            if p.exists() {
                Some(p)
            } else {
                circuit.path.parent().and_then(|dir| {
                    dir.parent().map(|pp| pp.join("Prover.toml")).filter(|cand| cand.exists())
                })
            }
        };
        let prover_path_opt = prover_file.as_ref().map(|p| p.as_path()).unwrap_or_else(|| std::path::Path::new("Prover.toml"));
        let witness_start = Instant::now();
        let exec_res = execute_program_artifact(&compiled, &Bn254BlackBoxSolver(false), &mut DefaultForeignCallBuilder::default().build(), prover_path_opt)
            .map_err(|e| BenchError::Message(format!("execution for witness failed: {e}")))?;
        let witness_ms = witness_start.elapsed().as_millis();

        let tempdir = tempfile::tempdir().map_err(|e| BenchError::Message(e.to_string()))?;
        let witness_path = save_witness_to_dir(&exec_res.witness_stack, "witness", tempdir.path())
            .map_err(|e| BenchError::Message(e.to_string()))?;
        let out_dir = tempfile::tempdir().map_err(|e| BenchError::Message(e.to_string()))?;

        let mut cmd = Command::new(&self.bb_path);
        cmd.arg("prove")
            .arg("-b").arg(&circuit.path)
            .arg("-w").arg(&witness_path)
            .arg("-o").arg(out_dir.path());
        for a in &self.extra_args { cmd.arg(a); }
        cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let (status, peak_rss, backend_ms) = self.run_bb_with_timeout(cmd, Duration::from_secs(24 * 60 * 60))?;
        if !status.success() {
            return Err(BenchError::Message(format!("backend prove failed: status={status}")));
        }
        let proof_file = out_dir.path().join("proof");
        let proof_size_bytes = std::fs::metadata(&proof_file).ok().map(|m| m.len() as u64);
        let prove_time_ms = backend_ms + witness_ms;
        Ok(ProofOutput {
            prove_time_ms,
            backend_prove_time_ms: Some(backend_ms),
            witness_gen_time_ms: Some(witness_ms),
            peak_memory_bytes: peak_rss,
            proof_size_bytes,
            proof_path: Some(proof_file),
        })
    }

    fn verify(&self, proof: &ProofOutput) -> BenchResult<VerifyOutput> {
        let proof_path = proof.proof_path.as_ref().ok_or_else(|| BenchError::Message("missing proof_path for verify".into()))?;
        let mut cmd = Command::new(&self.bb_path);
        cmd.arg("verify").arg("-p").arg(proof_path);
        for a in &self.extra_args { cmd.arg(a); }
        cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let start = Instant::now();
        let status = cmd.status().map_err(|e| BenchError::Message(e.to_string()))?;
        let verify_time_ms = start.elapsed().as_millis();
        Ok(VerifyOutput { verify_time_ms: Some(verify_time_ms), success: status.success(), gas_used: None })
    }

    fn constraints(&self, circuit: &CircuitSpec) -> BenchResult<u64> {
        // Reuse gates_cmd provider to query gates
        let mut cmd = Command::new(&self.bb_path);
        cmd.arg("gates").arg("-b").arg(&circuit.path);
        for a in &self.extra_args { cmd.arg(a); }
        let output = cmd.output().map_err(|e| BenchError::Message(e.to_string()))?;
        if !output.status.success() {
            return Err(BenchError::Message(format!(
                "backend gates failed: status={} stderr={}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        #[derive(serde::Deserialize)]
        struct BackendGatesResponse { functions: Vec<BackendGatesReport> }
        #[derive(serde::Deserialize)]
        struct BackendGatesReport {
            #[serde(alias = "circuit_size")]
            pub total_gates: usize,
        }
        let parsed: BackendGatesResponse = serde_json::from_slice(&output.stdout)
            .map_err(|e| BenchError::Message(format!("failed to parse gates json: {e}")))?;
        let total = parsed.functions.get(0).map(|f| f.total_gates as u64).unwrap_or(0);
        Ok(total)
    }
}

pub struct EvmBackend {
    pub foundry_dir: PathBuf,
    pub forge_bin: Option<PathBuf>,
    pub test_pattern: Option<String>,
    pub gas_per_second: Option<u64>,
}

impl Backend for EvmBackend {
    fn name(&self) -> &'static str { "evm" }

    fn compile(&self, _circuit: &CircuitSpec) -> BenchResult<CompileOutput> {
        Err(BenchError::Message("compile not supported for EVM backend".into()))
    }

    fn prove(&self, _circuit: &CircuitSpec) -> BenchResult<ProofOutput> {
        Err(BenchError::Message("prove not supported for EVM backend".into()))
    }

    fn verify(&self, _proof: &ProofOutput) -> BenchResult<VerifyOutput> {
        // Delegate to existing evm_verify_cmd logic: run `forge test --gas-report` and parse gas
        let forge = self.forge_bin.clone().unwrap_or_else(|| PathBuf::from("forge"));
        let mut cmd = Command::new(&forge);
        cmd.arg("test").arg("--gas-report");
        if let Some(pat) = &self.test_pattern { cmd.arg("-m").arg(pat); }
        cmd.current_dir(&self.foundry_dir);
        cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let output = cmd.output().map_err(|e| BenchError::Message(format!("failed to run forge: {e}")))?;
        let stdout_s = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr_s = String::from_utf8_lossy(&output.stderr).to_string();
        if !output.status.success() {
            return Err(BenchError::Message(format!("forge test failed. stderr=\n{}", stderr_s)));
        }
        // Prefer .gas-snapshot, fallback to stdout heuristic
        let snapshot_path = self.foundry_dir.join(".gas-snapshot");
        let gas_used = {
            fn read_gas_from_snapshot(snapshot_path: &Path, match_pattern: &Option<String>) -> Option<u64> {
                let Ok(contents) = std::fs::read_to_string(snapshot_path) else { return None; };
                let lines = contents.lines();
                let mut best: Option<u64> = None;
                for line in lines {
                    if let Some(pat) = match_pattern {
                        if !line.contains(pat) { continue; }
                    }
                    if let Some(start_idx) = line.find("(gas:") {
                        let slice = &line[start_idx + 5..];
                        if let Some(end_idx) = slice.find(')') {
                            let num_s = slice[..end_idx].trim().trim_start_matches(':').trim();
                            let num_s = num_s.chars().filter(|c| c.is_ascii_digit()).collect::<String>();
                            if let Ok(v) = num_s.parse::<u64>() {
                                best = Some(v);
                                break;
                            }
                        }
                    }
                }
                best
            }
            fn read_gas_from_stdout(stdout: &str) -> Option<u64> {
                if let Some(idx) = stdout.find("gas:") {
                    let mut s = &stdout[idx + 4..];
                    s = s.trim_start();
                    let num: String = s.chars().take_while(|c| c.is_ascii_digit() || *c == '_').collect();
                    let num = num.replace('_', "");
                    if let Ok(v) = num.parse::<u64>() { return Some(v); }
                }
                None
            }
            read_gas_from_snapshot(&snapshot_path, &self.test_pattern)
                .or_else(|| read_gas_from_stdout(&stdout_s))
                .ok_or_else(|| BenchError::Message("failed to parse gas used from Foundry outputs".into()))?
        };
        Ok(VerifyOutput { verify_time_ms: None, success: true, gas_used: Some(gas_used) })
    }

    fn constraints(&self, _circuit: &CircuitSpec) -> BenchResult<u64> {
        Err(BenchError::Message("constraints not supported for EVM backend".into()))
    }
}


