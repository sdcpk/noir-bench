use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::{BackendInfo, BenchError, BenchResult, CommonMeta, GatesOpcodeBreakdown, GatesReport, SystemInfo, collect_system_info};
use noir_artifact_cli::fs::artifact::read_program_from_file;
use shlex::Shlex;

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
        // Try `<backend_path> --version`
        let version = Command::new(&self.backend_path)
            .arg("--version")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        BackendInfo { name: self.backend_name.clone(), version }
    }
}

pub struct GenericGatesProvider {
    pub command_template: String,
    pub extra_args: Vec<String>,
}

impl GenericGatesProvider {
    fn build_command(&self, artifact: &Path) -> BenchResult<Command> {
        let mut parts: Vec<String> = Shlex::new(&self.command_template).collect();
        if parts.is_empty() { return Err(BenchError::Message("empty command template".into())); }
        let artifact_str = artifact.to_string_lossy();
        for p in &mut parts {
            *p = p.replace("{artifact}", &artifact_str);
        }
        let mut cmd = Command::new(&parts[0]);
        for p in &parts[1..] { cmd.arg(p); }
        for a in &self.extra_args { cmd.arg(a); }
        Ok(cmd)
    }
}

impl GatesProvider for GenericGatesProvider {
    fn gates(&self, artifact: &Path) -> BenchResult<BackendGatesResponse> {
        let mut cmd = self.build_command(artifact)?;
        let output = cmd.output().map_err(|e| BenchError::Message(e.to_string()))?;
        if !output.status.success() {
            return Err(BenchError::Message(format!(
                "generic gates failed: status={} stderr={}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        let parsed: BackendGatesResponse = serde_json::from_slice(&output.stdout)
            .map_err(|e| BenchError::Message(format!("failed to parse gates json: {e}")))?;
        Ok(parsed)
    }

    fn backend_info(&self) -> BackendInfo {
        let mut sh = Shlex::new(&self.command_template);
        let program = sh.next().unwrap_or_else(|| "generic".into());
        // Try `<program> --version`
        let version = Command::new(&program)
            .arg("--version")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        BackendInfo { name: "generic".into(), version }
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
    command_template: Option<String>,
    json_out: Option<PathBuf>,
) -> BenchResult<()> {
    let backend_name = backend.unwrap_or_else(|| "barretenberg".to_string());

    let provider: Box<dyn GatesProvider> = match (backend_name.as_str(), command_template.as_ref()) {
        ("generic", Some(tpl)) | (_, Some(tpl)) => {
            Box::new(GenericGatesProvider { command_template: tpl.clone(), extra_args: backend_args })
        }
        _ => {
            let backend_path = backend_path.ok_or_else(|| BenchError::Message("--backend-path is required".into()))?;
            let gates_command = "gates".to_string();
            Box::new(BackendGatesProvider { backend_name: backend_name.clone(), backend_path, gates_command, extra_args: backend_args })
        }
    };

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

    // Noir version from artifact if available
    let noir_version = read_program_from_file(&artifact).ok().map(|p| p.noir_version).unwrap_or_default();
    let meta = CommonMeta { name: "gates".into(), timestamp: now_string(), noir_version, artifact_path: artifact.clone(), cli_args: std::env::args().collect() };
    let system: SystemInfo = collect_system_info();
    let report = GatesReport { meta, total_gates, acir_opcodes, per_opcode, backend: provider.backend_info(), system: Some(system) };

    if let Some(json_path) = json_out { write_json(&json_path, &report)?; }

    println!("gates: backend={} total={} opcodes={}", backend_name, total_gates, acir_opcodes);
    Ok(())
} 