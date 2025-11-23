use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

use serde_json::json;

use crate::{BenchError, BenchResult};

use super::backend::{Backend, BarretenbergBackend, EvmBackend};
use super::config::{CircuitSpec, load_bench_config, list_circuits_in_config};

const DEFAULT_CONFIG: &str = "bench-config.toml";
const DEFAULT_JSONL: &str = "out/bench.jsonl";
const DEFAULT_CSV: &str = "out/bench.csv";

fn now_string() -> String {
    time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339).unwrap_or_else(|_| "".to_string())
}

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
    specs.iter().cloned().find(|c| c.name == name && c.params == params)
        .or_else(|| {
            if params.is_none() {
                specs.iter().cloned().find(|c| c.name == name)
            } else {
                None
            }
        })
}

fn open_jsonl(path: &PathBuf) -> BenchResult<File> {
    if let Some(dir) = path.parent() { std::fs::create_dir_all(dir).ok(); }
    let f = OpenOptions::new().create(true).append(true).open(path).map_err(|e| BenchError::Message(e.to_string()))?;
    Ok(f)
}

pub fn run(
    circuit_name: String,
    backend_name: Option<String>,
    params: Option<u64>,
    config: Option<PathBuf>,
    csv_out: Option<PathBuf>,
    jsonl_out: Option<PathBuf>,
) -> BenchResult<()> {
    let cfg_path = config.unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG));
    let specs = load_bench_config(&cfg_path)?;
    let Some(spec) = find_circuit(&specs, &circuit_name, params) else { return Err(BenchError::Message("circuit not found".into())); };
    let backend_s = backend_name.unwrap_or_else(|| "bb".to_string());
    let mut csv_logger = crate::logging::csv_logger::CsvLogger::new(csv_out.clone().unwrap_or_else(|| PathBuf::from(DEFAULT_CSV)));
    let jsonl_path = jsonl_out.unwrap_or_else(|| PathBuf::from(DEFAULT_JSONL));
    let mut jsonl = open_jsonl(&jsonl_path)?;
    let timestamp = now_string();

    match backend_s.as_str() {
        "bb" | "barretenberg" => {
            let bb = BarretenbergBackend { bb_path: PathBuf::from("bb"), extra_args: vec![] };
            // compile
            let compile = bb.compile(&spec)?;
            // prove
            let proof = bb.prove(&spec)?;
            // verify
            let verify = bb.verify(&proof)?;
            // JSONL
            let rec = json!({
                "timestamp": timestamp,
                "circuit": spec.name,
                "params": spec.params,
                "backend": "barretenberg",
                "compile_ms": compile.compile_time_ms,
                "constraints": compile.constraints,
                "prove_ms": proof.prove_time_ms,
                "memory_bytes": proof.peak_memory_bytes,
                "proof_size": proof.proof_size_bytes,
                "evm_gas": serde_json::Value::Null,
                "status": verify.success,
            });
            let _ = writeln!(jsonl, "{}", serde_json::to_string(&rec).unwrap());
            // CSV
            csv_logger.append_row(
                &timestamp,
                &spec.name,
                spec.params,
                "barretenberg",
                Some(compile.compile_time_ms),
                Some(proof.prove_time_ms),
                proof.peak_memory_bytes.map(|b| b / (1024 * 1024)),
                compile.constraints,
                proof.proof_size_bytes,
                None,
                if verify.success { "ok" } else { "fail" },
            )?;
            println!("bench run: {} backend=barretenberg prove_ms={} verify_ok={}", spec.name, proof.prove_time_ms, verify.success);
        }
        "evm" => {
            let evm = EvmBackend { foundry_dir: spec.path.clone(), forge_bin: None, test_pattern: None, gas_per_second: Some(1_250_000) };
            let verify = evm.verify(&super::backend::ProofOutput {
                prove_time_ms: 0,
                backend_prove_time_ms: None,
                witness_gen_time_ms: None,
                peak_memory_bytes: None,
                proof_size_bytes: None,
                proof_path: None,
            })?;
            // JSONL
            let rec = json!({
                "timestamp": timestamp,
                "circuit": spec.name,
                "params": spec.params,
                "backend": "evm",
                "compile_ms": serde_json::Value::Null,
                "constraints": serde_json::Value::Null,
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
                verify.gas_used,
                if verify.success { "ok" } else { "fail" },
            )?;
            println!("bench run: {} backend=evm gas={:?}", spec.name, verify.gas_used);
        }
        other => {
            return Err(BenchError::Message(format!("unknown backend '{}'", other)));
        }
    }
    Ok(())
}

fn csv_logger_path(csv_out: Option<PathBuf>) -> PathBuf {
    csv_out.unwrap_or_else(|| PathBuf::from(DEFAULT_CSV))
}

