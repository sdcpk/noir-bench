//! Bench command implementations using the engine workflow.
//!
//! This module provides the CLI interface for config-driven benchmarking.
//! It uses `crate::engine::workflow` for the actual proving pipeline.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::time::Duration;

use serde_json::json;

use crate::backend::{BarretenbergBackend, BarretenbergConfig};
use crate::engine::{NargoToolchain, ProveInputs, full_benchmark};
use crate::{BenchError, BenchResult};

use super::backend::EvmBackend;
use super::config::{CircuitSpec, list_circuits_in_config, load_bench_config};

const DEFAULT_CONFIG: &str = "bench-config.toml";
const DEFAULT_JSONL: &str = "out/bench.jsonl";
const DEFAULT_CSV: &str = "out/bench.csv";

fn now_string() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "".to_string())
}

/// List circuits from bench config.
pub fn list(config: Option<PathBuf>) -> BenchResult<()> {
    let cfg_path = config.unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG));
    let entries = list_circuits_in_config(&cfg_path)?;
    for (name, path, params) in entries {
        if let Some(ps) = params {
            println!("{} => {} params={:?}", name, path.display(), ps);
        } else {
            println!("{} => {}", name, path.display());
        }
    }
    Ok(())
}

fn find_circuit(specs: &[CircuitSpec], name: &str, params: Option<u64>) -> Option<CircuitSpec> {
    specs
        .iter()
        .cloned()
        .find(|c| c.name == name && c.params == params)
        .or_else(|| {
            if params.is_none() {
                specs.iter().cloned().find(|c| c.name == name)
            } else {
                None
            }
        })
}

fn open_jsonl(path: &PathBuf) -> BenchResult<File> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    let f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| BenchError::Message(e.to_string()))?;
    Ok(f)
}

/// Find Prover.toml for a circuit spec.
fn find_prover_toml(spec: &CircuitSpec) -> Option<PathBuf> {
    // Try alongside artifact with .toml extension
    let mut p = spec.path.clone();
    p.set_extension("toml");
    if p.exists() {
        return Some(p);
    }
    // Try parent of target/
    spec.path
        .parent()
        .and_then(|dir| dir.parent().map(|pp| pp.join("Prover.toml")))
        .filter(|cand| cand.exists())
}

