//! Compare benchmark results for regression detection.
//!
//! Supports comparing single JSON reports or JSONL files containing multiple records.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::engine::provenance;
use crate::report::{
    CircuitRegression, MetricDelta, RegressionReport, RegressionStatus,
    render_markdown as report_render_markdown, write_html as report_write_html,
};
use crate::{BenchError, BenchResult, JsonlWriter};

/// Default regression threshold percentage
pub const DEFAULT_THRESHOLD: f64 = 10.0;

/// Comparison of a single metric
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricComparison {
    pub metric: String,
    pub baseline: f64,
    pub target: f64,
    pub delta: f64,
    pub percent: f64,
    pub status: CompareStatus,
}

/// Status of a metric comparison
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CompareStatus {
    Regression,
    Improvement,
    Unchanged,
}

impl CompareStatus {
    pub fn emoji(&self) -> &'static str {
        match self {
            CompareStatus::Regression => "🔴",
            CompareStatus::Improvement => "🟢",
            CompareStatus::Unchanged => "⚪",
        }
    }
}

/// Comparison results for a single circuit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitComparison {
    pub circuit_name: String,
    pub metrics: Vec<MetricComparison>,
    pub has_regression: bool,
}

/// Full comparison result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompareResult {
    pub baseline_ref: String,
    pub target_ref: String,
    pub threshold: f64,
    pub circuits: Vec<CircuitComparison>,
    pub total_regressions: usize,
    pub total_improvements: usize,
    pub ci_exit_code: i32,
}

/// Metrics to compare with their display names and whether higher is worse
const METRIC_DEFS: &[(&str, &str, bool)] = &[
    ("prove_time_ms", "prove_ms", true),
    ("prove_stats.mean_ms", "prove_ms", true),
    ("witness_gen_time_ms", "witness_ms", true),
    ("witness_stats.mean_ms", "witness_ms", true),
    ("verify_time_ms", "verify_ms", true),
    ("verify_stats.mean_ms", "verify_ms", true),
    ("backend_prove_time_ms", "backend_ms", true),
    ("execution_time_ms", "exec_ms", true),
    ("total_gates", "gates", true),
    ("proof_size_bytes", "proof_size", true),
    ("peak_memory_bytes", "peak_mem", true),
    ("peak_rss_mb", "peak_rss_mb", true),
    ("proving_key_size_bytes", "pk_size", false),
    ("verification_key_size_bytes", "vk_size", false),
];

fn get_nested_num(v: &Value, path: &str) -> Option<f64> {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = v;
    for part in &parts[..parts.len() - 1] {
        current = current.get(*part)?;
    }
    let final_key = parts.last()?;
    current
        .get(*final_key)
        .and_then(|x| x.as_f64().or_else(|| x.as_u64().map(|u| u as f64)))
}

fn get_circuit_name(v: &Value) -> Option<String> {
    v.get("circuit_name")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            // Fall back to artifact_path or name field
            v.get("name")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
        })
        .or_else(|| {
            v.get("artifact_path").and_then(|x| x.as_str()).map(|s| {
                std::path::Path::new(s)
                    .file_stem()
                    .and_then(|os| os.to_str())
                    .unwrap_or("unknown")
                    .to_string()
            })
        })
}

fn compare_values(baseline: &Value, target: &Value, threshold: f64) -> Vec<MetricComparison> {
    let mut results = Vec::new();
    let mut seen_metrics = std::collections::HashSet::new();

    for (json_path, display_name, higher_is_worse) in METRIC_DEFS {
        // Skip if we've already seen this display name
        if seen_metrics.contains(*display_name) {
            continue;
        }

        if let (Some(bv), Some(tv)) = (
            get_nested_num(baseline, json_path),
            get_nested_num(target, json_path),
        ) {
            seen_metrics.insert(*display_name);

            let delta = tv - bv;
            let percent = if bv != 0.0 { delta * 100.0 / bv } else { 0.0 };

            let status = if *higher_is_worse {
                if percent > threshold {
                    CompareStatus::Regression
                } else if percent < -threshold {
                    CompareStatus::Improvement
                } else {
                    CompareStatus::Unchanged
                }
            } else {
                // For metrics where lower is worse (like key sizes - informational only)
                CompareStatus::Unchanged
            };

            results.push(MetricComparison {
                metric: display_name.to_string(),
                baseline: bv,
                target: tv,
                delta,
                percent,
                status,
            });
        }
    }

    results
}

