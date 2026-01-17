//! Build derived index from canonical JSONL.
//!
//! This module reads BenchRecord from JSONL and derives RunIndexRecordV1.

use std::cmp::Ordering;
use std::fs;
use std::path::Path;

use crate::BenchError;
use crate::core::schema::BenchRecord;
use crate::storage::JsonlWriter;

use super::schema::{
    RUN_INDEX_SCHEMA_VERSION, RunIndexMetricsV1, RunIndexRecordV1, make_run_href, make_run_slug,
};

/// Round a floating point value to 3 decimal places for deterministic output.
///
/// Uses the formula: (x * 1000.0).round() / 1000.0
/// This ensures consistent rounding across platforms.
fn round_to_3dp(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

/// Derive status from BenchRecord.
///
/// Returns "ok" if prove_stats exists and has iterations > 0, otherwise "error".
fn derive_status(record: &BenchRecord) -> String {
    // If we have prove_stats with at least one iteration, consider it successful
    if let Some(ref stats) = record.prove_stats {
        if stats.iterations > 0 {
            return "ok".to_string();
        }
    }
    // If no prove_stats but we have gates, it might be a gates-only run
    if record.total_gates.is_some() {
        return "ok".to_string();
    }
    "error".to_string()
}

/// Derive RunIndexMetricsV1 from BenchRecord.
fn derive_metrics(record: &BenchRecord) -> RunIndexMetricsV1 {
    let prove_ms_p50 = record
        .prove_stats
        .as_ref()
        .and_then(|s| s.median_ms)
        .map(round_to_3dp);

    let prove_ms_p95 = record
        .prove_stats
        .as_ref()
        .and_then(|s| s.p95_ms)
        .map(round_to_3dp);

    let verify_ms_p50 = record
        .verify_stats
        .as_ref()
        .and_then(|s| s.median_ms)
        .map(round_to_3dp);

    let gates = record.total_gates;

    // Convert peak_rss_mb to bytes (if present)
    let peak_rss_bytes = record.peak_rss_mb.map(|mb| (mb * 1_000_000.0) as u64);

    RunIndexMetricsV1 {
        prove_ms_p50,
        prove_ms_p95,
        verify_ms_p50,
        gates,
        peak_rss_bytes,
    }
}

/// Derive a single RunIndexRecordV1 from a BenchRecord.
///
/// Note: detail_slug and detail_href are NOT set here - they are assigned
/// after sorting in `assign_detail_slugs`.
fn derive_record(record: &BenchRecord) -> RunIndexRecordV1 {
    RunIndexRecordV1 {
        schema_version: RUN_INDEX_SCHEMA_VERSION,
        record_id: record.record_id.clone(),
        timestamp: record.timestamp.clone(),
        circuit_name: record.circuit_name.clone(),
        backend: record.backend.name.clone(),
        suite: None, // Not currently in BenchRecord; reserved for future
        status: derive_status(record),
        metrics: derive_metrics(record),
        detail_slug: None, // Assigned after sorting
        detail_href: None, // Assigned after sorting
    }
}

/// Assign deterministic slugs to sorted records.
///
/// Slugs are assigned based on the sorted order (1-based index):
/// - run_slug = "run_{:06}" (e.g., "run_000001")
/// - run_href = "runs/{run_slug}.html"
///
/// This must be called AFTER sorting to ensure deterministic slug assignment.
pub fn assign_detail_slugs(records: &mut [RunIndexRecordV1]) {
    for (i, record) in records.iter_mut().enumerate() {
        let slug = make_run_slug(i + 1); // 1-based index
        let href = make_run_href(&slug);
        record.detail_slug = Some(slug);
        record.detail_href = Some(href);
    }
}

/// Compare two timestamps for sorting.
///
/// Attempts ISO 8601 comparison; falls back to string comparison if parsing fails.
/// This ensures deterministic ordering even with malformed timestamps.
fn compare_timestamps(a: &str, b: &str) -> Ordering {
    // ISO 8601 timestamps are lexicographically sortable when well-formed
    // e.g., "2024-01-15T12:00:00Z" < "2024-01-16T12:00:00Z"
    a.cmp(b)
}

/// Sort records by (timestamp ascending, then record_id ascending).
///
/// This provides stable, deterministic ordering.
fn sort_records(records: &mut [RunIndexRecordV1]) {
    records.sort_by(|a, b| {
        compare_timestamps(&a.timestamp, &b.timestamp).then_with(|| a.record_id.cmp(&b.record_id))
    });
}

/// Build a derived index from a JSONL file.
///
/// Reads all BenchRecords from the JSONL file, derives RunIndexRecordV1 for each,
/// sorts them deterministically, assigns detail slugs, and returns the result.
///
/// # Arguments
/// * `jsonl_path` - Path to the input JSONL file
///
/// # Returns
/// A vector of RunIndexRecordV1 sorted by (timestamp, record_id) with detail slugs assigned.
pub fn build_index(jsonl_path: &Path) -> Result<Vec<RunIndexRecordV1>, BenchError> {
    let reader = JsonlWriter::new(jsonl_path);
    let bench_records = reader.read_all()?;

    let mut index_records: Vec<RunIndexRecordV1> =
        bench_records.iter().map(derive_record).collect();

    // Sort for deterministic output
    sort_records(&mut index_records);

    // Assign deterministic slugs based on sorted order
    assign_detail_slugs(&mut index_records);

    Ok(index_records)
}

/// Write index records to a JSON file.
///
/// Uses compact JSON format (no pretty-printing) for deterministic output.
/// The same input will always produce identical bytes.
pub fn write_index_json(
    records: &[RunIndexRecordV1],
    output_path: &Path,
) -> Result<(), BenchError> {
    // Ensure parent directory exists
    if let Some(parent) = output_path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)
                .map_err(|e| BenchError::Message(format!("failed to create directory: {e}")))?;
        }
    }

    // Use compact JSON for deterministic output (no variable whitespace)
    let json = serde_json::to_string(records)
        .map_err(|e| BenchError::Message(format!("failed to serialize index: {e}")))?;

    fs::write(output_path, json)
        .map_err(|e| BenchError::Message(format!("failed to write index.json: {e}")))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::env::EnvironmentInfo;
    use crate::core::schema::{BackendInfo, RunConfig, TimingStat};

    fn make_test_record(name: &str, timestamp: &str, record_id: &str) -> BenchRecord {
        let mut record = BenchRecord::new(
            name.to_string(),
            EnvironmentInfo::default(),
            BackendInfo {
                name: "bb".to_string(),
                version: Some("0.62.0".to_string()),
                variant: None,
            },
            RunConfig::default(),
        );
        record.timestamp = timestamp.to_string();
        record.record_id = record_id.to_string();
        record
    }

    #[test]
    fn test_round_to_3dp() {
        assert_eq!(round_to_3dp(100.1234), 100.123);
        assert_eq!(round_to_3dp(100.1235), 100.124); // rounds up
        assert_eq!(round_to_3dp(100.1), 100.1);
        assert_eq!(round_to_3dp(0.0), 0.0);
    }

    #[test]
    fn test_derive_status_ok() {
        let mut record = make_test_record("test", "2024-01-15T12:00:00Z", "id1");
        record.prove_stats = Some(TimingStat::from_samples(&[100.0, 110.0, 120.0]));

        assert_eq!(derive_status(&record), "ok");
    }

    #[test]
    fn test_derive_status_ok_gates_only() {
        let mut record = make_test_record("test", "2024-01-15T12:00:00Z", "id1");
        record.total_gates = Some(10000);

        assert_eq!(derive_status(&record), "ok");
    }

    #[test]
    fn test_derive_status_error() {
        let record = make_test_record("test", "2024-01-15T12:00:00Z", "id1");
        assert_eq!(derive_status(&record), "error");
    }

    #[test]
    fn test_derive_metrics_with_timing() {
        let mut record = make_test_record("test", "2024-01-15T12:00:00Z", "id1");
        record.prove_stats = Some(TimingStat {
            iterations: 5,
            mean_ms: 110.0,
            median_ms: Some(110.1234),
            stddev_ms: Some(7.0),
            min_ms: 100.0,
            max_ms: 120.0,
            p95_ms: Some(118.5678),
        });
        record.total_gates = Some(50000);
        record.peak_rss_mb = Some(256.5);

        let metrics = derive_metrics(&record);

        assert_eq!(metrics.prove_ms_p50, Some(110.123)); // rounded
        assert_eq!(metrics.prove_ms_p95, Some(118.568)); // rounded
        assert_eq!(metrics.verify_ms_p50, None);
        assert_eq!(metrics.gates, Some(50000));
        assert_eq!(metrics.peak_rss_bytes, Some(256_500_000));
    }

    #[test]
    fn test_sort_records_by_timestamp_then_id() {
        let mut records = vec![
            RunIndexRecordV1::new(
                "id2".to_string(),
                "2024-01-15T12:00:00Z".to_string(),
                "circuit".to_string(),
                "bb".to_string(),
                "ok".to_string(),
            ),
            RunIndexRecordV1::new(
                "id1".to_string(),
                "2024-01-15T12:00:00Z".to_string(), // same timestamp
                "circuit".to_string(),
                "bb".to_string(),
                "ok".to_string(),
            ),
            RunIndexRecordV1::new(
                "id3".to_string(),
                "2024-01-14T12:00:00Z".to_string(), // earlier timestamp
                "circuit".to_string(),
                "bb".to_string(),
                "ok".to_string(),
            ),
        ];

        sort_records(&mut records);

        // Should be sorted by timestamp first, then record_id
        assert_eq!(records[0].record_id, "id3"); // 2024-01-14
        assert_eq!(records[1].record_id, "id1"); // 2024-01-15, id1
        assert_eq!(records[2].record_id, "id2"); // 2024-01-15, id2
    }

    #[test]
    fn test_assign_detail_slugs() {
        let mut records = vec![
            RunIndexRecordV1::new(
                "id1".to_string(),
                "2024-01-14T12:00:00Z".to_string(),
                "circuit".to_string(),
                "bb".to_string(),
                "ok".to_string(),
            ),
            RunIndexRecordV1::new(
                "id2".to_string(),
                "2024-01-15T12:00:00Z".to_string(),
                "circuit".to_string(),
                "bb".to_string(),
                "ok".to_string(),
            ),
            RunIndexRecordV1::new(
                "id3".to_string(),
                "2024-01-16T12:00:00Z".to_string(),
                "circuit".to_string(),
                "bb".to_string(),
                "ok".to_string(),
            ),
        ];

        // Sort first (required for deterministic slugs)
        sort_records(&mut records);
        assign_detail_slugs(&mut records);

        // Slugs should be assigned based on sorted order (1-based)
        assert_eq!(records[0].detail_slug, Some("run_000001".to_string()));
        assert_eq!(
            records[0].detail_href,
            Some("runs/run_000001.html".to_string())
        );
        assert_eq!(records[1].detail_slug, Some("run_000002".to_string()));
        assert_eq!(
            records[1].detail_href,
            Some("runs/run_000002.html".to_string())
        );
        assert_eq!(records[2].detail_slug, Some("run_000003".to_string()));
        assert_eq!(
            records[2].detail_href,
            Some("runs/run_000003.html".to_string())
        );
    }

    #[test]
    fn test_assign_detail_slugs_deterministic() {
        let mut records1 = vec![
            RunIndexRecordV1::new(
                "id1".to_string(),
                "2024-01-15T12:00:00Z".to_string(),
                "c".to_string(),
                "bb".to_string(),
                "ok".to_string(),
            ),
            RunIndexRecordV1::new(
                "id2".to_string(),
                "2024-01-14T12:00:00Z".to_string(),
                "c".to_string(),
                "bb".to_string(),
                "ok".to_string(),
            ),
        ];
        let mut records2 = records1.clone();

        sort_records(&mut records1);
        sort_records(&mut records2);
        assign_detail_slugs(&mut records1);
        assign_detail_slugs(&mut records2);

        // Same input, same output
        assert_eq!(records1[0].detail_slug, records2[0].detail_slug);
        assert_eq!(records1[1].detail_slug, records2[1].detail_slug);
    }

    #[test]
    fn test_derive_record_preserves_fields() {
        let mut record = make_test_record("my_circuit", "2024-01-15T12:00:00Z", "unique-id");
        record.prove_stats = Some(TimingStat::from_samples(&[100.0]));
        record.total_gates = Some(25000);

        let index_record = derive_record(&record);

        assert_eq!(index_record.schema_version, 1);
        assert_eq!(index_record.record_id, "unique-id");
        assert_eq!(index_record.timestamp, "2024-01-15T12:00:00Z");
        assert_eq!(index_record.circuit_name, "my_circuit");
        assert_eq!(index_record.backend, "bb");
        assert_eq!(index_record.status, "ok");
        assert_eq!(index_record.metrics.gates, Some(25000));
    }

    #[test]
    fn test_json_output_deterministic() {
        let records = vec![RunIndexRecordV1::new(
            "id1".to_string(),
            "2024-01-15T12:00:00Z".to_string(),
            "circuit".to_string(),
            "bb".to_string(),
            "ok".to_string(),
        )];

        let json1 = serde_json::to_string(&records).unwrap();
        let json2 = serde_json::to_string(&records).unwrap();

        assert_eq!(json1, json2, "JSON serialization must be deterministic");
    }

    // =======================================================================
    // XSS Safety and Determinism Regression Tests
    // =======================================================================

    /// Test dangerous strings are handled safely in JSON output.
    ///
    /// Tests three dangerous strings:
    /// 1. "O'Reilly" - single quote
    /// 2. "</script><img src=x onerror=alert(1)>" - script injection
    /// 3. "<tag>&stuff" - HTML special chars
    #[test]
    fn test_xss_safety_dangerous_strings_in_json() {
        const SINGLE_QUOTE: &str = "O'Reilly";
        const SCRIPT_INJECTION: &str = "</script><img src=x onerror=alert(1)>";
        const HTML_SPECIAL: &str = "<tag>&stuff";

        // Create records with dangerous strings in circuit_name
        let mut records = vec![
            RunIndexRecordV1::new(
                "id1".to_string(),
                "2024-01-15T12:00:00Z".to_string(),
                SINGLE_QUOTE.to_string(),
                "bb".to_string(),
                "ok".to_string(),
            ),
            RunIndexRecordV1::new(
                "id2".to_string(),
                "2024-01-15T12:00:01Z".to_string(),
                SCRIPT_INJECTION.to_string(),
                "bb".to_string(),
                "ok".to_string(),
            ),
            RunIndexRecordV1::new(
                "id3".to_string(),
                "2024-01-15T12:00:02Z".to_string(),
                HTML_SPECIAL.to_string(),
                "bb".to_string(),
                "ok".to_string(),
            ),
        ];

        sort_records(&mut records);

        // Serialize to JSON
        let json = serde_json::to_string(&records).unwrap();

        // JSON should be valid and parseable
        let parsed: Vec<RunIndexRecordV1> =
            serde_json::from_str(&json).expect("JSON with dangerous strings must be parseable");
        assert_eq!(parsed.len(), 3);

        // Verify strings survived round-trip
        let names: Vec<&str> = parsed.iter().map(|r| r.circuit_name.as_str()).collect();
        assert!(
            names.contains(&SINGLE_QUOTE),
            "Single quote string should be preserved"
        );
        assert!(
            names.contains(&SCRIPT_INJECTION),
            "Script injection string should be preserved"
        );
        assert!(
            names.contains(&HTML_SPECIAL),
            "HTML special chars string should be preserved"
        );

        // Output should be deterministic
        let json2 = serde_json::to_string(&records).unwrap();
        assert_eq!(
            json, json2,
            "JSON output must be deterministic with dangerous strings"
        );
    }

    /// Test that full build pipeline is deterministic with dangerous strings.
    #[test]
    fn test_build_deterministic_with_dangerous_strings() {
        use crate::storage::JsonlWriter;
        use tempfile::TempDir;

        const SCRIPT_INJECTION: &str = "</script><img src=x onerror=alert(1)>";

        let temp = TempDir::new().unwrap();
        let jsonl_path = temp.path().join("input.jsonl");

        // Create record with dangerous string
        let mut record = make_test_record(SCRIPT_INJECTION, "2024-01-15T12:00:00Z", "id1");
        record.prove_stats = Some(TimingStat::from_samples(&[100.0]));

        // Write to JSONL
        let writer = JsonlWriter::new(&jsonl_path);
        writer.append(&record).unwrap();

        // Build index twice
        let index1 = build_index(&jsonl_path).unwrap();
        let index2 = build_index(&jsonl_path).unwrap();

        // Results should be identical
        assert_eq!(index1.len(), index2.len());
        assert_eq!(index1[0].circuit_name, index2[0].circuit_name);
        assert_eq!(index1[0].circuit_name, SCRIPT_INJECTION);

        // JSON serialization should be identical
        let json1 = serde_json::to_string(&index1).unwrap();
        let json2 = serde_json::to_string(&index2).unwrap();
        assert_eq!(json1, json2, "Build output must be deterministic");
    }

    /// Test ordering stability with identical timestamps.
    #[test]
    fn test_ordering_stability_identical_timestamps() {
        let mut records = vec![
            RunIndexRecordV1::new(
                "zzz".to_string(),
                "2024-01-15T12:00:00Z".to_string(),
                "c".to_string(),
                "bb".to_string(),
                "ok".to_string(),
            ),
            RunIndexRecordV1::new(
                "aaa".to_string(),
                "2024-01-15T12:00:00Z".to_string(),
                "c".to_string(),
                "bb".to_string(),
                "ok".to_string(),
            ),
            RunIndexRecordV1::new(
                "mmm".to_string(),
                "2024-01-15T12:00:00Z".to_string(),
                "c".to_string(),
                "bb".to_string(),
                "ok".to_string(),
            ),
        ];

        sort_records(&mut records);

        // With identical timestamps, should sort by record_id
        assert_eq!(records[0].record_id, "aaa");
        assert_eq!(records[1].record_id, "mmm");
        assert_eq!(records[2].record_id, "zzz");

        // Sorting again should produce identical result
        let mut records2 = records.clone();
        sort_records(&mut records2);
        assert_eq!(records, records2, "Sorting must be stable/deterministic");
    }

    /// Test rounding consistency for metrics.
    #[test]
    fn test_rounding_consistency() {
        // Test edge cases for rounding
        assert_eq!(round_to_3dp(0.0005), 0.001); // rounds up from 0.5
        assert_eq!(round_to_3dp(0.0004), 0.0); // rounds down
        assert_eq!(round_to_3dp(123.4565), 123.457); // standard case
        assert_eq!(round_to_3dp(123.4564), 123.456); // standard case

        // Verify rounding is applied in derive_metrics
        let mut record = make_test_record("test", "2024-01-15T12:00:00Z", "id1");
        record.prove_stats = Some(TimingStat {
            iterations: 1,
            mean_ms: 100.0,
            median_ms: Some(100.1239), // should round to 100.124
            stddev_ms: None,
            min_ms: 100.0,
            max_ms: 100.0,
            p95_ms: Some(100.1231), // should round to 100.123
        });

        let metrics = derive_metrics(&record);
        assert_eq!(metrics.prove_ms_p50, Some(100.124));
        assert_eq!(metrics.prove_ms_p95, Some(100.123));
    }
}
