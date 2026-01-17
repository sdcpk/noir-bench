//! Regression report structure for CI/CD pipelines.
//!
//! This module defines a stable `RegressionReport` schema that can be:
//! - Serialized to JSON for machine consumption
//! - Rendered to Markdown for PR comments
//! - Used to determine CI exit codes

use serde::{Deserialize, Serialize};

use crate::engine::provenance::{Provenance, VersionMismatch};

/// Schema version for RegressionReport
pub const REGRESSION_REPORT_VERSION: u32 = 1;

/// A complete regression report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegressionReport {
    /// Schema version for forward compatibility
    pub version: u32,
    /// Report metadata
    pub metadata: ReportMetadata,
    /// Per-circuit regression analysis
    pub circuits: Vec<CircuitRegression>,
    /// Summary statistics
    pub summary: ReportSummary,
    /// Tool version mismatches between baseline and target
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub version_mismatches: Vec<VersionMismatch>,
}

/// Metadata about the regression report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportMetadata {
    /// Identifier for baseline (e.g., filename, git SHA, or "main")
    pub baseline_id: String,
    /// Identifier for target (e.g., filename, git SHA, or PR number)
    pub target_id: String,
    /// ISO 8601 timestamp when report was generated
    pub generated_at: String,
    /// Regression threshold percentage used
    pub threshold_percent: f64,
    /// Baseline provenance (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline_provenance: Option<Provenance>,
    /// Target provenance (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_provenance: Option<Provenance>,
}

/// Regression analysis for a single circuit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitRegression {
    /// Circuit name
    pub circuit_name: String,
    /// Optional circuit parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<u64>,
    /// Per-metric deltas
    pub metrics: Vec<MetricDelta>,
    /// Overall status for this circuit
    pub status: RegressionStatus,
}

/// Delta analysis for a single metric.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricDelta {
    /// Metric name (e.g., "prove_ms", "gates", "proof_size")
    pub metric: String,
    /// Baseline value
    pub baseline: f64,
    /// Target value
    pub target: f64,
    /// Absolute delta (target - baseline)
    pub delta_abs: f64,
    /// Percentage change ((target - baseline) / baseline * 100)
    pub delta_pct: f64,
    /// Threshold that was applied
    pub threshold: f64,
    /// Status for this metric
    pub status: RegressionStatus,
}

/// Status of a regression check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegressionStatus {
    /// Value increased beyond threshold (regression)
    ExceededThreshold,
    /// Value decreased beyond threshold (improvement)
    Improved,
    /// Value within threshold (no significant change)
    Ok,
    /// Baseline data missing for comparison
    MissingBaseline,
    /// Error during comparison
    Error,
    /// Metric was skipped (e.g., not available)
    Skipped,
}

impl RegressionStatus {
    /// Get emoji representation for markdown.
    pub fn emoji(&self) -> &'static str {
        match self {
            RegressionStatus::ExceededThreshold => "🔴",
            RegressionStatus::Improved => "🟢",
            RegressionStatus::Ok => "⚪",
            RegressionStatus::MissingBaseline => "⚠️",
            RegressionStatus::Error => "❌",
            RegressionStatus::Skipped => "⏭️",
        }
    }

    /// Get short text label.
    pub fn label(&self) -> &'static str {
        match self {
            RegressionStatus::ExceededThreshold => "REGRESS",
            RegressionStatus::Improved => "IMPROVED",
            RegressionStatus::Ok => "OK",
            RegressionStatus::MissingBaseline => "NO_BASE",
            RegressionStatus::Error => "ERROR",
            RegressionStatus::Skipped => "SKIP",
        }
    }

    /// Is this status a failure for CI purposes?
    pub fn is_failure(&self) -> bool {
        matches!(
            self,
            RegressionStatus::ExceededThreshold | RegressionStatus::Error
        )
    }
}