pub fn run_all(
    backend_name: Option<String>,
    config: Option<PathBuf>,
    csv_out: Option<PathBuf>,
    jsonl_out: Option<PathBuf>,
) -> BenchResult<()> {
    let cfg_path = config.unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG));
    let specs = load_bench_config(&cfg_path)?;
    let backend_s = backend_name.unwrap_or_else(|| "bb".to_string());
    let mut csv_logger = crate::logging::csv_logger::CsvLogger::new(csv_out.unwrap_or_else(|| PathBuf::from(DEFAULT_CSV)));
    let jsonl_path = jsonl_out.unwrap_or_else(|| PathBuf::from(DEFAULT_JSONL));
    let mut jsonl = open_jsonl(&jsonl_path)?;

    for spec in specs {
        let timestamp = now_string();
        match backend_s.as_str() {
            "bb" | "barretenberg" => {
                let bb = BarretenbergBackend { bb_path: PathBuf::from("bb"), extra_args: vec![] };
                let compile = bb.compile(&spec)?;
                let proof = bb.prove(&spec)?;
                let verify = bb.verify(&proof)?;
                let rec = json!({
                    "timestamp": timestamp,
                    "circuit": spec.name,
                    "params": spec.params,
                    "backend": "barretenberg",
                    "compile_ms": compile.compile_time_ms,
                    "constraints": compile.constraints,
                    "prove_ms": proof.prove_time_ms,
                    "memory_bytes": proof.peak_memory_bytes,
                    "proof_size": proof.proof_size_bytes,
                    "evm_gas": serde_json::Value::Null,
                    "status": verify.success,
                });
                let _ = writeln!(jsonl, "{}", serde_json::to_string(&rec).unwrap());
                csv_logger.append_row(
                    &timestamp,
                    &spec.name,
                    spec.params,
                    "barretenberg",
                    Some(compile.compile_time_ms),
                    Some(proof.prove_time_ms),
                    proof.peak_memory_bytes.map(|b| b / (1024 * 1024)),
                    compile.constraints,
                    proof.proof_size_bytes,
                    None,
                    if verify.success { "ok" } else { "fail" },
                )?;
            }
            "evm" => {
                let evm = EvmBackend { foundry_dir: spec.path.clone(), forge_bin: None, test_pattern: None, gas_per_second: Some(1_250_000) };
                let verify = evm.verify(&super::backend::ProofOutput {
                    prove_time_ms: 0,
                    backend_prove_time_ms: None,
                    witness_gen_time_ms: None,
                    peak_memory_bytes: None,
                    proof_size_bytes: None,
                    proof_path: None,
                })?;
                let rec = json!({
                    "timestamp": timestamp,
                    "circuit": spec.name,
                    "params": spec.params,
                    "backend": "evm",
                    "compile_ms": serde_json::Value::Null,
                    "constraints": serde_json::Value::Null,
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

pub fn export_csv(jsonl_path: Option<PathBuf>, csv_out: Option<PathBuf>) -> BenchResult<()> {
    let jsonl = jsonl_path.unwrap_or_else(|| PathBuf::from(DEFAULT_JSONL));
    let csvp = csv_out.unwrap_or_else(|| PathBuf::from(DEFAULT_CSV));
    if let Some(dir) = csvp.parent() { std::fs::create_dir_all(dir).ok(); }
    let reader = BufReader::new(File::open(&jsonl).map_err(|e| BenchError::Message(e.to_string()))?);
    let mut csv_w = crate::logging::csv_logger::CsvLogger::new(&csvp);
    for line in reader.lines() {
        let Ok(l) = line else { continue; };
        let Ok(v): Result<serde_json::Value, _> = serde_json::from_str(&l) else { continue; };
        let timestamp = v.get("timestamp").and_then(|x| x.as_str()).unwrap_or_default().to_string();
        let circuit = v.get("circuit").and_then(|x| x.as_str()).unwrap_or_default().to_string();
        let params = v.get("params").and_then(|x| x.as_u64());
        let backend = v.get("backend").and_then(|x| x.as_str()).unwrap_or_default().to_string();
        let compile_ms = v.get("compile_ms").and_then(|x| x.as_u64()).map(|x| x as u128);
        let prove_ms = v.get("prove_ms").and_then(|x| x.as_u64()).map(|x| x as u128);
        let memory_mb = v.get("memory_bytes").and_then(|x| x.as_u64()).map(|b| b / (1024 * 1024));
        let constraints = v.get("constraints").and_then(|x| x.as_u64());
        let proof_size = v.get("proof_size").and_then(|x| x.as_u64());
        let evm_gas = v.get("evm_gas").and_then(|x| x.as_u64());
        let status = v.get("status").map(|x| {
            if x.as_bool() == Some(true) { "ok" } else { "fail" }
        }).unwrap_or("unknown");
        csv_w.append_row(
            &timestamp,
            &circuit,
            params,
            &backend,
            compile_ms,
            prove_ms,
            memory_mb,
            constraints,
            proof_size,
            evm_gas,
            status,
        )?;
    }
    Ok(())
}

pub fn evm_verify(circuit_name: String, config: Option<PathBuf>, csv_out: Option<PathBuf>) -> BenchResult<()> {
    let cfg_path = config.unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG));
    let specs = load_bench_config(&cfg_path)?;
    let Some(spec) = find_circuit(&specs, &circuit_name, None) else { return Err(BenchError::Message("circuit not found".into())); };
    let mut csv_logger = crate::logging::csv_logger::CsvLogger::new(csv_out.unwrap_or_else(|| PathBuf::from(DEFAULT_CSV)));
    let evm = EvmBackend { foundry_dir: spec.path.clone(), forge_bin: None, test_pattern: None, gas_per_second: Some(1_250_000) };
    let verify = evm.verify(&super::backend::ProofOutput {
        prove_time_ms: 0,
        backend_prove_time_ms: None,
        witness_gen_time_ms: None,
        peak_memory_bytes: None,
        proof_size_bytes: None,
        proof_path: None,
    })?;
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
        verify.gas_used,
        if verify.success { "ok" } else { "fail" },
    )?;
    println!("bench evm-verify: {} gas={:?}", spec.name, verify.gas_used);
    Ok(())
}


