use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use noir_artifact_cli::fs::artifact::read_program_from_file;

use crate::{
    BackendInfo, BenchError, BenchResult, CommonMeta, EvmVerifyReport, SystemInfo,
    collect_system_info,
};

fn read_gas_from_snapshot(snapshot_path: &Path, match_pattern: &Option<String>) -> Option<u128> {
    let Ok(contents) = std::fs::read_to_string(snapshot_path) else {
        return None;
    };
    let lines = contents.lines();
    let mut best: Option<u128> = None;
    for line in lines {
        if let Some(pat) = match_pattern {
            if !line.contains(pat) {
                continue;
            }
        }
        if let Some(start_idx) = line.find("(gas:") {
            let slice = &line[start_idx + 5..];
            if let Some(end_idx) = slice.find(')') {
                let num_s = slice[..end_idx].trim().trim_start_matches(':').trim();
                let num_s = num_s
                    .chars()
                    .filter(|c| c.is_ascii_digit())
                    .collect::<String>();
                if let Ok(v) = num_s.parse::<u128>() {
                    best = Some(v);
                    break;
                }
            }
        }
    }
    best
}

fn read_gas_from_stdout(stdout: &str) -> Option<u128> {
    // Heuristic: look for "gas: <num>" first occurrence
    if let Some(idx) = stdout.find("gas:") {
        let mut s = &stdout[idx + 4..];
        // skip spaces
        s = s.trim_start();
        let num: String = s
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '_')
            .collect();
        let num = num.replace('_', "");
        if let Ok(v) = num.parse::<u128>() {
            return Some(v);
        }
    }
    None
}

fn foundry_backend_info(forge_bin: &Path) -> BackendInfo {
    let version = Command::new(forge_bin)
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    BackendInfo {
        name: "foundry".into(),
        version,
    }
}

fn estimate_latency_ms(gas_used: u128, gas_per_second: u64) -> u64 {
    if gas_per_second == 0 {
        return 0;
    }
    let secs = (gas_used as f64) / (gas_per_second as f64);
    (secs * 1000.0).round() as u64
}

pub fn run(
    foundry_dir: PathBuf,
    artifact: Option<PathBuf>,
    test_pattern: Option<String>,
    calldata_bytes: Option<u64>,
    gas_per_second: Option<u64>,
    forge_bin: Option<PathBuf>,
    json_out: Option<PathBuf>,
) -> BenchResult<()> {
    let forge = forge_bin.unwrap_or_else(|| PathBuf::from("forge"));

    // Execute forge test with gas report
    let mut cmd = Command::new(&forge);
    cmd.arg("test").arg("--gas-report");
    if let Some(pat) = &test_pattern {
        cmd.arg("-m").arg(pat);
    }
    cmd.current_dir(&foundry_dir);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = cmd
        .output()
        .map_err(|e| BenchError::Message(format!("failed to run forge: {e}")))?;
    let stdout_s = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr_s = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        return Err(BenchError::Message(format!(
            "forge test failed. stderr=\n{}",
            stderr_s
        )));
    }

    // Prefer .gas-snapshot, fallback to stdout heuristic
    let snapshot_path = foundry_dir.join(".gas-snapshot");
    let gas_used = read_gas_from_snapshot(&snapshot_path, &test_pattern)
        .or_else(|| read_gas_from_stdout(&stdout_s))
        .ok_or_else(|| {
            BenchError::Message("failed to parse gas used from Foundry outputs".into())
        })?;

    // Calldata bytes: user-provided or discover from stdout if test logs a line like "CALDATA_BYTES: <n>"
    let mut calldata_b = calldata_bytes;
    if calldata_b.is_none() {
        if let Some(idx) = stdout_s.find("CALDATA_BYTES:") {
            let s = &stdout_s[idx + "CALDATA_BYTES:".len()..];
            let num: String = s
                .chars()
                .skip_while(|c| c.is_whitespace())
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if let Ok(v) = num.parse::<u64>() {
                calldata_b = Some(v);
            }
        }
    }

    // Build meta: if artifact provided, use it to extract Noir version; else fill placeholders
    let meta = if let Some(artifact_path) = &artifact {
        let program = read_program_from_file(artifact_path)
            .map_err(|e| BenchError::Message(e.to_string()))?;
        let artifact_bytes = std::fs::read(artifact_path).ok();
        let meta = CommonMeta {
            name: "evm-verify".into(),
            timestamp: time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default(),
            noir_version: program.noir_version.clone(),
            artifact_path: artifact_path.clone(),
            cli_args: std::env::args().collect(),
            artifact_sha256: artifact_bytes.as_ref().map(|b| crate::sha256_hex(b)),
            inputs_sha256: None,
        };
        meta
    } else {
        let meta = CommonMeta {
            name: "evm-verify".into(),
            timestamp: time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default(),
            noir_version: "n/a".into(),
            artifact_path: foundry_dir.clone(),
            cli_args: std::env::args().collect(),
            artifact_sha256: None,
            inputs_sha256: None,
        };
        meta
    };

    let system: Option<SystemInfo> = Some(collect_system_info());
    let backend = foundry_backend_info(&forge);
    let est_latency_ms = Some(estimate_latency_ms(
        gas_used,
        gas_per_second.unwrap_or(1_250_000),
    ));

    let report = EvmVerifyReport {
        meta,
        gas_used,
        calldata_bytes: calldata_b,
        est_latency_ms,
        backend,
        system,
    };

    if let Some(json) = json_out {
        if let Some(dir) = json.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        std::fs::write(&json, serde_json::to_vec_pretty(&report).unwrap()).ok();
    }
    println!(
        "evm-verify: gas={} calldata_bytes={:?} latency_ms={:?}",
        report.gas_used, report.calldata_bytes, report.est_latency_ms
    );
    Ok(())
}
