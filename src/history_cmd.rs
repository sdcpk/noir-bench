//! CLI command handler for `history build`.
//!
//! Builds derived artifacts (index.json, index.html, per-run detail pages) from canonical JSONL.

use std::path::PathBuf;

use crate::history::{build_index, write_history_html, write_index_json, write_run_detail_html};
use crate::storage::JsonlWriter;
use crate::{BenchError, BenchResult};

/// Run the `history build` command.
///
/// Reads BenchRecord from JSONL, derives RunIndexRecordV1, and writes:
/// - <out>/index.json - derived index data
/// - <out>/index.html - single-file HTML dashboard
/// - <out>/runs/*.html - per-run detail pages (static, no JS)
///
/// # Arguments
/// * `jsonl_path` - Path to input JSONL file
/// * `out_dir` - Output directory for derived artifacts
pub fn build(jsonl_path: PathBuf, out_dir: PathBuf) -> BenchResult<()> {
    // Validate input exists
    if !jsonl_path.exists() {
        return Err(BenchError::Message(format!(
            "JSONL file not found: {}",
            jsonl_path.display()
        )));
    }

    // Build the index from JSONL (this also assigns detail slugs)
    eprintln!("Reading JSONL from: {}", jsonl_path.display());
    let records = build_index(&jsonl_path)?;
    eprintln!("Derived {} index record(s)", records.len());

    // Ensure output directory exists
    if !out_dir.exists() {
        std::fs::create_dir_all(&out_dir)
            .map_err(|e| BenchError::Message(format!("failed to create output directory: {e}")))?;
    }

    // Write index.json
    let json_path = out_dir.join("index.json");
    write_index_json(&records, &json_path)?;
    eprintln!("Wrote index.json to: {}", json_path.display());

    // Write index.html
    let html_path = out_dir.join("index.html");
    write_history_html(&html_path)?;
    eprintln!("Wrote index.html to: {}", html_path.display());

    // Generate per-run detail pages
    let runs_dir = out_dir.join("runs");
    if !runs_dir.exists() {
        std::fs::create_dir_all(&runs_dir)
            .map_err(|e| BenchError::Message(format!("failed to create runs directory: {e}")))?;
    }

    // Read the original BenchRecords to get full data for detail pages
    let reader = JsonlWriter::new(&jsonl_path);
    let bench_records = reader.read_all()?;

    // Build a map from record_id to BenchRecord for lookup
    let record_map: std::collections::HashMap<&str, &crate::core::schema::BenchRecord> =
        bench_records
            .iter()
            .map(|r| (r.record_id.as_str(), r))
            .collect();

    // Generate detail pages for each index record
    let mut detail_count = 0;
    for index_record in &records {
        if let (Some(slug), Some(bench_record)) = (
            index_record.detail_slug.as_ref(),
            record_map.get(index_record.record_id.as_str()),
        ) {
            let detail_path = runs_dir.join(format!("{}.html", slug));
            write_run_detail_html(bench_record, slug, &detail_path)?;
            detail_count += 1;
        }
    }
    eprintln!(
        "Wrote {} detail page(s) to: {}",
        detail_count,
        runs_dir.display()
    );

    eprintln!("History build complete.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::env::EnvironmentInfo;
    use crate::core::schema::{BackendInfo, BenchRecord, RunConfig, TimingStat};
    use crate::storage::JsonlWriter;
    use tempfile::TempDir;

    fn make_test_record(name: &str, timestamp: &str) -> BenchRecord {
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
        record.prove_stats = Some(TimingStat::from_samples(&[100.0, 110.0, 120.0]));
        record.total_gates = Some(50000);
        record
    }

    #[test]
    fn test_build_creates_output_files() {
        let temp = TempDir::new().unwrap();
        let jsonl_path = temp.path().join("input.jsonl");
        let out_dir = temp.path().join("out");

        // Write test JSONL
        let writer = JsonlWriter::new(&jsonl_path);
        writer
            .append(&make_test_record("circuit1", "2024-01-15T12:00:00Z"))
            .unwrap();
        writer
            .append(&make_test_record("circuit2", "2024-01-15T13:00:00Z"))
            .unwrap();

        // Run build
        let result = build(jsonl_path, out_dir.clone());
        assert!(result.is_ok(), "Build should succeed: {:?}", result.err());

        // Verify outputs exist
        assert!(
            out_dir.join("index.json").exists(),
            "index.json should exist"
        );
        assert!(
            out_dir.join("index.html").exists(),
            "index.html should exist"
        );
        assert!(out_dir.join("runs").exists(), "runs directory should exist");

        // Verify index.json is valid JSON with detail slugs
        let json_content = std::fs::read_to_string(out_dir.join("index.json")).unwrap();
        let records: Vec<crate::history::RunIndexRecordV1> =
            serde_json::from_str(&json_content).expect("index.json should be valid JSON");
        assert_eq!(records.len(), 2);

        // Verify detail slugs are assigned
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

        // Verify per-run detail pages exist
        assert!(
            out_dir.join("runs/run_000001.html").exists(),
            "run_000001.html should exist"
        );
        assert!(
            out_dir.join("runs/run_000002.html").exists(),
            "run_000002.html should exist"
        );

        // Verify detail page content
        let detail1 = std::fs::read_to_string(out_dir.join("runs/run_000001.html")).unwrap();
        assert!(
            detail1.contains("circuit1"),
            "Detail page should contain circuit name"
        );
        assert!(
            detail1.contains("Back to History"),
            "Detail page should have back link"
        );
        assert!(
            !detail1.contains("<script"),
            "Detail page should have no JavaScript"
        );

        // Verify index.html has expected content
        let html_content = std::fs::read_to_string(out_dir.join("index.html")).unwrap();
        assert!(html_content.contains("noir-bench History"));
        assert!(html_content.contains("fetch('./index.json')"));
    }

    #[test]
    fn test_build_missing_input() {
        let temp = TempDir::new().unwrap();
        let jsonl_path = temp.path().join("nonexistent.jsonl");
        let out_dir = temp.path().join("out");

        let result = build(jsonl_path, out_dir);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_build_deterministic_output() {
        let temp = TempDir::new().unwrap();
        let jsonl_path = temp.path().join("input.jsonl");

        // Write test JSONL
        let writer = JsonlWriter::new(&jsonl_path);
        writer
            .append(&make_test_record("circuit", "2024-01-15T12:00:00Z"))
            .unwrap();

        // Build twice to different directories
        let out1 = temp.path().join("out1");
        let out2 = temp.path().join("out2");

        build(jsonl_path.clone(), out1.clone()).unwrap();
        build(jsonl_path, out2.clone()).unwrap();

        // Compare outputs - all must be byte-for-byte identical
        let json1 = std::fs::read_to_string(out1.join("index.json")).unwrap();
        let json2 = std::fs::read_to_string(out2.join("index.json")).unwrap();
        assert_eq!(json1, json2, "index.json must be deterministic");

        let html1 = std::fs::read_to_string(out1.join("index.html")).unwrap();
        let html2 = std::fs::read_to_string(out2.join("index.html")).unwrap();
        assert_eq!(html1, html2, "index.html must be deterministic");

        // Per-run detail pages must also be deterministic
        let detail1 = std::fs::read_to_string(out1.join("runs/run_000001.html")).unwrap();
        let detail2 = std::fs::read_to_string(out2.join("runs/run_000001.html")).unwrap();
        assert_eq!(detail1, detail2, "detail pages must be deterministic");
    }

    #[test]
    fn test_build_xss_safety() {
        let temp = TempDir::new().unwrap();
        let jsonl_path = temp.path().join("input.jsonl");
        let out_dir = temp.path().join("out");

        // Write test JSONL with XSS attack strings
        let mut record = make_test_record("<script>alert('xss')</script>", "2024-01-15T12:00:00Z");
        record.record_id = "<img onerror=alert(1)>".to_string();

        let writer = JsonlWriter::new(&jsonl_path);
        writer.append(&record).unwrap();

        // Build
        build(jsonl_path, out_dir.clone()).unwrap();

        // Verify detail page escapes dangerous strings
        let detail = std::fs::read_to_string(out_dir.join("runs/run_000001.html")).unwrap();

        // Should NOT contain unescaped dangerous strings
        assert!(
            !detail.contains("<script>alert"),
            "Should escape script tags"
        );
        assert!(!detail.contains("<img onerror"), "Should escape img tags");

        // Should contain escaped versions
        assert!(
            detail.contains("&lt;script&gt;"),
            "Should contain escaped script tag"
        );
        assert!(
            detail.contains("&lt;img onerror"),
            "Should contain escaped img tag"
        );
    }

    #[test]
    fn test_build_link_integrity() {
        let temp = TempDir::new().unwrap();
        let jsonl_path = temp.path().join("input.jsonl");
        let out_dir = temp.path().join("out");

        // Write test JSONL
        let writer = JsonlWriter::new(&jsonl_path);
        writer
            .append(&make_test_record("circuit1", "2024-01-15T12:00:00Z"))
            .unwrap();
        writer
            .append(&make_test_record("circuit2", "2024-01-15T13:00:00Z"))
            .unwrap();

        // Build
        build(jsonl_path, out_dir.clone()).unwrap();

        // Read index.json to get detail_href values
        let json_content = std::fs::read_to_string(out_dir.join("index.json")).unwrap();
        let records: Vec<crate::history::RunIndexRecordV1> =
            serde_json::from_str(&json_content).unwrap();

        // Verify each detail_href points to an existing file
        for record in &records {
            if let Some(href) = &record.detail_href {
                let detail_path = out_dir.join(href);
                assert!(
                    detail_path.exists(),
                    "detail_href '{}' should point to existing file",
                    href
                );
            }
        }

        // Verify detail pages link back to index
        let detail1 = std::fs::read_to_string(out_dir.join("runs/run_000001.html")).unwrap();
        assert!(
            detail1.contains("href=\"../index.html\""),
            "Detail page should link back to ../index.html"
        );
    }
}
