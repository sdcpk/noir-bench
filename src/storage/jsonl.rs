//! JSONL (JSON Lines) storage for benchmark records.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use crate::BenchError;
use crate::core::schema::{BenchRecord, SCHEMA_VERSION};

/// JSONL writer/reader for benchmark records.
///
/// Each record is stored as a single JSON line, making it easy to append
/// and stream records without loading the entire file.
#[derive(Debug, Clone)]
pub struct JsonlWriter {
    path: PathBuf,
}

impl JsonlWriter {
    /// Create a new JsonlWriter for the given path.
    ///
    /// The file will be created if it doesn't exist when writing.
    pub fn new(path: impl AsRef<Path>) -> Self {
        JsonlWriter {
            path: path.as_ref().to_path_buf(),
        }
    }

    /// Get the path to the JSONL file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append a single record to the JSONL file.
    ///
    /// # Errors
    /// Returns an error if:
    /// - The record's schema_version doesn't match SCHEMA_VERSION
    /// - File operations fail
    /// - JSON serialization fails
    pub fn append(&self, record: &BenchRecord) -> Result<(), BenchError> {
        // Validate schema version
        if record.schema_version != SCHEMA_VERSION {
            return Err(BenchError::Message(format!(
                "schema version mismatch: record has v{}, expected v{}",
                record.schema_version, SCHEMA_VERSION
            )));
        }

        // Ensure parent directory exists
        if let Some(parent) = self.path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| BenchError::Message(format!("failed to create directory: {e}")))?;
            }
        }

        // Open file in append mode
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| BenchError::Message(format!("failed to open file: {e}")))?;

        // Serialize and write
        let json = serde_json::to_string(record)
            .map_err(|e| BenchError::Message(format!("failed to serialize record: {e}")))?;

        writeln!(file, "{}", json)
            .map_err(|e| BenchError::Message(format!("failed to write record: {e}")))?;

        Ok(())
    }

    /// Read all records from the JSONL file.
    ///
    /// # Errors
    /// Returns an error if:
    /// - The file doesn't exist
    /// - File operations fail
    /// - JSON deserialization fails for any line
    pub fn read_all(&self) -> Result<Vec<BenchRecord>, BenchError> {
        self.read_filtered(None)
    }

    /// Read records from the JSONL file, optionally filtered by circuit name.
    ///
    /// # Arguments
    /// * `circuit_name` - If Some, only return records matching this circuit name
    ///
    /// # Errors
    /// Returns an error if:
    /// - The file doesn't exist
    /// - File operations fail
    /// - JSON deserialization fails for any line
    pub fn read_filtered(
        &self,
        circuit_name: Option<&str>,
    ) -> Result<Vec<BenchRecord>, BenchError> {
        if !self.path.exists() {
            return Err(BenchError::Message(format!(
                "file not found: {}",
                self.path.display()
            )));
        }

        let file = File::open(&self.path)
            .map_err(|e| BenchError::Message(format!("failed to open file: {e}")))?;

        let reader = BufReader::new(file);
        let mut records = Vec::new();

        for (line_num, line_result) in reader.lines().enumerate() {
            let line = line_result.map_err(|e| {
                BenchError::Message(format!("failed to read line {}: {e}", line_num + 1))
            })?;

            // Skip empty lines
            if line.trim().is_empty() {
                continue;
            }

            let record: BenchRecord = serde_json::from_str(&line).map_err(|e| {
                BenchError::Message(format!("failed to parse line {}: {e}", line_num + 1))
            })?;

            // Apply filter if specified
            if let Some(name) = circuit_name {
                if record.circuit_name != name {
                    continue;
                }
            }

            records.push(record);
        }

        Ok(records)
    }

    /// Check if the JSONL file exists.
    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    /// Get the number of records in the file.
    ///
    /// This reads through the entire file to count lines.
    pub fn count(&self) -> Result<usize, BenchError> {
        if !self.path.exists() {
            return Ok(0);
        }

        let file = File::open(&self.path)
            .map_err(|e| BenchError::Message(format!("failed to open file: {e}")))?;

        let reader = BufReader::new(file);
        let count = reader
            .lines()
            .filter_map(|l| l.ok())
            .filter(|l| !l.trim().is_empty())
            .count();

        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::env::EnvironmentInfo;
    use crate::core::schema::{BackendInfo, RunConfig};

    fn make_test_record(name: &str) -> BenchRecord {
        BenchRecord::new(
            name.to_string(),
            EnvironmentInfo::default(),
            BackendInfo {
                name: "test".to_string(),
                version: None,
                variant: None,
            },
            RunConfig::default(),
        )
    }

    #[test]
    fn test_schema_version_validation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.jsonl");
        let writer = JsonlWriter::new(&path);

        let mut record = make_test_record("test");
        record.schema_version = 999; // Wrong version

        let result = writer.append(&record);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("schema version mismatch")
        );
    }
}