fn compare_single_records(baseline: &Value, target: &Value, threshold: f64) -> CircuitComparison {
    let circuit_name = get_circuit_name(baseline)
        .or_else(|| get_circuit_name(target))
        .unwrap_or_else(|| "unknown".to_string());

    let metrics = compare_values(baseline, target, threshold);
    let has_regression = metrics
        .iter()
        .any(|m| m.status == CompareStatus::Regression);

    CircuitComparison {
        circuit_name,
        metrics,
        has_regression,
    }
}

/// Compare JSONL files by matching records with the same circuit_name
fn compare_jsonl_files(
    baseline_path: &PathBuf,
    target_path: &PathBuf,
    threshold: f64,
) -> BenchResult<Vec<CircuitComparison>> {
    let baseline_reader = JsonlWriter::new(baseline_path);
    let target_reader = JsonlWriter::new(target_path);

    let baseline_records = baseline_reader.read_all()?;
    let target_records = target_reader.read_all()?;

    // Index baseline records by circuit_name
    let mut baseline_map: HashMap<String, Value> = HashMap::new();
    for record in baseline_records {
        let json = serde_json::to_value(&record)
            .map_err(|e| BenchError::Message(format!("failed to serialize record: {e}")))?;
        baseline_map.insert(record.circuit_name.clone(), json);
    }

    // Compare each target record against its baseline
    let mut comparisons = Vec::new();
    for record in target_records {
        let target_json = serde_json::to_value(&record)
            .map_err(|e| BenchError::Message(format!("failed to serialize record: {e}")))?;

        if let Some(baseline_json) = baseline_map.get(&record.circuit_name) {
            let comparison = compare_single_records(baseline_json, &target_json, threshold);
            comparisons.push(comparison);
        } else {
            // New circuit in target, no baseline to compare
            let metrics = compare_values(&Value::Null, &target_json, threshold);
            comparisons.push(CircuitComparison {
                circuit_name: record.circuit_name,
                metrics,
                has_regression: false,
            });
        }
    }

    Ok(comparisons)
}

/// Compare single JSON files
fn compare_json_files(
    baseline_path: &PathBuf,
    target_path: &PathBuf,
    threshold: f64,
) -> BenchResult<Vec<CircuitComparison>> {
    let b = std::fs::read(baseline_path).map_err(|e| BenchError::Message(e.to_string()))?;
    let t = std::fs::read(target_path).map_err(|e| BenchError::Message(e.to_string()))?;
    let baseline: Value =
        serde_json::from_slice(&b).map_err(|e| BenchError::Message(e.to_string()))?;
    let target: Value =
        serde_json::from_slice(&t).map_err(|e| BenchError::Message(e.to_string()))?;

    let comparison = compare_single_records(&baseline, &target, threshold);
    Ok(vec![comparison])
}

fn format_value(value: f64, metric: &str) -> String {
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
        format!("{:.0}", value)
    }
}

fn format_text(result: &CompareResult) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Comparing: {} vs {} (threshold: {:.1}%)\n\n",
        result.baseline_ref, result.target_ref, result.threshold
    ));

    for circuit in &result.circuits {
        out.push_str(&format!("Circuit: {}\n", circuit.circuit_name));
        for m in &circuit.metrics {
            let status_str = match m.status {
                CompareStatus::Regression => "[REGRESS]",
                CompareStatus::Improvement => "[IMPROVE]",
                CompareStatus::Unchanged => "[OK]",
            };
            out.push_str(&format!(
                "  {}: {} -> {} ({:+.2}%) {}\n",
                m.metric,
                format_value(m.baseline, &m.metric),
                format_value(m.target, &m.metric),
                m.percent,
                status_str
            ));
        }
        out.push('\n');
    }

    if result.total_regressions > 0 {
        out.push_str(&format!(
            "Result: {} regression(s) detected\n",
            result.total_regressions
        ));
    } else {
        out.push_str("Result: No regressions detected\n");
    }

    out
}