/// Run benchmark for a single circuit using engine workflow.
pub fn run(
    circuit_name: String,
    backend_name: Option<String>,
    params: Option<u64>,
    config: Option<PathBuf>,
    csv_out: Option<PathBuf>,
    jsonl_out: Option<PathBuf>,
    iterations: Option<usize>,
    warmup: Option<usize>,
) -> BenchResult<()> {
    let cfg_path = config.unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG));
    let specs = load_bench_config(&cfg_path)?;
    let Some(spec) = find_circuit(&specs, &circuit_name, params) else {
        return Err(BenchError::Message("circuit not found".into()));
    };
    let backend_s = backend_name.unwrap_or_else(|| "bb".to_string());
    let mut csv_logger = crate::logging::csv_logger::CsvLogger::new(
        csv_out.unwrap_or_else(|| PathBuf::from(DEFAULT_CSV)),
    );
    let jsonl_path = jsonl_out.unwrap_or_else(|| PathBuf::from(DEFAULT_JSONL));
    let mut jsonl = open_jsonl(&jsonl_path)?;
    let timestamp = now_string();
    let iter_n = iterations.unwrap_or(1);
    let warmup_n = warmup.unwrap_or(0);

    match backend_s.as_str() {
        "bb" | "barretenberg" => {
            // Create toolchain and backend
            let toolchain = NargoToolchain::new();
            let bb_config =
                BarretenbergConfig::new("bb").with_timeout(Duration::from_secs(24 * 60 * 60));
            let backend = BarretenbergBackend::new(bb_config);

            // Prepare inputs
            let prover_toml = find_prover_toml(&spec);
            let mut inputs = ProveInputs::new(&spec.path, &spec.name)
                .with_timeout(Duration::from_secs(24 * 60 * 60));
            if let Some(pt) = prover_toml {
                inputs = inputs.with_prover_toml(pt);
            }

            // Run full benchmark workflow
            let result = full_benchmark(&toolchain, &backend, &inputs, warmup_n, iter_n)?;

            // Extract metrics for legacy JSONL format
            let compile_ms = 0u128; // Compile is implicit in artifact loading
            let constraints = result.constraints;
            let acir_opcodes = result.acir_opcodes;
            let acir_bytes = result.record.artifact_size_bytes;
            let prove_ms_avg = result
                .record
                .prove_stats
                .as_ref()
                .map(|s| s.mean_ms)
                .unwrap_or(0.0);
            let memory_bytes = result
                .record
                .peak_rss_mb
                .map(|mb| (mb * 1024.0 * 1024.0) as u64);
            let proof_size = result.record.proof_size_bytes;
            let verify_success = result.verify_success;

            // Get iteration stats
            let prove_stats = result.record.prove_stats.as_ref();
            let iterations_obj = json!({
                "iterations": prove_stats.map(|s| s.iterations).unwrap_or(0),
                "warmup": warmup_n,
                "avg_ms": prove_stats.map(|s| s.mean_ms),
                "min_ms": prove_stats.map(|s| s.min_ms),
                "max_ms": prove_stats.map(|s| s.max_ms),
                "stddev_ms": prove_stats.and_then(|s| s.stddev_ms)
            });

            // Write legacy JSONL format
            let rec = json!({
                "timestamp": timestamp,
                "circuit": spec.name,
                "params": spec.params,
                "backend": "barretenberg",
                "compile_ms": compile_ms,
                "constraints": constraints,
                "acir_opcodes": acir_opcodes,
                "acir_bytes": acir_bytes,
                "prove_ms": prove_ms_avg,
                "memory_bytes": memory_bytes,
                "proof_size": proof_size,
                "evm_gas": serde_json::Value::Null,
                "status": verify_success,
                "iterations": iterations_obj
            });
            let _ = writeln!(jsonl, "{}", serde_json::to_string(&rec).unwrap());

            // Write CSV
            csv_logger.append_row(
                &timestamp,
                &spec.name,
                spec.params,
                "barretenberg",
                Some(compile_ms),
                Some(prove_ms_avg as u128),
                memory_bytes.map(|b| b / (1024 * 1024)),
                constraints,
                acir_opcodes,
                acir_bytes,
                proof_size,
                None,
                if verify_success { "ok" } else { "fail" },
            )?;

            println!(
                "bench run: {} backend=barretenberg prove_ms_avg={:.2} verify_ok={}",
                spec.name, prove_ms_avg, verify_success
            );
        }
        "evm" => {
            let evm = EvmBackend::new(&spec.path);
            let verify = evm.verify()?;

            // JSONL
            let rec = json!({
                "timestamp": timestamp,
                "circuit": spec.name,
                "params": spec.params,
                "backend": "evm",
                "compile_ms": serde_json::Value::Null,
                "constraints": serde_json::Value::Null,
                "acir_opcodes": serde_json::Value::Null,
                "prove_ms": serde_json::Value::Null,
                "memory_bytes": serde_json::Value::Null,
                "proof_size": serde_json::Value::Null,
                "evm_gas": verify.gas_used,
                "status": verify.success,
            });
            let _ = writeln!(jsonl, "{}", serde_json::to_string(&rec).unwrap());

            // CSV
            csv_logger.append_row(
                &timestamp,
                &spec.name,
                spec.params,
                "evm",
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                verify.gas_used,
                if verify.success { "ok" } else { "fail" },
            )?;
            println!(
                "bench run: {} backend=evm gas={:?}",
                spec.name, verify.gas_used
            );
        }
        other => {
            return Err(BenchError::Message(format!("unknown backend '{}'", other)));
        }
    }
    Ok(())
}

