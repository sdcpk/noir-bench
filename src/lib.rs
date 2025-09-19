pub mod exec_cmd;
pub mod gates_cmd;
pub mod prove_cmd;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommonMeta {
    pub name: String,
    pub timestamp: String,
    pub noir_version: String,
    pub artifact_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecReport {
    #[serde(flatten)]
    pub meta: CommonMeta,
    pub execution_time_ms: u128,
    pub samples_count: usize,
    pub peak_memory_bytes: Option<u64>,
    pub flamegraph_svg: Option<PathBuf>,
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
    pub peak_memory_bytes: Option<u64>,
    pub proof_size_bytes: Option<u64>,
    pub gate_count: Option<u64>,
    pub backend: BackendInfo,
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
    pub backend: BackendInfo,
} 