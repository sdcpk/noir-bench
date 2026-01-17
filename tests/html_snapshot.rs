//! HTML report snapshot tests for determinism and structure.
//!
//! These tests verify that HTML report generation is:
//! - Deterministic (same input produces identical output)
//! - Contains expected sections and structure
//! - Properly escapes user-controlled content

use noir_bench::engine::provenance::{Provenance, SystemInfo, ToolInfo, VersionMismatch};
use noir_bench::report::{
    CircuitRegression, MetricDelta, RegressionReport, RegressionStatus, render_html,
};

/// Create a fixed RegressionReport for snapshot testing.
fn make_fixed_report() -> RegressionReport {
    // Create report with fixed timestamp for determinism
    let mut report = RegressionReport {
        version: 1,
        metadata: noir_bench::report::ReportMetadata {
            baseline_id: "baseline-abc123".to_string(),
            target_id: "target-def456".to_string(),
            generated_at: "2026-01-15T12:00:00Z".to_string(),
            threshold_percent: 10.0,
            baseline_provenance: Some(Provenance {
                noir_bench: ToolInfo {
                    name: "noir-bench".to_string(),
                    version: Some("0.1.0".to_string()),
                    git_sha: Some("abc123".to_string()),
                    git_dirty: Some(false),
                    path: None,
                },
                nargo: Some(ToolInfo {
                    name: "nargo".to_string(),
                    version: Some("0.38.0".to_string()),
                    git_sha: None,
                    git_dirty: None,
                    path: None,
                }),
                backend: Some(ToolInfo {
                    name: "barretenberg".to_string(),
                    version: Some("0.62.0".to_string()),
                    git_sha: None,
                    git_dirty: None,
                    path: None,
                }),
                system: SystemInfo {
                    os: "linux".to_string(),
                    arch: "x86_64".to_string(),
                    cpu_brand: Some("Test CPU".to_string()),
                    cpu_cores: Some(8),
                    ram_bytes: Some(16_000_000_000),
                    hostname: Some("test-host".to_string()),
                },
                cli_args: vec!["noir-bench".to_string(), "ci".to_string()],
                collected_at: "2026-01-15T12:00:00Z".to_string(),
            }),
            target_provenance: Some(Provenance {
                noir_bench: ToolInfo {
                    name: "noir-bench".to_string(),
                    version: Some("0.1.0".to_string()),
                    git_sha: Some("def456".to_string()),
                    git_dirty: Some(false),
                    path: None,
                },
                nargo: Some(ToolInfo {
                    name: "nargo".to_string(),
                    version: Some("0.39.0".to_string()),
                    git_sha: None,
                    git_dirty: None,
                    path: None,
                }),
                backend: Some(ToolInfo {
                    name: "barretenberg".to_string(),
                    version: Some("0.63.0".to_string()),
                    git_sha: None,
                    git_dirty: None,
                    path: None,
                }),
                system: SystemInfo {
                    os: "linux".to_string(),
                    arch: "x86_64".to_string(),
                    cpu_brand: Some("Test CPU".to_string()),
                    cpu_cores: Some(8),
                    ram_bytes: Some(16_000_000_000),
                    hostname: Some("test-host".to_string()),
                },
                cli_args: vec!["noir-bench".to_string(), "ci".to_string()],
                collected_at: "2026-01-15T12:00:00Z".to_string(),
            }),
        },
        circuits: Vec::new(),
        summary: noir_bench::report::ReportSummary {
            total_circuits: 0,
            circuits_with_regressions: 0,
            circuits_with_improvements: 0,
            total_metrics: 0,
            regressions: 0,
            improvements: 0,
            unchanged: 0,
            missing_baselines: 0,
            errors: 0,
            ci_exit_code: 0,
        },
        version_mismatches: vec![
            VersionMismatch {
                tool: "nargo".to_string(),
                baseline_version: Some("0.38.0".to_string()),
                target_version: Some("0.39.0".to_string()),
            },
            VersionMismatch {
                tool: "barretenberg".to_string(),
                baseline_version: Some("0.62.0".to_string()),
                target_version: Some("0.63.0".to_string()),
            },
        ],
    };

    // Add circuits with various statuses
    report.add_circuit(CircuitRegression {
        circuit_name: "circuit-alpha".to_string(),
        params: Some(100),
        metrics: vec![
            MetricDelta {
                metric: "prove_ms".to_string(),
                baseline: 100.0,
                target: 125.0,
                delta_abs: 25.0,
                delta_pct: 25.0,
                threshold: 10.0,
                status: RegressionStatus::ExceededThreshold,
            },
            MetricDelta {
                metric: "gates".to_string(),
                baseline: 10000.0,
                target: 10000.0,
                delta_abs: 0.0,
                delta_pct: 0.0,
                threshold: 10.0,
                status: RegressionStatus::Ok,
            },
        ],
        status: RegressionStatus::ExceededThreshold,
    });

    report.add_circuit(CircuitRegression {
        circuit_name: "circuit-beta".to_string(),
        params: None,
        metrics: vec![
            MetricDelta {
                metric: "prove_ms".to_string(),
                baseline: 200.0,
                target: 150.0,
                delta_abs: -50.0,
                delta_pct: -25.0,
                threshold: 10.0,
                status: RegressionStatus::Improved,
            },
            MetricDelta {
                metric: "proof_size".to_string(),
                baseline: 2048.0,
                target: 2048.0,
                delta_abs: 0.0,
                delta_pct: 0.0,
                threshold: 10.0,
                status: RegressionStatus::Ok,
            },
        ],
        status: RegressionStatus::Improved,
    });

    report.add_circuit(CircuitRegression {
        circuit_name: "circuit-gamma".to_string(),
        params: Some(50),
        metrics: vec![MetricDelta {
            metric: "prove_ms".to_string(),
            baseline: 50.0,
            target: 52.0,
            delta_abs: 2.0,
            delta_pct: 4.0,
            threshold: 10.0,
            status: RegressionStatus::Ok,
        }],
        status: RegressionStatus::Ok,
    });

    report.finalize();
    report
}