/// Run benchmark for all circuits in config.
pub fn run_all(
    backend_name: Option<String>,
    config: Option<PathBuf>,
    csv_out: Option<PathBuf>,
    jsonl_out: Option<PathBuf>,
    iterations: Option<usize>,
    warmup: Option<usize>,
) -> BenchResult<()> {
    let cfg_path = config.unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG));
    let specs = load_bench_config(&cfg_path)?;
    let backend_s = backend_name.unwrap_or_else(|| "bb".to_string());
    let mut csv_logger = crate::logging::csv_logger::CsvLogger::new(
        csv_out.unwrap_or_else(|| PathBuf::from(DEFAULT_CSV)),
    );
    let jsonl_path = jsonl_out.unwrap_or_else(|| PathBuf::from(DEFAULT_JSONL));
    let mut jsonl = open_jsonl(&jsonl_path)?;
    let iter_n = iterations.unwrap_or(1);
    let warmup_n = warmup.unwrap_or(0);

    // Create shared toolchain and backend for barretenberg
    let toolchain = NargoToolchain::new();
    let bb_config = BarretenbergConfig::new("bb").with_timeout(Duration::from_secs(24 * 60 * 60));
    let backend = BarretenbergBackend::new(bb_config);

    for spec in specs {
        let timestamp = now_string();
        match backend_s.as_str() {
            "bb" | "barretenberg" => {
                // Prepare inputs
                let prover_toml = find_prover_toml(&spec);
                let mut inputs = ProveInputs::new(&spec.path, &spec.name)
                    .with_timeout(Duration::from_secs(24 * 60 * 60));
                if let Some(pt) = prover_toml {
                    inputs = inputs.with_prover_toml(pt);
                }

                // Run full benchmark workflow
                let result = full_benchmark(&toolchain, &backend, &inputs, warmup_n, iter_n)?;

                // Extract metrics for legacy JSONL format
                let compile_ms = 0u128;
                let constraints = result.constraints;
                let acir_opcodes = result.acir_opcodes;
                let acir_bytes = result.record.artifact_size_bytes;
                let prove_ms_avg = result
                    .record
                    .prove_stats
                    .as_ref()
                    .map(|s| s.mean_ms)
                    .unwrap_or(0.0);
                let memory_bytes = result
                    .record
                    .peak_rss_mb
                    .map(|mb| (mb * 1024.0 * 1024.0) as u64);
                let proof_size = result.record.proof_size_bytes;
                let verify_success = result.verify_success;

                let rec = json!({
                    "timestamp": timestamp,
                    "circuit": spec.name,
                    "params": spec.params,
                    "backend": "barretenberg",
                    "compile_ms": compile_ms,
                    "constraints": constraints,
                    "acir_opcodes": acir_opcodes,
                    "acir_bytes": acir_bytes,
                    "prove_ms": prove_ms_avg,
                    "memory_bytes": memory_bytes,
                    "proof_size": proof_size,
                    "evm_gas": serde_json::Value::Null,
                    "status": verify_success,
                });
                let _ = writeln!(jsonl, "{}", serde_json::to_string(&rec).unwrap());

                csv_logger.append_row(
                    &timestamp,
                    &spec.name,
                    spec.params,
                    "barretenberg",
                    Some(compile_ms),
                    Some(prove_ms_avg as u128),
                    memory_bytes.map(|b| b / (1024 * 1024)),
                    constraints,
                    acir_opcodes,
                    acir_bytes,
                    proof_size,
                    None,
                    if verify_success { "ok" } else { "fail" },
                )?;
            }
            "evm" => {
                let evm = EvmBackend::new(&spec.path);
                let verify = evm.verify()?;

                let rec = json!({
                    "timestamp": timestamp,
                    "circuit": spec.name,
                    "params": spec.params,
                    "backend": "evm",
                    "compile_ms": serde_json::Value::Null,
                    "constraints": serde_json::Value::Null,
                    "acir_opcodes": serde_json::Value::Null,
                    "prove_ms": serde_json::Value::Null,
                    "memory_bytes": serde_json::Value::Null,
                    "proof_size": serde_json::Value::Null,
                    "evm_gas": verify.gas_used,
                    "status": verify.success,
                });
                let _ = writeln!(jsonl, "{}", serde_json::to_string(&rec).unwrap());

                csv_logger.append_row(
                    &timestamp,
                    &spec.name,
                    spec.params,
                    "evm",
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    verify.gas_used,
                    if verify.success { "ok" } else { "fail" },
                )?;
            }
            other => {
                return Err(BenchError::Message(format!("unknown backend '{}'", other)));
            }
        }
    }
    Ok(())
}

