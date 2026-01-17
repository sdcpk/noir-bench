//! CI command for running benchmarks in CI/CD pipelines.
//!
//! This command runs a subset of benchmarks, compares against a baseline,
//! and outputs results suitable for CI environments.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::backend::{BarretenbergBackend, BarretenbergConfig};
use crate::compare_cmd::{self, CompareResult, DEFAULT_THRESHOLD, to_regression_report};
use crate::engine::provenance;
use crate::engine::{NargoToolchain, ProveInputs, full_benchmark};
use crate::report::{render_markdown as report_render_markdown, write_html as report_write_html};
use crate::{BenchError, BenchResult};

const DEFAULT_CONFIG: &str = "bench-config.toml";
const DEFAULT_BASELINE: &str = ".noir-bench-baseline.jsonl";
const DEFAULT_CI_ITERATIONS: usize = 3;
const DEFAULT_CI_WARMUP: usize = 1;

/// CI-specific configuration from bench-config.toml
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CiConfig {
    /// Circuits to run in CI (subset of full benchmark suite)
    #[serde(default)]
    pub circuits: Vec<String>,
    /// Path to baseline JSONL file
    #[serde(default)]
    pub baseline_file: Option<String>,
    /// Regression threshold percentage
    #[serde(default)]
    pub threshold_percent: Option<f64>,
    /// Number of iterations for CI runs
    #[serde(default)]
    pub iterations: Option<usize>,
    /// Number of warmup iterations
    #[serde(default)]
    pub warmup: Option<usize>,
}

/// Full config including CI section
#[derive(Debug, Deserialize)]
struct FullConfig {
    #[serde(default)]
    pub ci: Option<CiConfig>,
    #[serde(rename = "circuit", default)]
    pub circuits: Vec<RawCircuit>,
}

#[derive(Debug, Deserialize)]
struct RawCircuit {
    pub name: String,
    pub path: PathBuf,
    #[serde(default)]
    pub params: Option<Vec<u64>>,
}

/// CI run result for a single circuit
#[derive(Debug, Clone, Serialize)]
pub struct CiCircuitResult {
    pub circuit_name: String,
    pub params: Option<u64>,
    pub prove_ms: f64,
    pub gates: Option<u64>,
    pub proof_size_bytes: Option<u64>,
    pub status: String,
}

/// Full CI run result
#[derive(Debug, Clone, Serialize)]
pub struct CiRunResult {
    pub timestamp: String,
    pub circuits: Vec<CiCircuitResult>,
    pub comparison: Option<CompareResult>,
    pub exit_code: i32,
}

fn now_string() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

/// Load CI config from bench-config.toml
fn load_ci_config(
    path: &PathBuf,
) -> BenchResult<(CiConfig, Vec<(String, PathBuf, Option<Vec<u64>>)>)> {
    let s = std::fs::read_to_string(path)
        .map_err(|e| BenchError::Message(format!("failed to read config: {e}")))?;
    let cfg: FullConfig = toml::from_str(&s)
        .map_err(|e| BenchError::Message(format!("failed to parse config: {e}")))?;

    let ci_config = cfg.ci.unwrap_or_default();
    let circuits: Vec<_> = cfg
        .circuits
        .into_iter()
        .map(|c| (c.name, c.path, c.params))
        .collect();

    Ok((ci_config, circuits))
}