fn format_json(result: &CompareResult) -> String {
    serde_json::to_string_pretty(result).unwrap_or_else(|_| "{}".to_string())
}

/// Configuration for the compare command
pub struct CompareConfig {
    pub baseline_file: Option<PathBuf>,
    pub target_file: Option<PathBuf>,
    pub baseline_json: Option<PathBuf>,
    pub target_json: Option<PathBuf>,
    pub threshold: f64,
    pub format: String,
    pub json_out: Option<PathBuf>,
}

/// Convert CompareResult to RegressionReport for JSON output.
pub fn to_regression_report(result: &CompareResult) -> RegressionReport {
    let mut report =
        RegressionReport::new(&result.baseline_ref, &result.target_ref, result.threshold);

    for circuit in &result.circuits {
        let metrics: Vec<MetricDelta> = circuit
            .metrics
            .iter()
            .map(|m| {
                let status = match m.status {
                    CompareStatus::Regression => RegressionStatus::ExceededThreshold,
                    CompareStatus::Improvement => RegressionStatus::Improved,
                    CompareStatus::Unchanged => RegressionStatus::Ok,
                };

                MetricDelta {
                    metric: m.metric.clone(),
                    baseline: m.baseline,
                    target: m.target,
                    delta_abs: m.delta,
                    delta_pct: m.percent,
                    threshold: result.threshold,
                    status,
                }
            })
            .collect();

        let circuit_status = if circuit.has_regression {
            RegressionStatus::ExceededThreshold
        } else if metrics
            .iter()
            .any(|m| m.status == RegressionStatus::Improved)
        {
            RegressionStatus::Improved
        } else {
            RegressionStatus::Ok
        };

        report.add_circuit(CircuitRegression {
            circuit_name: circuit.circuit_name.clone(),
            params: None,
            metrics,
            status: circuit_status,
        });
    }

    report.finalize();
    report
}

/// Run comparison and return result
pub fn compare(config: &CompareConfig) -> BenchResult<CompareResult> {
    let (circuits, baseline_ref, target_ref) = if let (Some(baseline), Some(target)) =
        (&config.baseline_file, &config.target_file)
    {
        // JSONL comparison
        let circuits = compare_jsonl_files(baseline, target, config.threshold)?;
        let baseline_ref = baseline
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("baseline")
            .to_string();
        let target_ref = target
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("target")
            .to_string();
        (circuits, baseline_ref, target_ref)
    } else if let (Some(baseline), Some(target)) = (&config.baseline_json, &config.target_json) {
        // Single JSON comparison (legacy)
        let circuits = compare_json_files(baseline, target, config.threshold)?;
        let baseline_ref = baseline
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("baseline")
            .to_string();
        let target_ref = target
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("target")
            .to_string();
        (circuits, baseline_ref, target_ref)
    } else {
        return Err(BenchError::Message(
            "must provide either --baseline-file/--target-file or --baseline/--contender".into(),
        ));
    };

    let total_regressions = circuits
        .iter()
        .flat_map(|c| &c.metrics)
        .filter(|m| m.status == CompareStatus::Regression)
        .count();

    let total_improvements = circuits
        .iter()
        .flat_map(|c| &c.metrics)
        .filter(|m| m.status == CompareStatus::Improvement)
        .count();

    let ci_exit_code = if total_regressions > 0 { 1 } else { 0 };

    Ok(CompareResult {
        baseline_ref,
        target_ref,
        threshold: config.threshold,
        circuits,
        total_regressions,
        total_improvements,
        ci_exit_code,
    })
}