/// Summary statistics for the report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportSummary {
    /// Total circuits analyzed
    pub total_circuits: usize,
    /// Circuits with regressions
    pub circuits_with_regressions: usize,
    /// Circuits with improvements
    pub circuits_with_improvements: usize,
    /// Total metric comparisons
    pub total_metrics: usize,
    /// Metrics that exceeded threshold
    pub regressions: usize,
    /// Metrics that improved
    pub improvements: usize,
    /// Metrics that were OK
    pub unchanged: usize,
    /// Metrics with missing baselines
    pub missing_baselines: usize,
    /// Metrics with errors
    pub errors: usize,
    /// Recommended CI exit code (0 = pass, 1 = regressions)
    pub ci_exit_code: i32,
}

impl RegressionReport {
    /// Create a new regression report.
    pub fn new(
        baseline_id: impl Into<String>,
        target_id: impl Into<String>,
        threshold_percent: f64,
    ) -> Self {
        let generated_at = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();

        RegressionReport {
            version: REGRESSION_REPORT_VERSION,
            metadata: ReportMetadata {
                baseline_id: baseline_id.into(),
                target_id: target_id.into(),
                generated_at,
                threshold_percent,
                baseline_provenance: None,
                target_provenance: None,
            },
            circuits: Vec::new(),
            summary: ReportSummary {
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
        }
    }

    /// Add a circuit regression result.
    pub fn add_circuit(&mut self, circuit: CircuitRegression) {
        // Update summary
        self.summary.total_circuits += 1;

        let has_regression = circuit
            .metrics
            .iter()
            .any(|m| m.status == RegressionStatus::ExceededThreshold);
        let has_improvement = circuit
            .metrics
            .iter()
            .any(|m| m.status == RegressionStatus::Improved);

        if has_regression {
            self.summary.circuits_with_regressions += 1;
        }
        if has_improvement {
            self.summary.circuits_with_improvements += 1;
        }

        for metric in &circuit.metrics {
            self.summary.total_metrics += 1;
            match metric.status {
                RegressionStatus::ExceededThreshold => self.summary.regressions += 1,
                RegressionStatus::Improved => self.summary.improvements += 1,
                RegressionStatus::Ok => self.summary.unchanged += 1,
                RegressionStatus::MissingBaseline => self.summary.missing_baselines += 1,
                RegressionStatus::Error => self.summary.errors += 1,
                RegressionStatus::Skipped => {}
            }
        }

        self.circuits.push(circuit);
    }

    /// Finalize the report and compute exit code.
    pub fn finalize(&mut self) {
        self.summary.ci_exit_code = if self.summary.regressions > 0 || self.summary.errors > 0 {
            1
        } else {
            0
        };
    }

    /// Set provenance information.
    pub fn set_provenance(&mut self, baseline: Option<Provenance>, target: Option<Provenance>) {
        if let (Some(b), Some(t)) = (&baseline, &target) {
            self.version_mismatches = crate::engine::provenance::check_version_mismatches(b, t);
        }
        self.metadata.baseline_provenance = baseline;
        self.metadata.target_provenance = target;
    }
}

/// Compute delta status based on threshold.
///
/// For metrics where higher is worse (time, memory, gates), a positive delta
/// exceeding threshold is a regression.
pub fn compute_delta_status(
    baseline: f64,
    target: f64,
    threshold_pct: f64,
    higher_is_worse: bool,
) -> (f64, f64, RegressionStatus) {
    let delta_abs = target - baseline;
    let delta_pct = if baseline != 0.0 {
        delta_abs * 100.0 / baseline
    } else {
        0.0
    };

    let status = if higher_is_worse {
        if delta_pct > threshold_pct {
            RegressionStatus::ExceededThreshold
        } else if delta_pct < -threshold_pct {
            RegressionStatus::Improved
        } else {
            RegressionStatus::Ok
        }
    } else {
        // For metrics where lower is worse (informational only)
        RegressionStatus::Ok
    };

    (delta_abs, delta_pct, status)
}

/// Format a numeric value for display.
pub fn format_value(value: f64, metric: &str) -> String {
    if metric.contains("size") || metric.contains("mem") || metric.contains("rss") {
        if metric.contains("rss_mb") {
            format!("{:.1} MB", value)
        } else if value >= 1_000_000_000.0 {
            format!("{:.1} GB", value / 1_000_000_000.0)
        } else if value >= 1_000_000.0 {
            format!("{:.1} MB", value / 1_000_000.0)
        } else if value >= 1_000.0 {
            format!("{:.1} KB", value / 1_000.0)
        } else {
            format!("{:.0} B", value)
        }
    } else if metric.contains("ms") {
        if value >= 1000.0 {
            format!("{:.2}s", value / 1000.0)
        } else {
            format!("{:.0}ms", value)
        }
    } else if metric.contains("gates") {
        if value >= 1_000_000.0 {
            format!("{:.2}M", value / 1_000_000.0)
        } else if value >= 1_000.0 {
            format!("{:.1}K", value / 1_000.0)
        } else {
            format!("{:.0}", value)
        }
    } else {
        format!("{:.2}", value)
    }
}

/// Render regression report as Markdown for PR comments.
pub fn render_markdown(report: &RegressionReport) -> String {
    let mut out = String::new();

    // Header with status
    let status_emoji = if report.summary.regressions > 0 {
        "❌"
    } else if report.summary.improvements > 0 {
        "✅🎉"
    } else {
        "✅"
    };

    out.push_str(&format!(
        "## {} noir-bench Regression Report\n\n",
        status_emoji
    ));

    // Metadata
    out.push_str(&format!(
        "| | |\n|---|---|\n\
         | **Baseline** | `{}` |\n\
         | **Target** | `{}` |\n\
         | **Threshold** | {:.1}% |\n\
         | **Generated** | {} |\n\n",
        report.metadata.baseline_id,
        report.metadata.target_id,
        report.metadata.threshold_percent,
        &report.metadata.generated_at[..19].replace('T', " ")
    ));

    // Version mismatch warnings
    if !report.version_mismatches.is_empty() {
        out.push_str("### ⚠️ Tool Version Mismatches\n\n");
        out.push_str("| Tool | Baseline | Target |\n|------|----------|--------|\n");
        for m in &report.version_mismatches {
            out.push_str(&format!(
                "| {} | {} | {} |\n",
                m.tool,
                m.baseline_version.as_deref().unwrap_or("-"),
                m.target_version.as_deref().unwrap_or("-")
            ));
        }
        out.push_str("\n");
    }

    // Summary box
    out.push_str("### Summary\n\n");
    out.push_str(&format!(
        "| Metric | Count |\n|--------|-------|\n\
         | Circuits | {} |\n\
         | Regressions | {} |\n\
         | Improvements | {} |\n\
         | Unchanged | {} |\n\n",
        report.summary.total_circuits,
        report.summary.regressions,
        report.summary.improvements,
        report.summary.unchanged
    ));

    // Group regressions by metric
    if report.summary.regressions > 0 {
        out.push_str("### 🔴 Regressions\n\n");
        out.push_str("| Circuit | Metric | Baseline | Target | Delta | Status |\n");
        out.push_str("|---------|--------|----------|--------|-------|--------|\n");

        for circuit in &report.circuits {
            for metric in &circuit.metrics {
                if metric.status == RegressionStatus::ExceededThreshold {
                    out.push_str(&format!(
                        "| {} | {} | {} | {} | {:+.1}% | {} |\n",
                        circuit.circuit_name,
                        metric.metric,
                        format_value(metric.baseline, &metric.metric),
                        format_value(metric.target, &metric.metric),
                        metric.delta_pct,
                        metric.status.emoji()
                    ));
                }
            }
        }
        out.push_str("\n");
    }

    // Improvements (collapsed if many)
    if report.summary.improvements > 0 {
        out.push_str("### 🟢 Improvements\n\n");
        if report.summary.improvements > 5 {
            out.push_str("<details>\n<summary>Show all improvements</summary>\n\n");
        }
        out.push_str("| Circuit | Metric | Baseline | Target | Delta | Status |\n");
        out.push_str("|---------|--------|----------|--------|-------|--------|\n");

        for circuit in &report.circuits {
            for metric in &circuit.metrics {
                if metric.status == RegressionStatus::Improved {
                    out.push_str(&format!(
                        "| {} | {} | {} | {} | {:+.1}% | {} |\n",
                        circuit.circuit_name,
                        metric.metric,
                        format_value(metric.baseline, &metric.metric),
                        format_value(metric.target, &metric.metric),
                        metric.delta_pct,
                        metric.status.emoji()
                    ));
                }
            }
        }

        if report.summary.improvements > 5 {
            out.push_str("\n</details>\n");
        }
        out.push_str("\n");
    }

    // Full results table (collapsed)
    out.push_str("<details>\n<summary>All Results</summary>\n\n");
    out.push_str("| Circuit | Metric | Baseline | Target | Delta | Status |\n");
    out.push_str("|---------|--------|----------|--------|-------|--------|\n");

    for circuit in &report.circuits {
        for (i, metric) in circuit.metrics.iter().enumerate() {
            let circuit_col = if i == 0 { &circuit.circuit_name } else { "" };
            let delta_str = if metric.delta_abs == 0.0 {
                "0".to_string()
            } else {
                format!("{:+.1}%", metric.delta_pct)
            };
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} |\n",
                circuit_col,
                metric.metric,
                format_value(metric.baseline, &metric.metric),
                format_value(metric.target, &metric.metric),
                delta_str,
                metric.status.emoji()
            ));
        }
    }
    out.push_str("\n</details>\n\n");

    // Legend
    out.push_str("---\n");
    out.push_str("🔴 = regression (>{:.1}%) | 🟢 = improvement (<-{:.1}%) | ⚪ = unchanged\n");
    out = out.replace(
        "{:.1}",
        &format!("{:.1}", report.metadata.threshold_percent),
    );

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_delta_status_regression() {
        let (delta_abs, delta_pct, status) = compute_delta_status(100.0, 120.0, 10.0, true);
        assert_eq!(delta_abs, 20.0);
        assert!((delta_pct - 20.0).abs() < 0.01);
        assert_eq!(status, RegressionStatus::ExceededThreshold);
    }

    #[test]
    fn test_compute_delta_status_improvement() {
        let (delta_abs, delta_pct, status) = compute_delta_status(100.0, 80.0, 10.0, true);
        assert_eq!(delta_abs, -20.0);
        assert!((delta_pct - (-20.0)).abs() < 0.01);
        assert_eq!(status, RegressionStatus::Improved);
    }

    #[test]
    fn test_compute_delta_status_ok() {
        let (_, delta_pct, status) = compute_delta_status(100.0, 105.0, 10.0, true);
        assert!((delta_pct - 5.0).abs() < 0.01);
        assert_eq!(status, RegressionStatus::Ok);
    }

    #[test]
    fn test_compute_delta_status_zero_baseline() {
        let (delta_abs, delta_pct, status) = compute_delta_status(0.0, 100.0, 10.0, true);
        assert_eq!(delta_abs, 100.0);
        assert_eq!(delta_pct, 0.0); // Avoid division by zero
        assert_eq!(status, RegressionStatus::Ok);
    }

    #[test]
    fn test_regression_status_emoji() {
        assert_eq!(RegressionStatus::ExceededThreshold.emoji(), "🔴");
        assert_eq!(RegressionStatus::Improved.emoji(), "🟢");
        assert_eq!(RegressionStatus::Ok.emoji(), "⚪");
    }

    #[test]
    fn test_regression_status_is_failure() {
        assert!(RegressionStatus::ExceededThreshold.is_failure());
        assert!(RegressionStatus::Error.is_failure());
        assert!(!RegressionStatus::Ok.is_failure());
        assert!(!RegressionStatus::Improved.is_failure());
    }

    #[test]
    fn test_regression_report_new() {
        let report = RegressionReport::new("baseline.jsonl", "target.jsonl", 10.0);
        assert_eq!(report.version, REGRESSION_REPORT_VERSION);
        assert_eq!(report.metadata.baseline_id, "baseline.jsonl");
        assert_eq!(report.metadata.target_id, "target.jsonl");
        assert_eq!(report.metadata.threshold_percent, 10.0);
    }

    #[test]
    fn test_regression_report_add_circuit() {
        let mut report = RegressionReport::new("base", "target", 10.0);

        let circuit = CircuitRegression {
            circuit_name: "test-circuit".to_string(),
            params: None,
            metrics: vec![MetricDelta {
                metric: "prove_ms".to_string(),
                baseline: 100.0,
                target: 120.0,
                delta_abs: 20.0,
                delta_pct: 20.0,
                threshold: 10.0,
                status: RegressionStatus::ExceededThreshold,
            }],
            status: RegressionStatus::ExceededThreshold,
        };

        report.add_circuit(circuit);
        report.finalize();

        assert_eq!(report.summary.total_circuits, 1);
        assert_eq!(report.summary.regressions, 1);
        assert_eq!(report.summary.ci_exit_code, 1);
    }

    #[test]
    fn test_regression_report_serialization() {
        let mut report = RegressionReport::new("base", "target", 10.0);
        report.add_circuit(CircuitRegression {
            circuit_name: "test".to_string(),
            params: None,
            metrics: vec![MetricDelta {
                metric: "gates".to_string(),
                baseline: 1000.0,
                target: 1050.0,
                delta_abs: 50.0,
                delta_pct: 5.0,
                threshold: 10.0,
                status: RegressionStatus::Ok,
            }],
            status: RegressionStatus::Ok,
        });
        report.finalize();

        let json = serde_json::to_string_pretty(&report);
        assert!(json.is_ok());

        let json_str = json.unwrap();
        let deserialized: Result<RegressionReport, _> = serde_json::from_str(&json_str);
        assert!(deserialized.is_ok());

        let report2 = deserialized.unwrap();
        assert_eq!(
            report.summary.total_circuits,
            report2.summary.total_circuits
        );
    }

    #[test]
    fn test_format_value_time() {
        assert_eq!(format_value(100.0, "prove_ms"), "100ms");
        assert_eq!(format_value(1500.0, "prove_ms"), "1.50s");
    }

    #[test]
    fn test_format_value_size() {
        assert_eq!(format_value(500.0, "proof_size"), "500 B");
        assert_eq!(format_value(1024.0, "proof_size"), "1.0 KB");
        assert_eq!(format_value(1048576.0, "proof_size"), "1.0 MB");
    }

    #[test]
    fn test_format_value_gates() {
        assert_eq!(format_value(500.0, "gates"), "500");
        assert_eq!(format_value(1500.0, "gates"), "1.5K");
        assert_eq!(format_value(1500000.0, "gates"), "1.50M");
    }

    #[test]
    fn test_render_markdown_contains_headers() {
        let mut report = RegressionReport::new("base", "target", 10.0);
        report.add_circuit(CircuitRegression {
            circuit_name: "test".to_string(),
            params: None,
            metrics: vec![],
            status: RegressionStatus::Ok,
        });
        report.finalize();

        let md = render_markdown(&report);

        assert!(md.contains("noir-bench Regression Report"));
        assert!(md.contains("Baseline"));
        assert!(md.contains("Target"));
        assert!(md.contains("Threshold"));
        assert!(md.contains("Summary"));
    }

    #[test]
    fn test_render_markdown_shows_regressions() {
        let mut report = RegressionReport::new("base", "target", 10.0);
        report.add_circuit(CircuitRegression {
            circuit_name: "slow-circuit".to_string(),
            params: None,
            metrics: vec![MetricDelta {
                metric: "prove_ms".to_string(),
                baseline: 100.0,
                target: 150.0,
                delta_abs: 50.0,
                delta_pct: 50.0,
                threshold: 10.0,
                status: RegressionStatus::ExceededThreshold,
            }],
            status: RegressionStatus::ExceededThreshold,
        });
        report.finalize();

        let md = render_markdown(&report);

        assert!(md.contains("Regressions"));
        assert!(md.contains("slow-circuit"));
        assert!(md.contains("prove_ms"));
        assert!(md.contains("🔴"));
    }

    #[test]
    fn test_render_markdown_shows_version_mismatches() {
        let mut report = RegressionReport::new("base", "target", 10.0);
        report.version_mismatches.push(VersionMismatch {
            tool: "nargo".to_string(),
            baseline_version: Some("0.38.0".to_string()),
            target_version: Some("0.39.0".to_string()),
        });
        report.finalize();

        let md = render_markdown(&report);

        assert!(md.contains("Version Mismatches"));
        assert!(md.contains("nargo"));
        assert!(md.contains("0.38.0"));
        assert!(md.contains("0.39.0"));
    }
}
