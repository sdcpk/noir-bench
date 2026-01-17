//! Workflow orchestration for benchmark operations.
//!
//! This module composes `Toolchain` (compile, witness gen) and `Backend` (prove, verify)
//! to execute complete benchmark workflows while collecting timing statistics.
//!
//! # Design
//!
//! Workflows produce `BenchRecord` v1 outputs that are compatible with the existing
//! storage and reporting infrastructure (JSONL, CSV export, compare, etc.).

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::BenchResult;
use crate::backend::Backend;
use crate::core::{BackendInfo, BenchRecord, EnvironmentInfo, RunConfig, TimingStat};

use super::toolchain::Toolchain;

/// Inputs for a prove workflow.
#[derive(Debug, Clone)]
pub struct ProveInputs {
    /// Path to the compiled artifact (program.json)
    pub artifact_path: PathBuf,
    /// Optional path to Prover.toml inputs
    pub prover_toml: Option<PathBuf>,
    /// Circuit name for the record
    pub circuit_name: String,
    /// Timeout for backend operations
    pub timeout: Duration,
}

impl ProveInputs {
    /// Create new ProveInputs with required fields.
    pub fn new(artifact_path: impl Into<PathBuf>, circuit_name: impl Into<String>) -> Self {
        ProveInputs {
            artifact_path: artifact_path.into(),
            prover_toml: None,
            circuit_name: circuit_name.into(),
            timeout: Duration::from_secs(300), // 5 minute default
        }
    }

    /// Set the Prover.toml path.
    pub fn with_prover_toml(mut self, prover_toml: impl Into<PathBuf>) -> Self {
        self.prover_toml = Some(prover_toml.into());
        self
    }

