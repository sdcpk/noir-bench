use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use noir_artifact_cli::fs::artifact::read_program_from_file;
use shlex::Shlex;

use crate::{
    BackendInfo, BenchError, BenchResult, CommonMeta, VerifyReport, collect_system_info,
    compute_iteration_stats,
};

pub trait VerifyProvider {
    fn verify(&self, artifact: &Path, proof: &Path) -> BenchResult<VerifyReport>;
    fn backend_info(&self) -> BackendInfo;
}

pub struct BarretenbergVerifyProvider {
    pub backend_path: PathBuf,
    pub extra_args: Vec<String>,
}

impl VerifyProvider for BarretenbergVerifyProvider {
    fn verify(&self, artifact: &Path, proof: &Path) -> BenchResult<VerifyReport> {
        let program =
            read_program_from_file(artifact).map_err(|e| BenchError::Message(e.to_string()))?;
        let mut cmd = Command::new(&self.backend_path);
        // Current bb verify does not accept -b; only -p (proof), -i (public inputs), -k (vk) optionally
        cmd.arg("verify").arg("-p").arg(proof);
        for a in &self.extra_args {
            cmd.arg(a);
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let start = Instant::now();
        let status = cmd
            .status()
            .map_err(|e| BenchError::Message(e.to_string()))?;
        let verify_time_ms = start.elapsed().as_millis();
        let ok = status.success();
        let artifact_bytes = std::fs::read(artifact).ok();
        let meta = CommonMeta {
            name: "verify".into(),
            timestamp: time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default(),
            noir_version: program.noir_version,
            artifact_path: artifact.to_path_buf(),
            cli_args: std::env::args().collect(),
            artifact_sha256: artifact_bytes.as_ref().map(|b| crate::sha256_hex(b)),
            inputs_sha256: None,
        };
        let report = VerifyReport {
            meta,
            verify_time_ms,
            ok,
            backend: self.backend_info(),
            system: Some(collect_system_info()),
            iterations: None,
        };
        Ok(report)
    }

    fn backend_info(&self) -> BackendInfo {
        let version = Command::new(&self.backend_path)
            .arg("--version")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        BackendInfo {
            name: "barretenberg".into(),
            version,
        }
    }
}

pub struct GenericVerifyProvider {
    pub command_template: String,
    pub extra_args: Vec<String>,
}

impl GenericVerifyProvider {
    fn build_command(&self, artifact: &Path, proof: &Path) -> BenchResult<Command> {
        let mut parts: Vec<String> = Shlex::new(&self.command_template).collect();
        if parts.is_empty() {
            return Err(BenchError::Message("empty command template".into()));
        }
        let artifact_s = artifact.to_string_lossy();
        let proof_s = proof.to_string_lossy();
        for p in &mut parts {
            *p = p
                .replace("{artifact}", &artifact_s)
                .replace("{proof}", &proof_s);
        }
        let mut cmd = Command::new(&parts[0]);
        for p in &parts[1..] {
            cmd.arg(p);
        }
        for a in &self.extra_args {
            cmd.arg(a);
        }
        Ok(cmd)
    }
}

impl VerifyProvider for GenericVerifyProvider {
    fn verify(&self, artifact: &Path, proof: &Path) -> BenchResult<VerifyReport> {
        let program =
            read_program_from_file(artifact).map_err(|e| BenchError::Message(e.to_string()))?;
        let mut cmd = self.build_command(artifact, proof)?;
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let start = Instant::now();
        let status = cmd
            .status()
            .map_err(|e| BenchError::Message(e.to_string()))?;
        let verify_time_ms = start.elapsed().as_millis();
        let ok = status.success();
        let artifact_bytes = std::fs::read(artifact).ok();
        let meta = CommonMeta {
            name: "verify".into(),
            timestamp: time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default(),
            noir_version: program.noir_version,
            artifact_path: artifact.to_path_buf(),
            cli_args: std::env::args().collect(),
            artifact_sha256: artifact_bytes.as_ref().map(|b| crate::sha256_hex(b)),
            inputs_sha256: None,
        };
        let report = VerifyReport {
            meta,
            verify_time_ms,
            ok,
            backend: self.backend_info(),
            system: Some(collect_system_info()),
            iterations: None,
        };
        Ok(report)
    }

    fn backend_info(&self) -> BackendInfo {
        let mut sh = Shlex::new(&self.command_template);
        let program = sh.next().unwrap_or_else(|| "generic".into());
        let version = Command::new(&program)
            .arg("--version")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        BackendInfo {
            name: "generic".into(),
            version,
        }
    }
}

pub fn run(
    artifact: PathBuf,
    proof: PathBuf,
    backend: Option<String>,
    backend_path: Option<PathBuf>,
    backend_args: Vec<String>,
    template: Option<String>,
    iterations: Option<usize>,
    warmup: Option<usize>,
    json_out: Option<PathBuf>,
) -> BenchResult<()> {
    let backend_name = backend.unwrap_or_else(|| "barretenberg".to_string());
    let iter_n = iterations.unwrap_or(1);
    let warmup_n = warmup.unwrap_or(0);
    let mut last: Option<VerifyReport> = None;
    let mut times: Vec<u128> = Vec::new();
    for i in 0..(warmup_n + iter_n) {
        let res = match (backend_name.as_str(), template.as_ref()) {
            ("barretenberg", None) => {
                let Some(path) = backend_path.clone() else {
                    return Err(BenchError::Message(
                        "barretenberg verify requires --backend-path".into(),
                    ));
                };
                let provider = BarretenbergVerifyProvider {
                    backend_path: path,
                    extra_args: backend_args.clone(),
                };
                provider.verify(&artifact, &proof)
            }
            (_, Some(tpl)) => {
                let provider = GenericVerifyProvider {
                    command_template: tpl.clone(),
                    extra_args: backend_args.clone(),
                };
                provider.verify(&artifact, &proof)
            }
            (other, None) => {
                return Err(BenchError::Message(format!(
                    "verify not implemented for backend '{other}'"
                )));
            }
        }?;
        if i >= warmup_n {
            times.push(res.verify_time_ms);
        }
        last = Some(res);
    }
    let mut report = last.expect("at least one verify iteration");
    if iter_n > 1 || warmup_n > 0 {
        report.iterations = Some(compute_iteration_stats(times, iter_n, warmup_n));
    }

    if let Some(json) = json_out {
        if let Some(dir) = json.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        std::fs::write(&json, serde_json::to_vec_pretty(&report).unwrap()).ok();
    }
    println!(
        "verify: backend={} time={}ms ok={}",
        report.backend.name, report.verify_time_ms, report.ok
    );
    Ok(())
}