/// Run benchmarks for CI circuits using engine workflow.
fn run_ci_benchmarks(
    circuits: &[(String, PathBuf, Option<Vec<u64>>)],
    ci_circuits: &[String],
    iterations: usize,
    warmup: usize,
    output_path: &PathBuf,
) -> BenchResult<Vec<CiCircuitResult>> {
    // Ensure output directory exists
    if let Some(parent) = output_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).ok();
        }
    }

    let mut jsonl = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(output_path)
        .map_err(|e| BenchError::Message(format!("failed to create output file: {e}")))?;

    // Create toolchain and backend using engine workflow
    let toolchain = NargoToolchain::new();
    let bb_config = BarretenbergConfig::new("bb").with_timeout(Duration::from_secs(24 * 60 * 60));
    let backend = BarretenbergBackend::new(bb_config);

    let mut results = Vec::new();
    let timestamp = now_string();

    // Filter circuits to CI subset
    let filtered: Vec<_> = circuits
        .iter()
        .filter(|(name, _, _)| ci_circuits.is_empty() || ci_circuits.contains(name))
        .collect();

    if filtered.is_empty() {
        eprintln!("Warning: No matching circuits found for CI run");
        return Ok(results);
    }

    for (name, path, params_list) in filtered {
        // Expand params or run once with no params
        let param_values: Vec<Option<u64>> = match params_list {
            Some(list) if !list.is_empty() => list.iter().map(|p| Some(*p)).collect(),
            _ => vec![None],
        };

        for params in param_values {
            eprintln!("Running CI benchmark: {} (params={:?})", name, params);

            // Find Prover.toml
            let prover_toml = find_prover_toml(path);

            // Build workflow inputs
            let mut inputs =
                ProveInputs::new(path, name).with_timeout(Duration::from_secs(24 * 60 * 60));
            if let Some(pt) = prover_toml {
                inputs = inputs.with_prover_toml(pt);
            }

            // Run full benchmark using engine workflow
            let bench_result =
                match full_benchmark(&toolchain, &backend, &inputs, warmup, iterations) {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("  Benchmark failed: {e}");
                        results.push(CiCircuitResult {
                            circuit_name: name.clone(),
                            params,
                            prove_ms: 0.0,
                            gates: None,
                            proof_size_bytes: None,
                            status: "failed".to_string(),
                        });
                        continue;
                    }
                };

            // Extract metrics from BenchRecord
            let prove_stats = bench_result.record.prove_stats.as_ref();
            let avg_prove_ms = prove_stats.map(|s| s.mean_ms).unwrap_or(0.0);
            let gates = bench_result.constraints;
            let proof_size = bench_result.record.proof_size_bytes;
            let status = if bench_result.verify_success {
                "ok"
            } else {
                "verify_failed"
            };

            // Write JSONL record (compatible with BenchRecord schema)
            let record = json!({
                "schema_version": 1,
                "record_id": format!("ci-{}-{}", name, timestamp.replace([':', '-', 'T', 'Z'], "")),
                "timestamp": timestamp,
                "circuit_name": name,
                "env": { "os": std::env::consts::OS },
                "backend": { "name": "barretenberg" },
                "config": {
                    "warmup_iterations": warmup,
                    "measured_iterations": iterations
                },
                "prove_stats": {
                    "iterations": prove_stats.map(|s| s.iterations).unwrap_or(0),
                    "mean_ms": avg_prove_ms,
                    "min_ms": prove_stats.map(|s| s.min_ms).unwrap_or(0.0),
                    "max_ms": prove_stats.map(|s| s.max_ms).unwrap_or(0.0)
                },
                "total_gates": gates,
                "acir_opcodes": bench_result.acir_opcodes,
                "proof_size_bytes": proof_size,
                "peak_rss_mb": bench_result.record.peak_rss_mb
            });
            writeln!(jsonl, "{}", serde_json::to_string(&record).unwrap())
                .map_err(|e| BenchError::Message(format!("failed to write record: {e}")))?;

            results.push(CiCircuitResult {
                circuit_name: name.clone(),
                params,
                prove_ms: avg_prove_ms,
                gates,
                proof_size_bytes: proof_size,
                status: status.to_string(),
            });

            eprintln!(
                "  {} prove_ms={:.1} gates={:?} status={}",
                name, avg_prove_ms, gates, status
            );
        }
    }

    Ok(results)
}

/// Find Prover.toml for a circuit path.
fn find_prover_toml(path: &PathBuf) -> Option<PathBuf> {
    // Try alongside artifact with .toml extension
    let mut p = path.clone();
    p.set_extension("toml");
    if p.exists() {
        return Some(p);
    }
    // Try parent of target/
    path.parent()
        .and_then(|dir| dir.parent().map(|pp| pp.join("Prover.toml")))
        .filter(|cand| cand.exists())
}

