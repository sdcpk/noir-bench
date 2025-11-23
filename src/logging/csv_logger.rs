use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::BenchResult;

pub struct CsvLogger {
    path: PathBuf,
    has_header: bool,
}

impl CsvLogger {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        let p = path.as_ref().to_path_buf();
        let has_header = p.exists() && std::fs::metadata(&p).ok().map(|m| m.len() > 0).unwrap_or(false);
        CsvLogger { path: p, has_header }
    }

    fn ensure_parent(&self) {
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
    }

    pub fn append_row(
        &mut self,
        timestamp: &str,
        circuit: &str,
        params: Option<u64>,
        backend: &str,
        compile_ms: Option<u128>,
        prove_ms: Option<u128>,
        memory_mb: Option<u64>,
        constraints: Option<u64>,
        proof_size: Option<u64>,
        evm_gas: Option<u64>,
        status: &str,
    ) -> BenchResult<()> {
        self.ensure_parent();
        let mut file: File = OpenOptions::new().create(true).append(true).open(&self.path).map_err(|e| crate::BenchError::Message(e.to_string()))?;
        let mut w = BufWriter::new(&mut file);
        if !self.has_header {
            let header = "timestamp,circuit,params,backend,compile_ms,prove_ms,memory_mb,constraints,proof_size,evm_gas,status\n";
            w.write_all(header.as_bytes()).ok();
            self.has_header = true;
        }
        let params_s = params.map(|v| v.to_string()).unwrap_or_else(|| "".to_string());
        let compile_s = compile_ms.map(|v| v.to_string()).unwrap_or_else(|| "".to_string());
        let prove_s = prove_ms.map(|v| v.to_string()).unwrap_or_else(|| "".to_string());
        let mem_s = memory_mb.map(|v| v.to_string()).unwrap_or_else(|| "".to_string());
        let constraints_s = constraints.map(|v| v.to_string()).unwrap_or_else(|| "".to_string());
        let proof_size_s = proof_size.map(|v| v.to_string()).unwrap_or_else(|| "".to_string());
        let evm_gas_s = evm_gas.map(|v| v.to_string()).unwrap_or_else(|| "".to_string());
        let mut line = String::new();
        line.push_str(timestamp);
        line.push(',');
        line.push_str(circuit);
        line.push(',');
        line.push_str(&params_s);
        line.push(',');
        line.push_str(backend);
        line.push(',');
        line.push_str(&compile_s);
        line.push(',');
        line.push_str(&prove_s);
        line.push(',');
        line.push_str(&mem_s);
        line.push(',');
        line.push_str(&constraints_s);
        line.push(',');
        line.push_str(&proof_size_s);
        line.push(',');
        line.push_str(&evm_gas_s);
        line.push(',');
        line.push_str(status);
        line.push('\n');
        w.write_all(line.as_bytes()).map_err(|e| crate::BenchError::Message(e.to_string()))?;
        Ok(())
    }
}