/// Main entry point for the compare command
pub fn run(
    baseline: Option<PathBuf>,
    contender: Option<PathBuf>,
    baseline_file: Option<PathBuf>,
    target_file: Option<PathBuf>,
    threshold: f64,
    format: String,
    json_out: Option<PathBuf>,
    html_out: Option<PathBuf>,
) -> BenchResult<CompareResult> {
    let config = CompareConfig {
        baseline_file,
        target_file,
        baseline_json: baseline,
        target_json: contender,
        threshold,
        format: format.clone(),
        json_out: json_out.clone(),
    };

    let result = compare(&config)?;

    // Collect provenance once for reuse
    let target_provenance = provenance::collect(None);

    // Write RegressionReport JSON if requested
    if let Some(ref json_path) = json_out {
        let mut regression_report = to_regression_report(&result);
        regression_report.set_provenance(None, Some(target_provenance.clone()));

        let json_str = serde_json::to_string_pretty(&regression_report).map_err(|e| {
            BenchError::Message(format!("failed to serialize regression report: {e}"))
        })?;
        std::fs::write(json_path, json_str).map_err(|e| {
            BenchError::Message(format!("failed to write {}: {e}", json_path.display()))
        })?;
        eprintln!("Wrote regression report to {}", json_path.display());
    }

    // Write HTML report if requested
    if let Some(ref html_path) = html_out {
        let mut regression_report = to_regression_report(&result);
        regression_report.set_provenance(None, Some(target_provenance.clone()));

        report_write_html(html_path, &regression_report)
            .map_err(|e| BenchError::Message(format!("failed to write HTML report: {e}")))?;
        eprintln!("Wrote HTML report to {}", html_path.display());
    }

    let output = match format.as_str() {
        "json" => format_json(&result),
        "markdown" | "md" => {
            // Use the new report markdown renderer for better output
            let mut regression_report = to_regression_report(&result);
            regression_report.set_provenance(None, Some(target_provenance));
            report_render_markdown(&regression_report)
        }
        _ => format_text(&result),
    };

    print!("{}", output);

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compare_status_emoji() {
        assert_eq!(CompareStatus::Regression.emoji(), "🔴");
        assert_eq!(CompareStatus::Improvement.emoji(), "🟢");
        assert_eq!(CompareStatus::Unchanged.emoji(), "⚪");
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
    fn test_compare_values_regression() {
        let baseline = serde_json::json!({
            "prove_time_ms": 100.0,
            "total_gates": 1000
        });
        let target = serde_json::json!({
            "prove_time_ms": 120.0,  // 20% increase
            "total_gates": 1000
        });

        let results = compare_values(&baseline, &target, 10.0);

        let prove_metric = results.iter().find(|m| m.metric == "prove_ms").unwrap();
        assert_eq!(prove_metric.status, CompareStatus::Regression);
        assert!((prove_metric.percent - 20.0).abs() < 0.01);
    }

    #[test]
    fn test_compare_values_improvement() {
        let baseline = serde_json::json!({
            "prove_time_ms": 100.0
        });
        let target = serde_json::json!({
            "prove_time_ms": 80.0  // 20% decrease
        });

        let results = compare_values(&baseline, &target, 10.0);

        let prove_metric = results.iter().find(|m| m.metric == "prove_ms").unwrap();
        assert_eq!(prove_metric.status, CompareStatus::Improvement);
    }

    #[test]
    fn test_compare_values_unchanged() {
        let baseline = serde_json::json!({
            "prove_time_ms": 100.0
        });
        let target = serde_json::json!({
            "prove_time_ms": 105.0  // 5% increase, below threshold
        });

        let results = compare_values(&baseline, &target, 10.0);

        let prove_metric = results.iter().find(|m| m.metric == "prove_ms").unwrap();
        assert_eq!(prove_metric.status, CompareStatus::Unchanged);
    }

    #[test]
    fn test_default_threshold() {
        assert_eq!(DEFAULT_THRESHOLD, 10.0);
    }
}
