use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

use serde::Deserialize;
use serde_json::Value as JsonValue;

use crate::{BenchError, BenchResult};

#[derive(Debug, Deserialize)]
struct SuiteConfig {
    circuits: Vec<PathBuf>,
    tasks: Vec<String>,
    backend: Option<String>,
    backend_path: Option<PathBuf>,
    template: Option<String>,
    backend_args: Option<Vec<String>>, 
    iterations: Option<usize>,
    warmup: Option<usize>,
}

pub fn run(config_path: PathBuf, jsonl_out: Option<PathBuf>, summary_out: Option<PathBuf>) -> BenchResult<()> {
    let bytes = std::fs::read(&config_path).map_err(|e| BenchError::Message(e.to_string()))?;
    let cfg: SuiteConfig = serde_yaml::from_slice(&bytes).map_err(|e| BenchError::Message(e.to_string()))?;

    let mut jsonl: Option<File> = match jsonl_out {
        Some(p) => { if let Some(dir) = p.parent() { std::fs::create_dir_all(dir).ok(); } Some(File::create(&p).map_err(|e| BenchError::Message(e.to_string()))?) }
        None => None
    };

    let mut results: Vec<JsonValue> = Vec::new();

    for artifact in cfg.circuits.iter() {
        for task in cfg.tasks.iter() {
            match task.as_str() {
                "gates" => {
                    let tmp = tempfile::NamedTempFile::new().map_err(|e| BenchError::Message(e.to_string()))?;
                    crate::gates_cmd::run(artifact.clone(), cfg.backend.clone(), cfg.backend_path.clone(), cfg.backend_args.clone().unwrap_or_default(), cfg.template.clone(), Some(tmp.path().to_path_buf()))?;
                    let bytes = std::fs::read(tmp.path()).unwrap_or_default();
                    if let Ok(v) = serde_json::from_slice::<JsonValue>(&bytes) {
                        results.push(v.clone());
                        if let Some(f) = jsonl.as_mut() {
                            let compact = serde_json::to_vec(&v).unwrap_or_default();
                            let _ = f.write_all(&compact);
                            let _ = f.write_all(b"\n");
                        }
                    }
                }
                "prove" => {
                    let tmp = tempfile::NamedTempFile::new().map_err(|e| BenchError::Message(e.to_string()))?;
                    // try to locate Prover.toml either alongside the artifact or in the parent of target/
                    let mut prover_path: Option<PathBuf> = None;
                    if let Some(dir) = artifact.parent() {
                        let cand1 = dir.join("Prover.toml");
                        if cand1.exists() { prover_path = Some(cand1); }
                        if prover_path.is_none() {
                            if let Some(parent2) = dir.parent() {
                                let cand2 = parent2.join("Prover.toml");
                                if cand2.exists() { prover_path = Some(cand2); }
                            }
                        }
                    }
                    crate::prove_cmd::run(artifact.clone(), prover_path, cfg.backend.clone(), cfg.backend_path.clone(), cfg.backend_args.clone().unwrap_or_default(), cfg.template.clone(), 0, cfg.iterations, cfg.warmup, Some(tmp.path().to_path_buf()))?;
                    let bytes = std::fs::read(tmp.path()).unwrap_or_default();
                    if let Ok(v) = serde_json::from_slice::<JsonValue>(&bytes) {
                        results.push(v.clone());
                        if let Some(f) = jsonl.as_mut() {
                            let compact = serde_json::to_vec(&v).unwrap_or_default();
                            let _ = f.write_all(&compact);
                            let _ = f.write_all(b"\n");
                        }
                    }
                }
                "verify" => {
                    // skip: needs proof path
                }
                "exec" => {
                    // skip: needs Prover.toml
                }
                _ => {}
            }
        }
        // done per artifact
    }

    if let Some(p) = summary_out {
        if let Some(dir) = p.parent() { std::fs::create_dir_all(dir).ok(); }
        let summary = serde_json::json!({ "results": results });
        std::fs::write(&p, serde_json::to_vec_pretty(&summary).unwrap_or_default()).ok();
    }
    Ok(())
}


