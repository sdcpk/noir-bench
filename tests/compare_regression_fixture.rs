use std::fs;

use noir_bench::{RegressionReport, compare_cmd};
use serde_json::json;

#[test]
fn test_compare_detects_synthetic_regression_and_includes_provenance() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let baseline_path = tmp.path().join("baseline.json");
    let target_path = tmp.path().join("target.json");
    let report_path = tmp.path().join("regression_report.json");

    fs::write(
        &baseline_path,
        serde_json::to_vec(&json!({
            "circuit_name": "synthetic-circuit",
            "prove_stats": { "mean_ms": 100.0 },
            "total_gates": 1000
        }))
        .expect("serialize baseline"),
    )
    .expect("write baseline");

    fs::write(
        &target_path,
        serde_json::to_vec(&json!({
            "circuit_name": "synthetic-circuit",
            "prove_stats": { "mean_ms": 115.0 },
            "total_gates": 1000
        }))
        .expect("serialize target"),
    )
    .expect("write target");

    let compare = compare_cmd::run(
        Some(baseline_path),
        Some(target_path),
        None,
        None,
        10.0,
        "json".to_string(),
        Some(report_path.clone()),
        None,
    )
    .expect("compare should succeed");

    assert_eq!(compare.total_regressions, 1);
    let circuit = compare
        .circuits
        .iter()
        .find(|c| c.circuit_name == "synthetic-circuit")
        .expect("circuit comparison exists");
    let prove_metric = circuit
        .metrics
        .iter()
        .find(|m| m.metric == "prove_ms")
        .expect("prove_ms metric exists");
    assert_eq!(prove_metric.status, compare_cmd::CompareStatus::Regression);

    let report_bytes = fs::read(&report_path).expect("read regression report");
    let report_json: serde_json::Value =
        serde_json::from_slice(&report_bytes).expect("parse regression report json");
    assert!(
        report_json["metadata"]["target_provenance"].is_object(),
        "target provenance should be present in derived regression output"
    );

    let report: RegressionReport =
        serde_json::from_slice(&report_bytes).expect("deserialize regression report");
    assert_eq!(report.summary.regressions, 1);
    assert!(
        report.metadata.target_provenance.is_some(),
        "target provenance should be populated"
    );
}
