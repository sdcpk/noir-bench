use std::path::PathBuf;

use serde_json::Value;

use crate::{BenchError, BenchResult};

pub fn run(baseline: PathBuf, contender: PathBuf, fail_on_regress_pct: Option<f64>) -> BenchResult<()> {
    let b = std::fs::read(&baseline).map_err(|e| BenchError::Message(e.to_string()))?;
    let c = std::fs::read(&contender).map_err(|e| BenchError::Message(e.to_string()))?;
    let b: Value = serde_json::from_slice(&b).map_err(|e| BenchError::Message(e.to_string()))?;
    let c: Value = serde_json::from_slice(&c).map_err(|e| BenchError::Message(e.to_string()))?;

    fn get_num(v: &Value, k: &str) -> Option<f64> { v.get(k).and_then(|x| x.as_f64()).or_else(|| v.get(k).and_then(|x| x.as_u64().map(|u| u as f64))) }

    let mut regress = false;
    let pairs = [
        ("execution_time_ms", "exec time"),
        ("prove_time_ms", "prove time"),
        ("backend_prove_time_ms", "backend prove time"),
        ("witness_gen_time_ms", "witness time"),
        ("verify_time_ms", "verify time"),
        ("total_gates", "total gates"),
        ("proof_size_bytes", "proof size"),
    ];
    for (key, label) in pairs {
        if let (Some(bv), Some(cv)) = (get_num(&b, key), get_num(&c, key)) {
            let delta = cv - bv;
            let pct = if bv != 0.0 { delta * 100.0 / bv } else { 0.0 };
            println!("{label}: baseline={bv:.3} contender={cv:.3} delta={delta:.3} ({pct:.2}%)");
            if let Some(th) = fail_on_regress_pct { if pct > th { regress = true; } }
        }
    }

    if regress { return Err(BenchError::Message("regression detected".into())); }
    Ok(())
}







