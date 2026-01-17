//! Integration tests for engine workflow module.
//!
//! These tests verify the workflow orchestration using mock toolchain and backend.
//! No actual nargo or bb binaries are required.

use std::path::PathBuf;
use std::time::Duration;

use noir_bench::backend::{MockBackend, MockConfig, ProveOutput};
use noir_bench::core::{BenchRecord, TimingStat};
use noir_bench::engine::toolchain::{CompileArtifacts, MockToolchain, WitnessArtifact};
use noir_bench::engine::workflow::{ProveInputs, prove_only, prove_with_iterations};

/// Create a mock toolchain with deterministic outputs.
fn create_test_toolchain() -> MockToolchain {
    MockToolchain {
        mock_name: "test-nargo",
        mock_version: "0.42.0-test".to_string(),
        compile_output: Some(CompileArtifacts {
            artifact_path: PathBuf::from("/mock/artifact.json"),
            compile_time_ms: 100,
        }),
        witness_output: Some(WitnessArtifact {
            witness_path: PathBuf::from("/mock/witness.gz"),
            witness_gen_time_ms: 50,
        }),
        should_fail: false,
    }
}

/// Create a mock backend with deterministic outputs.
fn create_test_backend() -> MockBackend {
    MockBackend::new(
        MockConfig::new("test-backend").with_prove_output(ProveOutput {
            prove_time_ms: 200,
            witness_gen_time_ms: None,
            backend_prove_time_ms: Some(200),
            peak_memory_bytes: Some(100_000_000), // 100 MB
            proof_size_bytes: Some(4096),
            proving_key_size_bytes: Some(2_000_000),
            verification_key_size_bytes: Some(1024),
            proof_path: None,
            vk_path: None,
        }),
    )
}

#[test]
fn test_prove_only_returns_valid_bench_record() {
    let toolchain = create_test_toolchain();
    let backend = create_test_backend();
    let inputs = ProveInputs::new("/mock/circuit.json", "test-circuit");

    let result = prove_only(&toolchain, &backend, &inputs);
    assert!(
        result.is_ok(),
        "prove_only should succeed: {:?}",
        result.err()
    );

    let record = result.unwrap();

    // Verify required BenchRecord v1 fields
    assert_eq!(record.schema_version, 1, "Schema version should be 1");
    assert!(
        !record.record_id.is_empty(),
        "Record ID should not be empty"
    );
    assert!(
        !record.timestamp.is_empty(),
        "Timestamp should not be empty"
    );
    assert_eq!(record.circuit_name, "test-circuit");
}

#[test]
fn test_prove_only_populates_backend_info() {
    let toolchain = create_test_toolchain();
    let backend = create_test_backend();
    let inputs = ProveInputs::new("/mock/circuit.json", "test-circuit");

    let record = prove_only(&toolchain, &backend, &inputs).unwrap();

    assert_eq!(record.backend.name, "test-backend");
    assert!(record.backend.version.is_some());
}

#[test]
fn test_prove_only_populates_toolchain_version() {
    let toolchain = create_test_toolchain();
    let backend = create_test_backend();
    let inputs = ProveInputs::new("/mock/circuit.json", "test-circuit");

    let record = prove_only(&toolchain, &backend, &inputs).unwrap();

    // Toolchain version should be stored in env.nargo_version
    assert_eq!(record.env.nargo_version, Some("0.42.0-test".to_string()));
}

#[test]
fn test_prove_only_sets_timing_stats() {
    let toolchain = create_test_toolchain();
    let backend = create_test_backend();
    let inputs = ProveInputs::new("/mock/circuit.json", "test-circuit");

    let record = prove_only(&toolchain, &backend, &inputs).unwrap();

    // Witness stats
    assert!(
        record.witness_stats.is_some(),
        "witness_stats should be set"
    );
    let witness_stats = record.witness_stats.unwrap();
    assert_eq!(witness_stats.iterations, 1);
    assert!(witness_stats.mean_ms > 0.0, "witness mean_ms should be > 0");

    // Prove stats
    assert!(record.prove_stats.is_some(), "prove_stats should be set");
    let prove_stats = record.prove_stats.unwrap();
    assert_eq!(prove_stats.iterations, 1);
    assert_eq!(
        prove_stats.mean_ms, 200.0,
        "prove mean_ms should match backend output"
    );
}

#[test]
fn test_prove_only_sets_size_metrics() {
    let toolchain = create_test_toolchain();
    let backend = create_test_backend();
    let inputs = ProveInputs::new("/mock/circuit.json", "test-circuit");

    let record = prove_only(&toolchain, &backend, &inputs).unwrap();

    assert_eq!(record.proof_size_bytes, Some(4096));
    assert_eq!(record.proving_key_size_bytes, Some(2_000_000));
    assert_eq!(record.verification_key_size_bytes, Some(1024));
}

#[test]
fn test_prove_only_sets_memory_metrics() {
    let toolchain = create_test_toolchain();
    let backend = create_test_backend();
    let inputs = ProveInputs::new("/mock/circuit.json", "test-circuit");

    let record = prove_only(&toolchain, &backend, &inputs).unwrap();

    // 100_000_000 bytes = ~95.37 MB
    assert!(record.peak_rss_mb.is_some());
    let peak_mb = record.peak_rss_mb.unwrap();
    assert!(
        (peak_mb - 95.367).abs() < 0.1,
        "peak_rss_mb should be ~95.37 MB, got {}",
        peak_mb
    );
}

