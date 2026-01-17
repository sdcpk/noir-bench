//! Integration tests for JSONL storage.

use noir_bench::core::env::EnvironmentInfo;
use noir_bench::core::schema::{BackendInfo, BenchRecord, RunConfig};
use noir_bench::storage::JsonlWriter;

/// Helper to create a test record with a given name
fn make_test_record(name: &str) -> BenchRecord {
    BenchRecord::new(
        name.to_string(),
        EnvironmentInfo::default(),
        BackendInfo {
            name: "test-backend".to_string(),
            version: Some("1.0.0".to_string()),
            variant: None,
        },
        RunConfig::default(),
    )
}

#[test]
fn test_write_and_read_multiple_records() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bench.jsonl");
    let writer = JsonlWriter::new(&path);

    // Write 3 records
    let record1 = make_test_record("circuit_a");
    let record2 = make_test_record("circuit_b");
    let record3 = make_test_record("circuit_c");

    writer.append(&record1).expect("failed to append record 1");
    writer.append(&record2).expect("failed to append record 2");
    writer.append(&record3).expect("failed to append record 3");

    // Read them back
    let records = writer.read_all().expect("failed to read records");

    // Verify count and order
    assert_eq!(records.len(), 3);
    assert_eq!(records[0].circuit_name, "circuit_a");
    assert_eq!(records[1].circuit_name, "circuit_b");
    assert_eq!(records[2].circuit_name, "circuit_c");

    // Verify content
    assert_eq!(records[0].record_id, record1.record_id);
    assert_eq!(records[1].record_id, record2.record_id);
    assert_eq!(records[2].record_id, record3.record_id);

    // Verify backend info is preserved
    assert_eq!(records[0].backend.name, "test-backend");
    assert_eq!(records[0].backend.version, Some("1.0.0".to_string()));
}

#[test]
fn test_append_does_not_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("append_test.jsonl");
    let writer = JsonlWriter::new(&path);

    // Write first record
    let record1 = make_test_record("first");
    writer
        .append(&record1)
        .expect("failed to append first record");

    // Verify 1 record
    assert_eq!(writer.count().unwrap(), 1);

    // Create a NEW writer instance (simulates reopening)
    let writer2 = JsonlWriter::new(&path);

    // Append second record
    let record2 = make_test_record("second");
    writer2
        .append(&record2)
        .expect("failed to append second record");

    // Verify 2 records (not overwritten)
    assert_eq!(writer2.count().unwrap(), 2);

    // Read and verify both are present
    let records = writer2.read_all().unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].circuit_name, "first");
    assert_eq!(records[1].circuit_name, "second");
}

#[test]
fn test_read_filtered_by_circuit_name() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("filtered.jsonl");
    let writer = JsonlWriter::new(&path);

    // Write mixed records
    writer.append(&make_test_record("alpha")).unwrap();
    writer.append(&make_test_record("beta")).unwrap();
    writer.append(&make_test_record("alpha")).unwrap();
    writer.append(&make_test_record("gamma")).unwrap();
    writer.append(&make_test_record("alpha")).unwrap();

    // Filter by "alpha"
    let alpha_records = writer.read_filtered(Some("alpha")).unwrap();
    assert_eq!(alpha_records.len(), 3);
    for r in &alpha_records {
        assert_eq!(r.circuit_name, "alpha");
    }

    // Filter by "beta"
    let beta_records = writer.read_filtered(Some("beta")).unwrap();
    assert_eq!(beta_records.len(), 1);
    assert_eq!(beta_records[0].circuit_name, "beta");

    // Filter by non-existent
    let none_records = writer.read_filtered(Some("nonexistent")).unwrap();
    assert_eq!(none_records.len(), 0);

    // No filter returns all
    let all_records = writer.read_filtered(None).unwrap();
    assert_eq!(all_records.len(), 5);
}

#[test]
fn test_read_nonexistent_file_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("does_not_exist.jsonl");
    let writer = JsonlWriter::new(&path);

    let result = writer.read_all();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("file not found"));
}

#[test]
fn test_exists_and_count() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("count_test.jsonl");
    let writer = JsonlWriter::new(&path);

    // File doesn't exist yet
    assert!(!writer.exists());
    assert_eq!(writer.count().unwrap(), 0);

    // Write some records
    writer.append(&make_test_record("one")).unwrap();
    writer.append(&make_test_record("two")).unwrap();

    // Now it exists
    assert!(writer.exists());
    assert_eq!(writer.count().unwrap(), 2);
}

#[test]
fn test_record_preserves_optional_fields() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("optional_fields.jsonl");
    let writer = JsonlWriter::new(&path);

    // Create record with some optional fields set
    let mut record = make_test_record("with_optionals");
    record.total_gates = Some(12345);
    record.proof_size_bytes = Some(4096);
    record.peak_rss_mb = Some(512.5);
    record.circuit_path = Some("/path/to/circuit.json".to_string());

    writer.append(&record).unwrap();

    let records = writer.read_all().unwrap();
    assert_eq!(records.len(), 1);

    let loaded = &records[0];
    assert_eq!(loaded.total_gates, Some(12345));
    assert_eq!(loaded.proof_size_bytes, Some(4096));
    assert_eq!(loaded.peak_rss_mb, Some(512.5));
    assert_eq!(
        loaded.circuit_path,
        Some("/path/to/circuit.json".to_string())
    );
}