/// Format CI results as markdown
fn format_markdown(result: &CiRunResult) -> String {
    let mut out = String::new();

    out.push_str("## 🚀 noir-bench CI Report\n\n");
    out.push_str(&format!("**Timestamp:** {}\n\n", result.timestamp));

    // Benchmark results table
    out.push_str("### Benchmark Results\n\n");
    out.push_str("| Circuit | Params | Prove (ms) | Gates | Proof Size | Status |\n");
    out.push_str("|---------|--------|------------|-------|------------|--------|\n");

    for c in &result.circuits {
        let params_str = c
            .params
            .map(|p| p.to_string())
            .unwrap_or_else(|| "-".to_string());
        let gates_str = c
            .gates
            .map(|g| format!("{}", g))
            .unwrap_or_else(|| "-".to_string());
        let size_str = c
            .proof_size_bytes
            .map(|s| {
                if s >= 1024 {
                    format!("{:.1} KB", s as f64 / 1024.0)
                } else {
                    format!("{} B", s)
                }
            })
            .unwrap_or_else(|| "-".to_string());
        let status_emoji = match c.status.as_str() {
            "ok" => "✅",
            "compile_failed" | "prove_failed" | "verify_failed" | "verify_error" => "❌",
            _ => "⚠️",
        };

        out.push_str(&format!(
            "| {} | {} | {:.1} | {} | {} | {} |\n",
            c.circuit_name, params_str, c.prove_ms, gates_str, size_str, status_emoji
        ));
    }

    // Comparison results if available
    if let Some(comparison) = &result.comparison {
        out.push_str("\n### Regression Analysis\n\n");
        out.push_str(&format!(
            "**Baseline:** `{}` | **Target:** current | **Threshold:** {:.1}%\n\n",
            comparison.baseline_ref, comparison.threshold
        ));

        out.push_str("| Circuit | Metric | Baseline | Current | Δ | Status |\n");
        out.push_str("|---------|--------|----------|---------|---|--------|\n");

        for circuit in &comparison.circuits {
            for (i, m) in circuit.metrics.iter().enumerate() {
                let circuit_col = if i == 0 { &circuit.circuit_name } else { "" };
                let delta_str = if m.delta == 0.0 {
                    "0".to_string()
                } else {
                    format!("{:+.1}%", m.percent)
                };

                out.push_str(&format!(
                    "| {} | {} | {:.1} | {:.1} | {} | {} |\n",
                    circuit_col,
                    m.metric,
                    m.baseline,
                    m.target,
                    delta_str,
                    m.status.emoji()
                ));
            }
        }

        out.push_str("\n");
        if comparison.total_regressions > 0 {
            out.push_str(&format!(
                "**Result:** ❌ {} regression(s) detected\n",
                comparison.total_regressions
            ));
        } else {
            out.push_str("**Result:** ✅ No regressions detected\n");
        }

        if comparison.total_improvements > 0 {
            out.push_str(&format!(
                "🎉 {} improvement(s) found\n",
                comparison.total_improvements
            ));
        }
    } else {
        out.push_str("\n*No baseline file found for comparison*\n");
    }

    out.push_str("\n---\n");
    out.push_str("Generated by `noir-bench ci`\n");

    out
}