#[test]
fn test_prove_only_json_serialization_roundtrips() {
    let toolchain = create_test_toolchain();
    let backend = create_test_backend();
    let inputs = ProveInputs::new("/mock/circuit.json", "serialize-test");

    let record = prove_only(&toolchain, &backend, &inputs).unwrap();

    // Serialize to JSON
    let json_str = serde_json::to_string(&record).expect("serialization should succeed");
    assert!(!json_str.is_empty());

    // Deserialize back
    let deserialized: BenchRecord =
        serde_json::from_str(&json_str).expect("deserialization should succeed");

    // Verify key fields match
    assert_eq!(record.schema_version, deserialized.schema_version);
    assert_eq!(record.circuit_name, deserialized.circuit_name);
    assert_eq!(record.backend.name, deserialized.backend.name);
    assert_eq!(record.proof_size_bytes, deserialized.proof_size_bytes);

    // Verify timing stats roundtrip
    assert_eq!(
        record.prove_stats.as_ref().map(|s| s.mean_ms),
        deserialized.prove_stats.as_ref().map(|s| s.mean_ms)
    );
}

#[test]
fn test_prove_only_handles_toolchain_failure() {
    let toolchain = MockToolchain::new().failing();
    let backend = create_test_backend();
    let inputs = ProveInputs::new("/mock/circuit.json", "fail-test");

    let result = prove_only(&toolchain, &backend, &inputs);
    assert!(result.is_err(), "Should fail when toolchain fails");
}

#[test]
fn test_prove_only_handles_backend_failure() {
    let toolchain = create_test_toolchain();
    let backend = MockBackend::new(MockConfig::new("failing-backend").prove_fails());
    let inputs = ProveInputs::new("/mock/circuit.json", "fail-test");

    let result = prove_only(&toolchain, &backend, &inputs);
    assert!(result.is_err(), "Should fail when backend fails");
}

#[test]
fn test_prove_inputs_builder() {
    let inputs = ProveInputs::new("/path/to/artifact.json", "my-circuit")
        .with_prover_toml("/path/to/Prover.toml")
        .with_timeout(Duration::from_secs(120));

    assert_eq!(
        inputs.artifact_path,
        PathBuf::from("/path/to/artifact.json")
    );
    assert_eq!(
        inputs.prover_toml,
        Some(PathBuf::from("/path/to/Prover.toml"))
    );
    assert_eq!(inputs.circuit_name, "my-circuit");
    assert_eq!(inputs.timeout, Duration::from_secs(120));
}

#[test]
fn test_prove_with_iterations_collects_multiple_samples() {
    let toolchain = create_test_toolchain();
    let backend = create_test_backend();
    let inputs = ProveInputs::new("/mock/circuit.json", "iter-test");

    let result = prove_with_iterations(&toolchain, &backend, &inputs, 1, 3);
    assert!(result.is_ok());

    let record = result.unwrap();

    // Config should reflect iterations
    assert_eq!(record.config.warmup_iterations, 1);
    assert_eq!(record.config.measured_iterations, 3);

    // Timing stats should have 3 samples
    let prove_stats = record.prove_stats.unwrap();
    assert_eq!(prove_stats.iterations, 3);

    let witness_stats = record.witness_stats.unwrap();
    assert_eq!(witness_stats.iterations, 3);
}

#[test]
fn test_prove_with_iterations_zero_iterations_fails() {
    let toolchain = create_test_toolchain();
    let backend = create_test_backend();
    let inputs = ProveInputs::new("/mock/circuit.json", "zero-test");

    let result = prove_with_iterations(&toolchain, &backend, &inputs, 0, 0);
    assert!(result.is_err(), "Should fail with 0 iterations");
}

#[test]
fn test_timing_stat_calculations() {
    // Verify TimingStat::from_samples works correctly
    let samples = vec![100.0, 110.0, 120.0];
    let stat = TimingStat::from_samples(&samples);

    assert_eq!(stat.iterations, 3);
    assert!((stat.mean_ms - 110.0).abs() < 0.001);
    assert_eq!(stat.min_ms, 100.0);
    assert_eq!(stat.max_ms, 120.0);
    assert!(stat.median_ms.is_some());
    assert!(stat.stddev_ms.is_some());
}

#[test]
fn test_bench_record_schema_version_is_constant() {
    let toolchain = create_test_toolchain();
    let backend = create_test_backend();

    // Run multiple times to verify schema version is always 1
    for i in 0..3 {
        let inputs = ProveInputs::new("/mock/circuit.json", format!("schema-test-{}", i));
        let record = prove_only(&toolchain, &backend, &inputs).unwrap();
        assert_eq!(
            record.schema_version, 1,
            "Schema version should always be 1"
        );
    }
}

#[test]
fn test_circuit_path_is_set() {
    let toolchain = create_test_toolchain();
    let backend = create_test_backend();
    let inputs = ProveInputs::new("/mock/my-circuit.json", "path-test");

    let record = prove_only(&toolchain, &backend, &inputs).unwrap();

    assert!(record.circuit_path.is_some());
    assert_eq!(record.circuit_path.unwrap(), "/mock/my-circuit.json");
}
