pub mod exec_cmd;
pub mod gates_cmd;
pub mod prove_cmd;
pub mod verify_cmd;
pub mod suite_cmd;
pub mod compare_cmd;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BenchError {
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),
}

pub type BenchResult<T> = Result<T, BenchError>;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SystemInfo {
    pub cpu_model: Option<String>,
    pub cpu_cores_logical: Option<usize>,
    pub cpu_cores_physical: Option<usize>,
    pub total_ram_bytes: Option<u64>,
    pub os: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IterationStats {
    pub iterations: usize,
    pub warmup: usize,
    pub times_ms: Vec<u128>,
    pub avg_ms: Option<f64>,
    pub min_ms: Option<u128>,
    pub max_ms: Option<u128>,
    pub stddev_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommonMeta {
    pub name: String,
    pub timestamp: String,
    pub noir_version: String,
    pub artifact_path: PathBuf,
    pub cli_args: Vec<String>,
    pub artifact_sha256: Option<String>,
    pub inputs_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecReport {
    #[serde(flatten)]
    pub meta: CommonMeta,
    pub execution_time_ms: u128,
    pub samples_count: usize,
    pub peak_memory_bytes: Option<u64>,
    pub flamegraph_svg: Option<PathBuf>,
    pub system: Option<SystemInfo>,
    pub iterations: Option<IterationStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendInfo {
    pub name: String,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProveReport {
    #[serde(flatten)]
    pub meta: CommonMeta,
    pub prove_time_ms: u128,
    pub witness_gen_time_ms: Option<u128>,
    pub backend_prove_time_ms: Option<u128>,
    pub peak_memory_bytes: Option<u64>,
    pub proof_size_bytes: Option<u64>,
    pub gate_count: Option<u64>,
    pub backend: BackendInfo,
    pub system: Option<SystemInfo>,
    pub iterations: Option<IterationStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatesOpcodeBreakdown {
    pub index: usize,
    pub opcode: String,
    pub gates: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatesReport {
    #[serde(flatten)]
    pub meta: CommonMeta,
    pub total_gates: usize,
    pub acir_opcodes: usize,
    pub per_opcode: Vec<GatesOpcodeBreakdown>,
    pub per_opcode_percent: Option<Vec<(String, f64)>>,
    pub backend: BackendInfo,
    pub system: Option<SystemInfo>,
} 

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyReport {
    #[serde(flatten)]
    pub meta: CommonMeta,
    pub verify_time_ms: u128,
    pub ok: bool,
    pub backend: BackendInfo,
    pub system: Option<SystemInfo>,
    pub iterations: Option<IterationStats>,
}

// Shared helpers
pub fn collect_system_info() -> SystemInfo {
    use sysinfo::System;
    let mut sys = System::new_all();
    sys.refresh_all();
    let cpu_model = sys.cpus().get(0).map(|c| c.brand().to_string());
    let cpu_cores_logical = Some(sys.cpus().len());
    let cpu_cores_physical = sys.physical_core_count();
    let total_ram_bytes = Some(sys.total_memory());
    let os = System::name();
    SystemInfo { cpu_model, cpu_cores_logical, cpu_cores_physical, total_ram_bytes, os }
}

pub fn compute_iteration_stats(times_ms: Vec<u128>, iterations: usize, warmup: usize) -> IterationStats {
    if times_ms.is_empty() {
        return IterationStats { iterations, warmup, times_ms, avg_ms: None, min_ms: None, max_ms: None, stddev_ms: None };
    }
    let len = times_ms.len() as f64;
    let sum: f64 = times_ms.iter().map(|v| *v as f64).sum();
    let avg = sum / len;
    let min = *times_ms.iter().min().unwrap();
    let max = *times_ms.iter().max().unwrap();
    let var = times_ms.iter().map(|v| {
        let d = *v as f64 - avg;
        d * d
    }).sum::<f64>() / len;
    let stddev = var.sqrt();
    IterationStats { iterations, warmup, times_ms, avg_ms: Some(avg), min_ms: Some(min), max_ms: Some(max), stddev_ms: Some(stddev) }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fingerprints {
    pub acir_hash: Option<String>,
    pub inputs_hash: Option<String>,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha256::digest;
    digest(bytes)
}