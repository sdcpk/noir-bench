//! CSV export for benchmark records.

use std::io::Write;
use std::path::Path;

use crate::BenchError;
use crate::core::schema::BenchRecord;

/// CSV column headers in deterministic order.
pub const CSV_HEADERS: &[&str] = &[
    "schema_version",
    "record_id",
    "timestamp",
    "circuit_name",
    "backend_name",
    "backend_version",
    "git_sha",
    "nargo_version",
    "warmup",
    "iterations",
    "compile_mean_ms",
    "compile_stddev_ms",
    "witness_mean_ms",
    "witness_stddev_ms",
    "prove_mean_ms",
    "prove_stddev_ms",
    "verify_mean_ms",
    "verify_stddev_ms",
    "proof_size_bytes",
    "pk_size_bytes",
    "vk_size_bytes",
    "gate_count",
    "subgroup_size",
    "peak_rss_mb",
];

/// CSV exporter for benchmark records.
///
/// Exports BenchRecord data to CSV format with a flat column structure
/// and deterministic column order for easy comparison and analysis.
#[derive(Debug, Clone, Default)]
pub struct CsvExporter;

impl CsvExporter {
    /// Create a new CsvExporter.
    pub fn new() -> Self {
        CsvExporter
    }

    /// Export records to a CSV file.
    ///
    /// # Arguments
    /// * `records` - Slice of BenchRecord to export
    /// * `output` - Path to the output CSV file
    ///
    /// # Errors
    /// Returns an error if file operations or CSV writing fails.
    pub fn export(&self, records: &[BenchRecord], output: &Path) -> Result<(), BenchError> {
        // Ensure parent directory exists
        if let Some(parent) = output.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| BenchError::Message(format!("failed to create directory: {e}")))?;
            }
        }

        let file = std::fs::File::create(output)
            .map_err(|e| BenchError::Message(format!("failed to create file: {e}")))?;

        self.export_to_writer(records, file)
    }

    /// Export records to stdout.
    ///
    /// # Arguments
    /// * `records` - Slice of BenchRecord to export
    ///
    /// # Errors
    /// Returns an error if CSV writing fails.
    pub fn export_to_stdout(&self, records: &[BenchRecord]) -> Result<(), BenchError> {
        let stdout = std::io::stdout();
        let handle = stdout.lock();
        self.export_to_writer(records, handle)
    }

    /// Export records to any writer implementing Write.
    ///
    /// # Arguments
    /// * `records` - Slice of BenchRecord to export
    /// * `writer` - Any type implementing std::io::Write
    ///
    /// # Errors
    /// Returns an error if CSV writing fails.
    pub fn export_to_writer<W: Write>(
        &self,
        records: &[BenchRecord],
        writer: W,
    ) -> Result<(), BenchError> {
        let mut csv_writer = csv::Writer::from_writer(writer);

        // Write headers
        csv_writer
            .write_record(CSV_HEADERS)
            .map_err(|e| BenchError::Message(format!("failed to write CSV headers: {e}")))?;

        // Write each record
        for record in records {
            let row = self.record_to_row(record);
            csv_writer
                .write_record(&row)
                .map_err(|e| BenchError::Message(format!("failed to write CSV row: {e}")))?;
        }

        csv_writer
            .flush()
            .map_err(|e| BenchError::Message(format!("failed to flush CSV writer: {e}")))?;

        Ok(())
    }

    /// Convert a BenchRecord to a row of CSV values.
    fn record_to_row(&self, record: &BenchRecord) -> Vec<String> {
        vec![
            // schema_version
            record.schema_version.to_string(),
            // record_id
            record.record_id.clone(),
            // timestamp
            record.timestamp.clone(),
            // circuit_name
            record.circuit_name.clone(),
            // backend_name
            record.backend.name.clone(),
            // backend_version
            record.backend.version.clone().unwrap_or_default(),
            // git_sha
            record.env.git_sha.clone().unwrap_or_default(),
            // nargo_version
            record.env.nargo_version.clone().unwrap_or_default(),
            // warmup
            record.config.warmup_iterations.to_string(),
            // iterations
            record.config.measured_iterations.to_string(),
            // compile_mean_ms
            record
                .compile_stats
                .as_ref()
                .map(|s| format!("{:.3}", s.mean_ms))
                .unwrap_or_default(),
            // compile_stddev_ms
            record
                .compile_stats
                .as_ref()
                .and_then(|s| s.stddev_ms)
                .map(|v| format!("{:.3}", v))
                .unwrap_or_default(),
            // witness_mean_ms
            record
                .witness_stats
                .as_ref()
                .map(|s| format!("{:.3}", s.mean_ms))
                .unwrap_or_default(),
            // witness_stddev_ms
            record
                .witness_stats
                .as_ref()
                .and_then(|s| s.stddev_ms)
                .map(|v| format!("{:.3}", v))
                .unwrap_or_default(),
            // prove_mean_ms
            record
                .prove_stats
                .as_ref()
                .map(|s| format!("{:.3}", s.mean_ms))
                .unwrap_or_default(),
            // prove_stddev_ms
            record
                .prove_stats
                .as_ref()
                .and_then(|s| s.stddev_ms)
                .map(|v| format!("{:.3}", v))
                .unwrap_or_default(),
            // verify_mean_ms
            record
                .verify_stats
                .as_ref()
                .map(|s| format!("{:.3}", s.mean_ms))
                .unwrap_or_default(),
            // verify_stddev_ms
            record
                .verify_stats
                .as_ref()
                .and_then(|s| s.stddev_ms)
                .map(|v| format!("{:.3}", v))
                .unwrap_or_default(),
            // proof_size_bytes
            record
                .proof_size_bytes
                .map(|v| v.to_string())
                .unwrap_or_default(),
            // pk_size_bytes
            record
                .proving_key_size_bytes
                .map(|v| v.to_string())
                .unwrap_or_default(),
            // vk_size_bytes
            record
                .verification_key_size_bytes
                .map(|v| v.to_string())
                .unwrap_or_default(),
            // gate_count
            record
                .total_gates
                .map(|v| v.to_string())
                .unwrap_or_default(),
            // subgroup_size
            record
                .subgroup_size
                .map(|v| v.to_string())
                .unwrap_or_default(),
            // peak_rss_mb
            record
                .peak_rss_mb
                .map(|v| format!("{:.2}", v))
                .unwrap_or_default(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::env::EnvironmentInfo;
    use crate::core::schema::{BackendInfo, RunConfig, TimingStat};

    fn make_test_record(name: &str) -> BenchRecord {
        BenchRecord::new(
            name.to_string(),
            EnvironmentInfo::default(),
            BackendInfo {
                name: "test-backend".to_string(),
                version: Some("1.0.0".to_string()),
                variant: None,
            },
            RunConfig {
                warmup_iterations: 2,
                measured_iterations: 5,
                timeout_secs: None,
            },
        )
    }

    #[test]
    fn test_csv_headers_count() {
        // Ensure we have all expected columns
        assert_eq!(CSV_HEADERS.len(), 24);
    }

    #[test]
    fn test_record_to_row_length() {
        let exporter = CsvExporter::new();
        let record = make_test_record("test_circuit");
        let row = exporter.record_to_row(&record);
        assert_eq!(row.len(), CSV_HEADERS.len());
    }

    #[test]
    fn test_export_to_writer() {
        let exporter = CsvExporter::new();
        let mut record = make_test_record("test_circuit");
        record.prove_stats = Some(TimingStat::from_samples(&[100.0, 110.0, 105.0]));
        record.total_gates = Some(1000);
        record.proof_size_bytes = Some(2048);

        let mut buffer = Vec::new();
        exporter.export_to_writer(&[record], &mut buffer).unwrap();

        let csv_str = String::from_utf8(buffer).unwrap();
        let lines: Vec<&str> = csv_str.lines().collect();

        // Should have header + 1 data row
        assert_eq!(lines.len(), 2);

        // Check header
        assert!(lines[0].starts_with("schema_version,record_id,timestamp"));

        // Check data contains expected values
        assert!(lines[1].contains("test_circuit"));
        assert!(lines[1].contains("test-backend"));
        assert!(lines[1].contains("1000")); // gate_count
        assert!(lines[1].contains("2048")); // proof_size_bytes
    }

    #[test]
    fn test_export_multiple_records() {
        let exporter = CsvExporter::new();
        let records = vec![
            make_test_record("circuit_a"),
            make_test_record("circuit_b"),
            make_test_record("circuit_c"),
        ];

        let mut buffer = Vec::new();
        exporter.export_to_writer(&records, &mut buffer).unwrap();

        let csv_str = String::from_utf8(buffer).unwrap();
        let lines: Vec<&str> = csv_str.lines().collect();

        // Should have header + 3 data rows
        assert_eq!(lines.len(), 4);
    }

    #[test]
    fn test_export_to_file() {
        let exporter = CsvExporter::new();
        let record = make_test_record("test_circuit");

        let dir = tempfile::tempdir().unwrap();
        let output_path = dir.path().join("test_output.csv");

        exporter.export(&[record], &output_path).unwrap();

        assert!(output_path.exists());

        let contents = std::fs::read_to_string(&output_path).unwrap();
        assert!(contents.contains("schema_version"));
        assert!(contents.contains("test_circuit"));
    }

    #[test]
    fn test_export_empty_records() {
        let exporter = CsvExporter::new();

        let mut buffer = Vec::new();
        exporter.export_to_writer(&[], &mut buffer).unwrap();

        let csv_str = String::from_utf8(buffer).unwrap();
        let lines: Vec<&str> = csv_str.lines().collect();

        // Should have only header
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("schema_version"));
    }

    #[test]
    fn test_optional_fields_default_to_empty() {
        let exporter = CsvExporter::new();
        let record = make_test_record("test_circuit");

        let row = exporter.record_to_row(&record);

        // git_sha (index 6) should be empty since we didn't set it
        assert_eq!(row[6], "");
        // prove_mean_ms (index 14) should be empty
        assert_eq!(row[14], "");
        // gate_count (index 21) should be empty
        assert_eq!(row[21], "");
    }
}