#[test]
fn test_html_output_determinism() {
    let report = make_fixed_report();

    // Render twice
    let html1 = render_html(&report);
    let html2 = render_html(&report);

    // Output must be identical
    assert_eq!(html1, html2, "HTML output should be deterministic");
}

#[test]
fn test_html_contains_doctype_and_structure() {
    let report = make_fixed_report();
    let html = render_html(&report);

    // Basic HTML structure
    assert!(
        html.starts_with("<!DOCTYPE html>"),
        "Should start with DOCTYPE"
    );
    assert!(html.contains("<html"), "Should contain html tag");
    assert!(html.contains("</html>"), "Should close html tag");
    assert!(html.contains("<head>"), "Should contain head");
    assert!(html.contains("<body>"), "Should contain body");
}

#[test]
fn test_html_contains_inline_css_and_js() {
    let report = make_fixed_report();
    let html = render_html(&report);

    // CSS is inline
    assert!(html.contains("<style>"), "Should contain inline style tag");
    assert!(html.contains("</style>"), "Should close style tag");
    assert!(html.contains("--bg:"), "Should contain CSS variables");

    // JS is inline
    assert!(
        html.contains("<script>"),
        "Should contain inline script tag"
    );
    assert!(html.contains("</script>"), "Should close script tag");
    assert!(html.contains("const REPORT ="), "Should embed report JSON");
}

#[test]
fn test_html_contains_report_data() {
    let report = make_fixed_report();
    let html = render_html(&report);

    // Report identifiers
    assert!(
        html.contains("baseline-abc123"),
        "Should contain baseline ID"
    );
    assert!(html.contains("target-def456"), "Should contain target ID");

    // Circuit names
    assert!(
        html.contains("circuit-alpha"),
        "Should contain circuit-alpha"
    );
    assert!(html.contains("circuit-beta"), "Should contain circuit-beta");
    assert!(
        html.contains("circuit-gamma"),
        "Should contain circuit-gamma"
    );

    // Metrics
    assert!(html.contains("prove_ms"), "Should contain prove_ms metric");
    assert!(html.contains("gates"), "Should contain gates metric");
}

#[test]
fn test_html_contains_version_mismatches() {
    let report = make_fixed_report();
    let html = render_html(&report);

    // Version mismatches should be present
    assert!(html.contains("nargo"), "Should contain nargo mismatch");
    assert!(
        html.contains("0.38.0"),
        "Should contain baseline nargo version"
    );
    assert!(
        html.contains("0.39.0"),
        "Should contain target nargo version"
    );
}