/// Main entry point for CI command
pub fn run(
    config: Option<PathBuf>,
    circuits: Option<Vec<String>>,
    baseline_file: Option<PathBuf>,
    threshold: Option<f64>,
    iterations: Option<usize>,
    warmup: Option<usize>,
    output: Option<PathBuf>,
    format: String,
    json_out: Option<PathBuf>,
    html_out: Option<PathBuf>,
) -> BenchResult<i32> {
    let config_path = config.unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG));

    // Load config
    let (ci_config, all_circuits) = if config_path.exists() {
        load_ci_config(&config_path)?
    } else {
        eprintln!(
            "Warning: Config file not found at {}",
            config_path.display()
        );
        (CiConfig::default(), Vec::new())
    };

    // Determine which circuits to run
    let ci_circuits: Vec<String> = circuits
        .or_else(|| {
            if ci_config.circuits.is_empty() {
                None
            } else {
                Some(ci_config.circuits.clone())
            }
        })
        .unwrap_or_default();

    // Determine baseline file
    let baseline_path = baseline_file
        .or_else(|| ci_config.baseline_file.map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from(DEFAULT_BASELINE));

    // Determine threshold
    let threshold_pct = threshold
        .or(ci_config.threshold_percent)
        .unwrap_or(DEFAULT_THRESHOLD);

    // Determine iterations
    let iter_n = iterations
        .or(ci_config.iterations)
        .unwrap_or(DEFAULT_CI_ITERATIONS);
    let warmup_n = warmup.or(ci_config.warmup).unwrap_or(DEFAULT_CI_WARMUP);

    // Output file for benchmark results
    let output_path = output.unwrap_or_else(|| {
        let tmp = std::env::temp_dir().join("noir-bench-ci-results.jsonl");
        tmp
    });

    eprintln!("noir-bench ci");
    eprintln!("  Config: {}", config_path.display());
    eprintln!(
        "  Circuits: {:?}",
        if ci_circuits.is_empty() {
            vec!["all".to_string()]
        } else {
            ci_circuits.clone()
        }
    );
    eprintln!("  Baseline: {}", baseline_path.display());
    eprintln!("  Threshold: {:.1}%", threshold_pct);
    eprintln!("  Iterations: {} (warmup: {})", iter_n, warmup_n);
    eprintln!("");

    // Run benchmarks
    let circuit_results =
        run_ci_benchmarks(&all_circuits, &ci_circuits, iter_n, warmup_n, &output_path)?;

    // Compare against baseline if it exists
    let comparison = if baseline_path.exists() {
        eprintln!("Comparing against baseline: {}", baseline_path.display());
        let compare_config = compare_cmd::CompareConfig {
            baseline_file: Some(baseline_path.clone()),
            target_file: Some(output_path.clone()),
            baseline_json: None,
            target_json: None,
            threshold: threshold_pct,
            format: "text".to_string(),
            json_out: None,
        };
        match compare_cmd::compare(&compare_config) {
            Ok(result) => Some(result),
            Err(e) => {
                eprintln!("Warning: Comparison failed: {e}");
                None
            }
        }
    } else {
        eprintln!("No baseline file found at {}", baseline_path.display());
        None
    };

    let exit_code = comparison.as_ref().map(|c| c.ci_exit_code).unwrap_or(0);

    let result = CiRunResult {
        timestamp: now_string(),
        circuits: circuit_results,
        comparison,
        exit_code,
    };

    // Collect provenance once for reuse
    let target_provenance = provenance::collect(None);

    // Write RegressionReport JSON if requested
    if let Some(ref json_path) = json_out {
        if let Some(ref comp) = result.comparison {
            let mut regression_report = to_regression_report(comp);
            regression_report.set_provenance(None, Some(target_provenance.clone()));

            let json_str = serde_json::to_string_pretty(&regression_report).map_err(|e| {
                BenchError::Message(format!("failed to serialize regression report: {e}"))
            })?;
            std::fs::write(json_path, json_str).map_err(|e| {
                BenchError::Message(format!("failed to write {}: {e}", json_path.display()))
            })?;
            eprintln!("Wrote regression report to {}", json_path.display());
        } else {
            eprintln!("Warning: No comparison data available for --json-out (no baseline)");
        }
    }

    // Write HTML report if requested
    if let Some(ref html_path) = html_out {
        if let Some(ref comp) = result.comparison {
            let mut regression_report = to_regression_report(comp);
            regression_report.set_provenance(None, Some(target_provenance.clone()));

            report_write_html(html_path, &regression_report)
                .map_err(|e| BenchError::Message(format!("failed to write HTML report: {e}")))?;
            eprintln!("Wrote HTML report to {}", html_path.display());
        } else {
            eprintln!("Warning: No comparison data available for --html-out (no baseline)");
        }
    }

    // Output results
    let output_str = match format.as_str() {
        "json" => serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".to_string()),
        "markdown" | "md" => {
            // Use the new RegressionReport markdown renderer if we have comparison data
            if let Some(ref comp) = result.comparison {
                let mut regression_report = to_regression_report(comp);
                regression_report.set_provenance(None, Some(target_provenance));
                report_render_markdown(&regression_report)
            } else {
                format_markdown(&result)
            }
        }
        _ => {
            // Text format
            let mut s = String::new();
            s.push_str(&format!("CI Run: {}\n", result.timestamp));
            for c in &result.circuits {
                s.push_str(&format!(
                    "  {}: prove_ms={:.1} gates={:?} status={}\n",
                    c.circuit_name, c.prove_ms, c.gates, c.status
                ));
            }
            if let Some(comp) = &result.comparison {
                s.push_str(&format!(
                    "\nRegressions: {} | Improvements: {}\n",
                    comp.total_regressions, comp.total_improvements
                ));
            }
            s
        }
    };

    println!("{}", output_str);

    Ok(exit_code)
}
