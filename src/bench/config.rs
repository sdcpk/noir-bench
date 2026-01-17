use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::{BenchError, BenchResult};

#[derive(Debug, Clone)]
pub struct CircuitSpec {
    pub name: String,
    pub path: PathBuf,
    pub params: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct RawCircuit {
    pub name: String,
    pub path: PathBuf,
    #[serde(default)]
    pub params: Option<Vec<u64>>,
}

#[derive(Debug, Deserialize)]
struct BenchConfig {
    #[serde(rename = "circuit")]
    pub circuits: Vec<RawCircuit>,
}

pub fn load_bench_config(path: &Path) -> BenchResult<Vec<CircuitSpec>> {
    let s = std::fs::read_to_string(path).map_err(|e| BenchError::Message(e.to_string()))?;
    let cfg: BenchConfig = toml::from_str(&s).map_err(|e| BenchError::Message(e.to_string()))?;
    let mut specs: Vec<CircuitSpec> = Vec::new();
    for c in cfg.circuits {
        match c.params {
            Some(list) if !list.is_empty() => {
                for p in list {
                    specs.push(CircuitSpec {
                        name: c.name.clone(),
                        path: c.path.clone(),
                        params: Some(p),
                    });
                }
            }
            _ => {
                specs.push(CircuitSpec {
                    name: c.name,
                    path: c.path,
                    params: None,
                });
            }
        }
    }
    Ok(specs)
}

pub fn list_circuits_in_config(
    path: &Path,
) -> BenchResult<Vec<(String, PathBuf, Option<Vec<u64>>)>> {
    let s = std::fs::read_to_string(path).map_err(|e| BenchError::Message(e.to_string()))?;
    let cfg: BenchConfig = toml::from_str(&s).map_err(|e| BenchError::Message(e.to_string()))?;
    Ok(cfg
        .circuits
        .into_iter()
        .map(|c| (c.name, c.path, c.params))
        .collect())
}