    /// Set the timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

/// Execute a prove-only workflow.
///
/// This workflow:
/// 1. Uses the toolchain to generate a witness from artifact + inputs
/// 2. Uses the backend to generate a proof
/// 3. Returns a BenchRecord v1 compatible record with timing stats
///
/// # Arguments
/// * `toolchain` - The toolchain for witness generation
/// * `backend` - The backend for proving
/// * `inputs` - The workflow inputs
///
/// # Returns
/// A `BenchRecord` with prove_stats, witness_stats, and size metrics populated.
pub fn prove_only(
    toolchain: &dyn Toolchain,
    backend: &dyn Backend,
    inputs: &ProveInputs,
) -> BenchResult<BenchRecord> {
    // Get environment info (includes nargo/bb versions)
    let env = EnvironmentInfo::detect();

    // Get toolchain version for record metadata
    let toolchain_version = toolchain.version().ok();

    // Get backend info
    let backend_info = BackendInfo {
        name: backend.name().to_string(),
        version: backend.version(),
        variant: None,
    };

    // Create run config (single iteration for now)
    let config = RunConfig {
        warmup_iterations: 0,
        measured_iterations: 1,
        timeout_secs: Some(inputs.timeout.as_secs()),
    };

    // Create the record
    let mut record = BenchRecord::new(inputs.circuit_name.clone(), env, backend_info, config);

    // Set circuit path
    record.circuit_path = Some(inputs.artifact_path.to_string_lossy().to_string());

    // Step 1: Generate witness using toolchain
    let prover_toml = inputs
        .prover_toml
        .as_deref()
        .unwrap_or(Path::new("Prover.toml"));
    let witness_result = toolchain.gen_witness(&inputs.artifact_path, prover_toml)?;

    // Record witness timing as TimingStat (single sample)
    let witness_ms = witness_result.witness_gen_time_ms as f64;
    record.witness_stats = Some(TimingStat::from_samples(&[witness_ms]));

    // Step 2: Call backend prove with the generated witness
    let prove_output = backend.prove(
        &inputs.artifact_path,
        Some(&witness_result.witness_path),
        inputs.timeout,
    )?;

    // Record prove timing (backend prove time, not including witness gen)
    let prove_ms = prove_output.prove_time_ms as f64;
    record.prove_stats = Some(TimingStat::from_samples(&[prove_ms]));

    // Record size metrics
    record.proof_size_bytes = prove_output.proof_size_bytes;
    record.proving_key_size_bytes = prove_output.proving_key_size_bytes;
    record.verification_key_size_bytes = prove_output.verification_key_size_bytes;

    // Record artifact size
    if let Ok(metadata) = std::fs::metadata(&inputs.artifact_path) {
        record.artifact_size_bytes = Some(metadata.len());
    }

    // Record peak memory if available
    if let Some(peak_bytes) = prove_output.peak_memory_bytes {
        record.peak_rss_mb = Some(peak_bytes as f64 / (1024.0 * 1024.0));
    }

    // Update env with toolchain version if we got it
    if toolchain_version.is_some() {
        record.env.nargo_version = toolchain_version;
    }

    // Cleanup: remove temp witness file
    let _ = std::fs::remove_file(&witness_result.witness_path);

    Ok(record)
}

/// Execute prove workflow with multiple iterations.
///
/// Runs warmup iterations followed by measured iterations, collecting timing statistics.
///
/// # Arguments
/// * `toolchain` - The toolchain for witness generation
/// * `backend` - The backend for proving
/// * `inputs` - The workflow inputs
/// * `warmup` - Number of warmup iterations (not measured)
/// * `iterations` - Number of measured iterations
///
/// # Returns
/// A `BenchRecord` with aggregated timing stats across all measured iterations.
pub fn prove_with_iterations(
    toolchain: &dyn Toolchain,
    backend: &dyn Backend,
    inputs: &ProveInputs,
    warmup: usize,
    iterations: usize,
) -> BenchResult<BenchRecord> {
    if iterations == 0 {
        return Err(crate::BenchError::Message(
            "iterations must be at least 1".into(),
        ));
    }

    let total_runs = warmup + iterations;
    let mut witness_times: Vec<f64> = Vec::with_capacity(iterations);
    let mut prove_times: Vec<f64> = Vec::with_capacity(iterations);

    // Get environment info once
    let env = EnvironmentInfo::detect();
    let toolchain_version = toolchain.version().ok();

    let backend_info = BackendInfo {
        name: backend.name().to_string(),
        version: backend.version(),
        variant: None,
    };

    let config = RunConfig {
        warmup_iterations: warmup as u32,
        measured_iterations: iterations as u32,
        timeout_secs: Some(inputs.timeout.as_secs()),
    };

    let mut record = BenchRecord::new(inputs.circuit_name.clone(), env, backend_info, config);
    record.circuit_path = Some(inputs.artifact_path.to_string_lossy().to_string());

    let prover_toml = inputs
        .prover_toml
        .as_deref()
        .unwrap_or(Path::new("Prover.toml"));
    let mut last_prove_output = None;

    for i in 0..total_runs {
        let is_warmup = i < warmup;

        // Generate witness
        let witness_result = toolchain.gen_witness(&inputs.artifact_path, prover_toml)?;

        // Run backend prove
        let prove_output = backend.prove(
            &inputs.artifact_path,
            Some(&witness_result.witness_path),
            inputs.timeout,
        )?;

        // Only collect times for measured iterations
        if !is_warmup {
            witness_times.push(witness_result.witness_gen_time_ms as f64);
            prove_times.push(prove_output.prove_time_ms as f64);
        }

        // Cleanup witness file
        let _ = std::fs::remove_file(&witness_result.witness_path);

        // Keep last output for size metrics
        last_prove_output = Some(prove_output);
    }

    // Populate timing stats from collected samples
    record.witness_stats = Some(TimingStat::from_samples(&witness_times));
    record.prove_stats = Some(TimingStat::from_samples(&prove_times));

    // Populate size metrics from last run
    if let Some(output) = last_prove_output {
        record.proof_size_bytes = output.proof_size_bytes;
        record.proving_key_size_bytes = output.proving_key_size_bytes;
        record.verification_key_size_bytes = output.verification_key_size_bytes;
        if let Some(peak_bytes) = output.peak_memory_bytes {
            record.peak_rss_mb = Some(peak_bytes as f64 / (1024.0 * 1024.0));
        }
    }

    // Record artifact size
    if let Ok(metadata) = std::fs::metadata(&inputs.artifact_path) {
        record.artifact_size_bytes = Some(metadata.len());
    }

    // Update env with toolchain version
    if toolchain_version.is_some() {
        record.env.nargo_version = toolchain_version;
    }

    Ok(record)
}

/// Gate info collection status for full benchmarks.
#[derive(Debug, Clone)]
pub enum GateInfoStatus {
    Ok,
    SkippedUnsupported,
    Failed(String),
}

/// Verification status for full benchmarks.
#[derive(Debug, Clone)]
pub enum VerifyStatus {
    Ok,
    SkippedUnsupported,
    SkippedMissingArtifacts,
    Failed(String),
}

/// Result from a full benchmark workflow (compile -> prove -> verify).
///
/// This struct provides all the data needed for both BenchRecord and legacy
/// bench runner JSONL formats.
#[derive(Debug, Clone)]
pub struct FullBenchmarkResult {
    /// The BenchRecord with all timing and size metrics
    pub record: BenchRecord,
    /// Gate count / constraints (from backend.gate_info)
    pub constraints: Option<u64>,
    /// ACIR opcode count (from artifact analysis or nargo info)
    pub acir_opcodes: Option<u64>,
    /// Gate info collection status
    pub gate_info_status: GateInfoStatus,
    /// Verification succeeded
    pub verify_success: bool,
    /// Verification status
    pub verify_status: VerifyStatus,
    /// Verification time in milliseconds
    pub verify_time_ms: Option<u128>,
    /// Proof path (for verify step)
    pub proof_path: Option<PathBuf>,
    /// Verification key path (for verify step)
    pub vk_path: Option<PathBuf>,
}

/// Execute a full benchmark workflow: prove -> verify.
///
/// This workflow:
/// 1. Uses the toolchain to generate a witness
/// 2. Uses the backend to generate a proof
/// 3. Uses the backend to verify the proof
/// 4. Collects gate info from the backend
/// 5. Returns all data needed for both BenchRecord and legacy bench formats
///
/// # Arguments
/// * `toolchain` - The toolchain for witness generation
/// * `backend` - The backend for prove/verify/gate operations
/// * `inputs` - The workflow inputs
/// * `warmup` - Number of warmup iterations (not measured)
/// * `iterations` - Number of measured iterations
///
/// # Returns
/// A `FullBenchmarkResult` with all benchmark data.
pub fn full_benchmark(
    toolchain: &dyn Toolchain,
    backend: &dyn Backend,
    inputs: &ProveInputs,
    warmup: usize,
    iterations: usize,
) -> BenchResult<FullBenchmarkResult> {
    if iterations == 0 {
        return Err(crate::BenchError::Message(
            "iterations must be at least 1".into(),
        ));
    }

    let total_runs = warmup + iterations;
    let mut witness_times: Vec<f64> = Vec::with_capacity(iterations);
    let mut prove_times: Vec<f64> = Vec::with_capacity(iterations);

    // Get environment info once
    let env = EnvironmentInfo::detect();
    let toolchain_version = toolchain.version().ok();

    let backend_info = BackendInfo {
        name: backend.name().to_string(),
        version: backend.version(),
        variant: None,
    };

    let config = RunConfig {
        warmup_iterations: warmup as u32,
        measured_iterations: iterations as u32,
        timeout_secs: Some(inputs.timeout.as_secs()),
    };

    let mut record = BenchRecord::new(inputs.circuit_name.clone(), env, backend_info, config);
    record.circuit_path = Some(inputs.artifact_path.to_string_lossy().to_string());

    let prover_toml = inputs
        .prover_toml
        .as_deref()
        .unwrap_or(Path::new("Prover.toml"));
    let mut last_prove_output = None;

    // Run prove iterations
    for i in 0..total_runs {
        let is_warmup = i < warmup;

        // Generate witness
        let witness_result = toolchain.gen_witness(&inputs.artifact_path, prover_toml)?;

        // Run backend prove
        let prove_output = backend.prove(
            &inputs.artifact_path,
            Some(&witness_result.witness_path),
            inputs.timeout,
        )?;

        // Only collect times for measured iterations
        if !is_warmup {
            witness_times.push(witness_result.witness_gen_time_ms as f64);
            prove_times.push(prove_output.prove_time_ms as f64);
        }

        // Cleanup witness file
        let _ = std::fs::remove_file(&witness_result.witness_path);

        // Keep last output for size metrics and verify
        last_prove_output = Some(prove_output);
    }

    // Populate timing stats from collected samples
    record.witness_stats = Some(TimingStat::from_samples(&witness_times));
    record.prove_stats = Some(TimingStat::from_samples(&prove_times));

    let capabilities = backend.capabilities();

    // Get gate info (constraints)
    let (gate_info, gate_info_status) = if capabilities.has_gate_count {
        match backend.gate_info(&inputs.artifact_path) {
            Ok(info) => (Some(info), GateInfoStatus::Ok),
            Err(err) => (None, GateInfoStatus::Failed(err.to_string())),
        }
    } else {
        (None, GateInfoStatus::SkippedUnsupported)
    };

    let constraints = gate_info.as_ref().map(|g| g.backend_gates);
    let acir_opcodes = gate_info.as_ref().and_then(|g| g.acir_opcodes);

    if let Some(ref gi) = gate_info {
        record.total_gates = Some(gi.backend_gates);
        record.acir_opcodes = gi.acir_opcodes;
        record.subgroup_size = gi.subgroup_size;
    }

    // Populate size metrics from last run
    let (proof_path, vk_path) = if let Some(ref output) = last_prove_output {
        record.proof_size_bytes = output.proof_size_bytes;
        record.proving_key_size_bytes = output.proving_key_size_bytes;
        record.verification_key_size_bytes = output.verification_key_size_bytes;
        if let Some(peak_bytes) = output.peak_memory_bytes {
            record.peak_rss_mb = Some(peak_bytes as f64 / (1024.0 * 1024.0));
        }
        (output.proof_path.clone(), output.vk_path.clone())
    } else {
        (None, None)
    };

    // Record artifact size
    if let Ok(metadata) = std::fs::metadata(&inputs.artifact_path) {
        record.artifact_size_bytes = Some(metadata.len());
    }

    // Update env with toolchain version
    if toolchain_version.is_some() {
        record.env.nargo_version = toolchain_version;
    }

    // Run verification if supported and we have proof/vk paths
    let (verify_success, verify_time_ms, verify_status) = if !capabilities.can_verify {
        (false, None, VerifyStatus::SkippedUnsupported)
    } else {
        match (&proof_path, &vk_path) {
            (Some(proof), Some(vk)) => match backend.verify(proof, vk) {
                Ok(output) => {
                    record.verify_stats =
                        Some(TimingStat::from_samples(&[output.verify_time_ms as f64]));
                    let status = if output.success {
                        VerifyStatus::Ok
                    } else {
                        VerifyStatus::Failed("verification failed".to_string())
                    };
                    (output.success, Some(output.verify_time_ms), status)
                }
                Err(err) => (false, None, VerifyStatus::Failed(err.to_string())),
            },
            _ => (false, None, VerifyStatus::SkippedMissingArtifacts),
        }
    };

    Ok(FullBenchmarkResult {
        record,
        constraints,
        acir_opcodes,
        gate_info_status,
        verify_success,
        verify_status,
        verify_time_ms,
        proof_path,
        vk_path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{MockBackend, MockConfig, ProveOutput};
    use crate::engine::toolchain::MockToolchain;

    fn create_mock_toolchain() -> MockToolchain {
        MockToolchain::new().with_version("0.38.0-test")
    }

    fn create_mock_backend() -> MockBackend {
        MockBackend::new(
            MockConfig::new("mock-backend").with_prove_output(ProveOutput {
                prove_time_ms: 100,
                witness_gen_time_ms: None,
                backend_prove_time_ms: Some(100),
                peak_memory_bytes: Some(50_000_000),
                proof_size_bytes: Some(2048),
                proving_key_size_bytes: Some(1_000_000),
                verification_key_size_bytes: Some(512),
                proof_path: None,
                vk_path: None,
            }),
        )
    }

    #[test]
    fn test_prove_only_returns_bench_record() {
        let toolchain = create_mock_toolchain();
        let backend = create_mock_backend();
        let inputs = ProveInputs::new("/tmp/test-artifact.json", "test-circuit");

        let result = prove_only(&toolchain, &backend, &inputs);
        assert!(result.is_ok());

        let record = result.unwrap();
        assert_eq!(record.circuit_name, "test-circuit");
        assert_eq!(record.backend.name, "mock-backend");
        assert_eq!(record.schema_version, 1);
    }

    #[test]
    fn test_prove_only_populates_timing_stats() {
        let toolchain = create_mock_toolchain();
        let backend = create_mock_backend();
        let inputs = ProveInputs::new("/tmp/test-artifact.json", "test-circuit");

        let record = prove_only(&toolchain, &backend, &inputs).unwrap();

        // Witness stats should be set
        assert!(record.witness_stats.is_some());
        let witness_stats = record.witness_stats.unwrap();
        assert_eq!(witness_stats.iterations, 1);
        assert!(witness_stats.mean_ms > 0.0);

        // Prove stats should be set
        assert!(record.prove_stats.is_some());
        let prove_stats = record.prove_stats.unwrap();
        assert_eq!(prove_stats.iterations, 1);
        assert_eq!(prove_stats.mean_ms, 100.0);
    }

    #[test]
    fn test_prove_only_populates_size_metrics() {
        let toolchain = create_mock_toolchain();
        let backend = create_mock_backend();
        let inputs = ProveInputs::new("/tmp/test-artifact.json", "test-circuit");

        let record = prove_only(&toolchain, &backend, &inputs).unwrap();

        assert_eq!(record.proof_size_bytes, Some(2048));
        assert_eq!(record.proving_key_size_bytes, Some(1_000_000));
        assert_eq!(record.verification_key_size_bytes, Some(512));
    }

    #[test]
    fn test_prove_only_sets_backend_and_toolchain_names() {
        let toolchain = create_mock_toolchain();
        let backend = create_mock_backend();
        let inputs = ProveInputs::new("/tmp/test-artifact.json", "test-circuit");

        let record = prove_only(&toolchain, &backend, &inputs).unwrap();

        assert_eq!(record.backend.name, "mock-backend");
        // Toolchain version should be in env
        assert_eq!(record.env.nargo_version, Some("0.38.0-test".to_string()));
    }

    #[test]
    fn test_prove_only_serializes_to_json() {
        let toolchain = create_mock_toolchain();
        let backend = create_mock_backend();
        let inputs = ProveInputs::new("/tmp/test-artifact.json", "test-circuit");

        let record = prove_only(&toolchain, &backend, &inputs).unwrap();

        // Serialize to JSON
        let json = serde_json::to_string(&record);
        assert!(json.is_ok());

        // Deserialize back
        let json_str = json.unwrap();
        let deserialized: Result<BenchRecord, _> = serde_json::from_str(&json_str);
        assert!(deserialized.is_ok());

        let record2 = deserialized.unwrap();
        assert_eq!(record.circuit_name, record2.circuit_name);
        assert_eq!(record.backend.name, record2.backend.name);
    }

    #[test]
    fn test_prove_inputs_builder() {
        let inputs = ProveInputs::new("/path/to/artifact.json", "my-circuit")
            .with_prover_toml("/path/to/Prover.toml")
            .with_timeout(Duration::from_secs(60));

        assert_eq!(
            inputs.artifact_path,
            PathBuf::from("/path/to/artifact.json")
        );
        assert_eq!(
            inputs.prover_toml,
            Some(PathBuf::from("/path/to/Prover.toml"))
        );
        assert_eq!(inputs.circuit_name, "my-circuit");
        assert_eq!(inputs.timeout, Duration::from_secs(60));
    }

    #[test]
    fn test_prove_with_iterations_multiple_samples() {
        let toolchain = create_mock_toolchain();
        let backend = create_mock_backend();
        let inputs = ProveInputs::new("/tmp/test-artifact.json", "test-circuit");

        let result = prove_with_iterations(&toolchain, &backend, &inputs, 1, 3);
        assert!(result.is_ok());

        let record = result.unwrap();

        // Config should reflect iterations
        assert_eq!(record.config.warmup_iterations, 1);
        assert_eq!(record.config.measured_iterations, 3);

        // Should have 3 measured samples
        let prove_stats = record.prove_stats.unwrap();
        assert_eq!(prove_stats.iterations, 3);
    }

    #[test]
    fn test_prove_with_iterations_zero_fails() {
        let toolchain = create_mock_toolchain();
        let backend = create_mock_backend();
        let inputs = ProveInputs::new("/tmp/test-artifact.json", "test-circuit");

        let result = prove_with_iterations(&toolchain, &backend, &inputs, 0, 0);
        assert!(result.is_err());
    }
}