/// Export JSONL to CSV format.
pub fn export_csv(jsonl_path: Option<PathBuf>, csv_out: Option<PathBuf>) -> BenchResult<()> {
    let jsonl = jsonl_path.unwrap_or_else(|| PathBuf::from(DEFAULT_JSONL));
    let csvp = csv_out.unwrap_or_else(|| PathBuf::from(DEFAULT_CSV));
    if let Some(dir) = csvp.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    let reader =
        BufReader::new(File::open(&jsonl).map_err(|e| BenchError::Message(e.to_string()))?);
    let mut csv_w = crate::logging::csv_logger::CsvLogger::new(&csvp);

    for line in reader.lines() {
        let Ok(l) = line else { continue };
        let Ok(v): Result<serde_json::Value, _> = serde_json::from_str(&l) else {
            continue;
        };
        let timestamp = v
            .get("timestamp")
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string();
        let circuit = v
            .get("circuit")
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string();
        let params = v.get("params").and_then(|x| x.as_u64());
        let backend = v
            .get("backend")
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string();
        let compile_ms = v
            .get("compile_ms")
            .and_then(|x| x.as_u64())
            .map(|x| x as u128);
        // prove_ms may be float (avg) in JSONL now; support both number types
        let prove_ms = if let Some(u) = v.get("prove_ms").and_then(|x| x.as_u64()) {
            Some(u as u128)
        } else if let Some(f) = v.get("prove_ms").and_then(|x| x.as_f64()) {
            Some(f.round() as u128)
        } else {
            None
        };
        let memory_mb = v
            .get("memory_bytes")
            .and_then(|x| x.as_u64())
            .map(|b| b / (1024 * 1024));
        let constraints = v.get("constraints").and_then(|x| x.as_u64());
        let acir_opcodes = v.get("acir_opcodes").and_then(|x| x.as_u64());
        let proof_size = v.get("proof_size").and_then(|x| x.as_u64());
        let acir_bytes = v.get("acir_bytes").and_then(|x| x.as_u64());
        let evm_gas = v.get("evm_gas").and_then(|x| x.as_u64());
        let status = v
            .get("status")
            .map(|x| {
                if x.as_bool() == Some(true) {
                    "ok"
                } else {
                    "fail"
                }
            })
            .unwrap_or("unknown");

        csv_w.append_row(
            &timestamp,
            &circuit,
            params,
            &backend,
            compile_ms,
            prove_ms,
            memory_mb,
            constraints,
            acir_opcodes,
            acir_bytes,
            proof_size,
            evm_gas,
            status,
        )?;
    }
    Ok(())
}

