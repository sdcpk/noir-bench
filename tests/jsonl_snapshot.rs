use noir_bench::core::env::EnvironmentInfo;
use noir_bench::core::schema::{BackendInfo, BenchRecord, RunConfig, TimingStat};

fn make_fixed_record() -> BenchRecord {
    let env = EnvironmentInfo {
        cpu_model: Some("Test CPU".to_string()),
        cpu_cores: Some(8),
        total_ram_bytes: Some(17_179_869_184),
        os: "test-os".to_string(),
        hostname: Some("test-host".to_string()),
        git_sha: Some("deadbeef".to_string()),
        git_dirty: Some(false),
        nargo_version: Some("0.42.0".to_string()),
        bb_version: Some("1.0.0".to_string()),
    };

    let backend = BackendInfo {
        name: "mock-backend".to_string(),
        version: Some("1.2.3".to_string()),
        variant: Some("mock-variant".to_string()),
    };

    let config = RunConfig {
        warmup_iterations: 1,
        measured_iterations: 2,
        timeout_secs: Some(30),
    };

    BenchRecord {
        schema_version: 1,
        record_id: "test-record-1".to_string(),
        timestamp: "2026-01-15T00:00:00Z".to_string(),
        circuit_name: "test-circuit".to_string(),
        circuit_path: Some("path/to/circuit.json".to_string()),
        env,
        backend,
        config,
        compile_stats: Some(TimingStat {
            iterations: 2,
            mean_ms: 1.5,
            median_ms: Some(1.5),
            stddev_ms: Some(0.1),
            min_ms: 1.4,
            max_ms: 1.6,
            p95_ms: Some(1.6),
        }),
        witness_stats: Some(TimingStat {
            iterations: 2,
            mean_ms: 2.5,
            median_ms: Some(2.5),
            stddev_ms: Some(0.2),
            min_ms: 2.4,
            max_ms: 2.6,
            p95_ms: Some(2.6),
        }),
        prove_stats: Some(TimingStat {
            iterations: 2,
            mean_ms: 10.5,
            median_ms: Some(10.0),
            stddev_ms: Some(0.3),
            min_ms: 10.0,
            max_ms: 11.0,
            p95_ms: Some(11.0),
        }),
        verify_stats: Some(TimingStat {
            iterations: 1,
            mean_ms: 3.0,
            median_ms: Some(3.0),
            stddev_ms: Some(0.0),
            min_ms: 3.0,
            max_ms: 3.0,
            p95_ms: Some(3.0),
        }),
        proof_size_bytes: Some(2048),
        proving_key_size_bytes: Some(4096),
        verification_key_size_bytes: Some(1024),
        artifact_size_bytes: Some(512),
        total_gates: Some(12_345),
        acir_opcodes: Some(234),
        subgroup_size: Some(16_384),
        peak_rss_mb: Some(12.34),
        cli_args: vec!["noir-bench".to_string(), "prove".to_string()],
    }
}

#[test]
fn test_bench_record_json_snapshot() {
    let record = make_fixed_record();
    let actual = serde_json::to_string(&record).expect("serialization should succeed");
    let expected = include_str!("fixtures/bench_record_v1.json").trim_end();
    assert_eq!(actual, expected);
}