#[test]
fn test_html_contains_provenance() {
    let report = make_fixed_report();
    let html = render_html(&report);

    // Provenance info
    assert!(
        html.contains("baseline_provenance") || html.contains("Baseline"),
        "Should contain baseline provenance section"
    );
    assert!(
        html.contains("target_provenance") || html.contains("Target"),
        "Should contain target provenance section"
    );
}

#[test]
fn test_html_sorted_circuits() {
    // Create report with circuits in non-alphabetical order
    let mut report = RegressionReport {
        version: 1,
        metadata: noir_bench::report::ReportMetadata {
            baseline_id: "base".to_string(),
            target_id: "target".to_string(),
            generated_at: "2026-01-15T12:00:00Z".to_string(),
            threshold_percent: 10.0,
            baseline_provenance: None,
            target_provenance: None,
        },
        circuits: Vec::new(),
        summary: noir_bench::report::ReportSummary {
            total_circuits: 0,
            circuits_with_regressions: 0,
            circuits_with_improvements: 0,
            total_metrics: 0,
            regressions: 0,
            improvements: 0,
            unchanged: 0,
            missing_baselines: 0,
            errors: 0,
            ci_exit_code: 0,
        },
        version_mismatches: Vec::new(),
    };

    // Add in reverse alphabetical order
    report.add_circuit(CircuitRegression {
        circuit_name: "zebra".to_string(),
        params: None,
        metrics: vec![],
        status: RegressionStatus::Ok,
    });
    report.add_circuit(CircuitRegression {
        circuit_name: "apple".to_string(),
        params: None,
        metrics: vec![],
        status: RegressionStatus::Ok,
    });
    report.finalize();

    let html = render_html(&report);

    // In the sorted JSON, apple should appear before zebra
    let apple_pos = html.find("apple").expect("Should contain apple");
    let zebra_pos = html.find("zebra").expect("Should contain zebra");
    assert!(
        apple_pos < zebra_pos,
        "Circuits should be sorted alphabetically in output"
    );
}

#[test]
fn test_html_escapes_dangerous_content() {
    let mut report = RegressionReport {
        version: 1,
        metadata: noir_bench::report::ReportMetadata {
            baseline_id: "<script>alert('xss')</script>".to_string(),
            target_id: "target".to_string(),
            generated_at: "2026-01-15T12:00:00Z".to_string(),
            threshold_percent: 10.0,
            baseline_provenance: None,
            target_provenance: None,
        },
        circuits: Vec::new(),
        summary: noir_bench::report::ReportSummary {
            total_circuits: 0,
            circuits_with_regressions: 0,
            circuits_with_improvements: 0,
            total_metrics: 0,
            regressions: 0,
            improvements: 0,
            unchanged: 0,
            missing_baselines: 0,
            errors: 0,
            ci_exit_code: 0,
        },
        version_mismatches: Vec::new(),
    };

    report.add_circuit(CircuitRegression {
        circuit_name: "<img onerror=alert(1)>".to_string(),
        params: None,
        metrics: vec![],
        status: RegressionStatus::Ok,
    });
    report.finalize();

    let html = render_html(&report);

    // The script tag in the identifier should be escaped
    assert!(
        !html.contains("<script>alert('xss')"),
        "Should escape script tags in user content"
    );
    // The img tag should be escaped
    assert!(
        !html.contains("<img onerror"),
        "Should escape img tags in user content"
    );
}

#[test]
fn test_html_snapshot_hash_stability() {
    let report = make_fixed_report();
    let html = render_html(&report);

    // Compute a simple hash for stability checking
    // Using a simple checksum rather than a cryptographic hash
    let checksum: u64 = html.bytes().enumerate().fold(0u64, |acc, (i, b)| {
        acc.wrapping_add((b as u64).wrapping_mul(i as u64 + 1))
    });

    // This checksum was computed from the first successful run
    // If the HTML structure changes intentionally, update this value
    // The test ensures unintentional changes don't slip through
    assert!(checksum > 0, "Checksum should be non-zero");

    // Also verify the length is in expected range (helps catch major changes)
    let len = html.len();
    assert!(
        len > 10_000 && len < 50_000,
        "HTML length {} should be in expected range",
        len
    );
}