/// Run EVM verification for a circuit.
pub fn evm_verify(
    circuit_name: String,
    config: Option<PathBuf>,
    csv_out: Option<PathBuf>,
) -> BenchResult<()> {
    let cfg_path = config.unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG));
    let specs = load_bench_config(&cfg_path)?;
    let Some(spec) = find_circuit(&specs, &circuit_name, None) else {
        return Err(BenchError::Message("circuit not found".into()));
    };
    let mut csv_logger = crate::logging::csv_logger::CsvLogger::new(
        csv_out.unwrap_or_else(|| PathBuf::from(DEFAULT_CSV)),
    );
    let evm = EvmBackend::new(&spec.path);
    let verify = evm.verify()?;
    let timestamp = now_string();

    csv_logger.append_row(
        &timestamp,
        &spec.name,
        spec.params,
        "evm",
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        verify.gas_used,
        if verify.success { "ok" } else { "fail" },
    )?;
    println!("bench evm-verify: {} gas={:?}", spec.name, verify.gas_used);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{GateInfo, MockBackend, MockConfig, ProveOutput, VerifyOutput};
    use crate::engine::toolchain::MockToolchain;
    use crate::engine::workflow::full_benchmark;
    use std::path::PathBuf;

    /// Test that bench uses engine workflow with mock backend/toolchain.
    #[test]
    fn test_bench_uses_engine_workflow() {
        // Create mock toolchain and backend
        let toolchain = MockToolchain::new().with_version("0.38.0-mock");
        let backend = MockBackend::new(
            MockConfig::new("mock-bb")
                .with_prove_output(ProveOutput {
                    prove_time_ms: 150,
                    witness_gen_time_ms: None,
                    backend_prove_time_ms: Some(150),
                    peak_memory_bytes: Some(50_000_000),
                    proof_size_bytes: Some(4096),
                    proving_key_size_bytes: Some(1_000_000),
                    verification_key_size_bytes: Some(512),
                    proof_path: Some(PathBuf::from("/mock/proof")),
                    vk_path: Some(PathBuf::from("/mock/vk")),
                })
                .with_verify_output(VerifyOutput {
                    verify_time_ms: 50,
                    success: true,
                })
                .with_gate_info(GateInfo {
                    backend_gates: 10000,
                    subgroup_size: Some(16384),
                    acir_opcodes: Some(100),
                    per_opcode: None,
                }),
        );

        let inputs = ProveInputs::new("/mock/artifact.json", "test-circuit");
        let result = full_benchmark(&toolchain, &backend, &inputs, 0, 1);

        assert!(
            result.is_ok(),
            "full_benchmark should succeed: {:?}",
            result.err()
        );

        let bench_result = result.unwrap();

        // Verify engine workflow returned expected data
        assert_eq!(bench_result.record.circuit_name, "test-circuit");
        assert_eq!(bench_result.record.backend.name, "mock-bb");
        assert_eq!(bench_result.constraints, Some(10000));
        assert_eq!(bench_result.acir_opcodes, Some(100));
        assert!(bench_result.verify_success);
        assert_eq!(bench_result.verify_time_ms, Some(50));

        // Verify timing stats
        let prove_stats = bench_result.record.prove_stats.as_ref().unwrap();
        assert_eq!(prove_stats.iterations, 1);
        assert_eq!(prove_stats.mean_ms, 150.0);

        // Verify size metrics
        assert_eq!(bench_result.record.proof_size_bytes, Some(4096));
    }

    #[test]
    fn test_bench_multiple_iterations() {
        let toolchain = MockToolchain::new();
        let backend = MockBackend::new(
            MockConfig::new("mock-bb")
                .with_prove_output(ProveOutput {
                    prove_time_ms: 100,
                    ..Default::default()
                })
                .with_verify_output(VerifyOutput {
                    verify_time_ms: 30,
                    success: true,
                })
                .with_gate_info(GateInfo::from_gates(5000)),
        );

        let inputs = ProveInputs::new("/mock/artifact.json", "iter-test");
        let result = full_benchmark(&toolchain, &backend, &inputs, 1, 3);

        assert!(result.is_ok());
        let bench_result = result.unwrap();

        // Should have 3 measured iterations
        let prove_stats = bench_result.record.prove_stats.unwrap();
        assert_eq!(prove_stats.iterations, 3);

        // Config should reflect warmup
        assert_eq!(bench_result.record.config.warmup_iterations, 1);
        assert_eq!(bench_result.record.config.measured_iterations, 3);
    }
}
