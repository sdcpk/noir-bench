//! Derived schema for run index records.
//!
//! These schemas are DERIVED artifacts - they do NOT modify or replace BenchRecord v1.
//! The canonical telemetry format remains JSONL with BenchRecord.

use serde::{Deserialize, Serialize};

/// Schema version for RunIndexRecord (derived schema, independent of BenchRecord).
pub const RUN_INDEX_SCHEMA_VERSION: u32 = 1;

/// Derived index record for history visualization.
///
/// This is a summarized view of BenchRecord, suitable for indexing and display.
/// It is NOT the canonical format - that remains JSONL with BenchRecord.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunIndexRecordV1 {
    /// Schema version (always 1 for this version)
    pub schema_version: u32,

    /// Unique record identifier (from BenchRecord.record_id)
    pub record_id: String,

    /// ISO 8601 timestamp (from BenchRecord.timestamp)
    pub timestamp: String,

    /// Circuit name (from BenchRecord.circuit_name)
    pub circuit_name: String,

    /// Backend name (from BenchRecord.backend.name)
    pub backend: String,

    /// Suite name if available (currently not in BenchRecord, reserved for future)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suite: Option<String>,

    /// Status: "ok" or "error" (derived best-effort)
    pub status: String,

    /// Summary metrics for display
    pub metrics: RunIndexMetricsV1,

    /// Deterministic slug for detail page (e.g., "run_000001")
    /// Assigned based on sorted index order (1-based).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail_slug: Option<String>,

    /// Relative href to detail page (e.g., "runs/run_000001.html")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail_href: Option<String>,
}

/// Summary metrics for the run index.
///
/// All fields are optional to handle sparse data gracefully.
/// Numeric values are rounded at derivation time for deterministic output.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct RunIndexMetricsV1 {
    /// Prove time p50 (median) in milliseconds, rounded to 3 decimal places
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prove_ms_p50: Option<f64>,

    /// Prove time p95 in milliseconds, rounded to 3 decimal places
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prove_ms_p95: Option<f64>,

    /// Verify time p50 (median) in milliseconds, rounded to 3 decimal places
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verify_ms_p50: Option<f64>,

    /// Total gates count
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gates: Option<u64>,

    /// Peak RSS in bytes (from peak_rss_mb * 1_000_000, if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peak_rss_bytes: Option<u64>,
}

impl RunIndexRecordV1 {
    /// Create a new RunIndexRecordV1 with required fields.
    pub fn new(
        record_id: String,
        timestamp: String,
        circuit_name: String,
        backend: String,
        status: String,
    ) -> Self {
        Self {
            schema_version: RUN_INDEX_SCHEMA_VERSION,
            record_id,
            timestamp,
            circuit_name,
            backend,
            suite: None,
            status,
            metrics: RunIndexMetricsV1::default(),
            detail_slug: None,
            detail_href: None,
        }
    }
}

/// Generate a deterministic run slug from a 1-based index.
///
/// Format: "run_{:06}" (e.g., "run_000001", "run_000002")
pub fn make_run_slug(index_1based: usize) -> String {
    format!("run_{:06}", index_1based)
}

/// Generate a relative href for a run detail page.
///
/// Format: "runs/{slug}.html"
pub fn make_run_href(slug: &str) -> String {
    format!("runs/{}.html", slug)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_index_record_serialization() {
        let record = RunIndexRecordV1 {
            schema_version: 1,
            record_id: "abc123".to_string(),
            timestamp: "2024-01-15T12:00:00Z".to_string(),
            circuit_name: "test_circuit".to_string(),
            backend: "bb".to_string(),
            suite: None,
            status: "ok".to_string(),
            metrics: RunIndexMetricsV1 {
                prove_ms_p50: Some(100.123),
                prove_ms_p95: Some(150.456),
                verify_ms_p50: None,
                gates: Some(10000),
                peak_rss_bytes: None,
            },
            detail_slug: Some("run_000001".to_string()),
            detail_href: Some("runs/run_000001.html".to_string()),
        };

        let json = serde_json::to_string(&record).unwrap();
        let parsed: RunIndexRecordV1 = serde_json::from_str(&json).unwrap();
        assert_eq!(record, parsed);
    }

    #[test]
    fn test_optional_fields_skipped_when_none() {
        let record = RunIndexRecordV1::new(
            "id".to_string(),
            "2024-01-15T12:00:00Z".to_string(),
            "circuit".to_string(),
            "bb".to_string(),
            "ok".to_string(),
        );

        let json = serde_json::to_string(&record).unwrap();
        // suite should not appear in JSON when None
        assert!(!json.contains("suite"));
        // Optional metric fields should not appear
        assert!(!json.contains("prove_ms_p50"));
        // detail_slug and detail_href should not appear when None
        assert!(!json.contains("detail_slug"));
        assert!(!json.contains("detail_href"));
    }

    #[test]
    fn test_make_run_slug() {
        assert_eq!(make_run_slug(1), "run_000001");
        assert_eq!(make_run_slug(42), "run_000042");
        assert_eq!(make_run_slug(999999), "run_999999");
        assert_eq!(make_run_slug(1000000), "run_1000000"); // exceeds 6 digits, still works
    }

    #[test]
    fn test_make_run_href() {
        assert_eq!(make_run_href("run_000001"), "runs/run_000001.html");
        assert_eq!(make_run_href("run_000042"), "runs/run_000042.html");
    }

    #[test]
    fn test_detail_fields_serialized_when_present() {
        let mut record = RunIndexRecordV1::new(
            "id".to_string(),
            "2024-01-15T12:00:00Z".to_string(),
            "circuit".to_string(),
            "bb".to_string(),
            "ok".to_string(),
        );
        record.detail_slug = Some("run_000001".to_string());
        record.detail_href = Some("runs/run_000001.html".to_string());

        let json = serde_json::to_string(&record).unwrap();
        assert!(json.contains("\"detail_slug\":\"run_000001\""));
        assert!(json.contains("\"detail_href\":\"runs/run_000001.html\""));
    }
}
