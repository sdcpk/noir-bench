use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::{BackendInfo, BenchError, BenchResult, CommonMeta, GatesOpcodeBreakdown, GatesReport};

pub trait GatesProvider {
    fn gates(&self, artifact: &Path) -> BenchResult<BackendGatesResponse>;
    fn backend_info(&self) -> BackendInfo;
}

pub struct BackendGatesProvider {
    pub backend_name: String,
    pub backend_path: PathBuf,
    pub gates_command: String,
    pub extra_args: Vec<String>,
}

impl GatesProvider for BackendGatesProvider {
    fn gates(&self, artifact: &Path) -> BenchResult<BackendGatesResponse> {
        let mut cmd = Command::new(&self.backend_path);
        cmd.arg(&self.gates_command).arg("-b").arg(artifact);
        for a in &self.extra_args { cmd.arg(a); }
        let output = cmd.output().map_err(|e| BenchError::Message(e.to_string()))?;
        if !output.status.success() {
            return Err(BenchError::Message(format!(
                "backend gates failed: status={} stderr={}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        let parsed: BackendGatesResponse = serde_json::from_slice(&output.stdout)
            .map_err(|e| BenchError::Message(format!("failed to parse gates json: {e}")))?;
        Ok(parsed)
    }

    fn backend_info(&self) -> BackendInfo {
        BackendInfo { name: self.backend_name.clone(), version: None }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendGatesReport {
    pub acir_opcodes: usize,
    #[serde(alias = "circuit_size")]
    pub total_gates: usize,
    #[serde(default)]
    pub gates_per_opcode: Vec<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendGatesResponse {
    pub functions: Vec<BackendGatesReport>,
}

fn now_string() -> String {
    time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339).unwrap_or_else(|_| "".to_string())
}

fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> BenchResult<()> {
    if let Some(dir) = path.parent() { std::fs::create_dir_all(dir).map_err(|e| BenchError::Message(e.to_string()))?; }
    let json = serde_json::to_vec_pretty(value).map_err(|e| BenchError::Message(e.to_string()))?;
    std::fs::write(path, json).map_err(|e| BenchError::Message(e.to_string()))
}

pub fn run(
    artifact: PathBuf,
    backend: Option<String>,
    backend_path: Option<PathBuf>,
    mut backend_args: Vec<String>,
    json_out: Option<PathBuf>,
) -> BenchResult<()> {
    let backend_name = backend.unwrap_or_else(|| "barretenberg".to_string());
    let backend_path = backend_path.ok_or_else(|| BenchError::Message("--backend-path is required".into()))?;

    // Default command and args similar to profiler
    let gates_command = "gates".to_string();
    let provider = BackendGatesProvider { backend_name: backend_name.clone(), backend_path, gates_command, extra_args: backend_args };

    let resp = provider.gates(&artifact)?;

    let mut total_gates = 0usize;
    let mut acir_opcodes = 0usize;
    let mut per_opcode: Vec<GatesOpcodeBreakdown> = Vec::new();

    if let Some(func) = resp.functions.get(0) {
        total_gates = func.total_gates;
        acir_opcodes = func.acir_opcodes;
        for (i, g) in func.gates_per_opcode.iter().copied().enumerate() {
            per_opcode.push(GatesOpcodeBreakdown { index: i, opcode: format!("acir[{i}]"), gates: g });
        }
    }

    let meta = CommonMeta { name: "gates".into(), timestamp: now_string(), noir_version: "".into(), artifact_path: artifact.clone() };
    let report = GatesReport { meta, total_gates, acir_opcodes, per_opcode, backend: provider.backend_info() };

    if let Some(json_path) = json_out { write_json(&json_path, &report)?; }

    println!("gates: backend={} total={} opcodes={}", backend_name, total_gates, acir_opcodes);
    Ok(())
} 