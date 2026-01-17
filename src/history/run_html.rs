//! Per-run detail page generator.
//!
//! Generates static HTML pages for individual benchmark runs.
//! NO JavaScript required - uses <details> for collapsible sections.
//! All user-controlled strings are HTML-escaped for XSS safety.

use std::fs;
use std::path::Path;

use crate::BenchError;
use crate::core::schema::BenchRecord;

/// HTML-escape a string for safe insertion into HTML content.
///
/// Escapes: & < > " '
/// This prevents XSS when inserting user-controlled strings into HTML.
pub fn html_escape(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => result.push_str("&amp;"),
            '<' => result.push_str("&lt;"),
            '>' => result.push_str("&gt;"),
            '"' => result.push_str("&quot;"),
            '\'' => result.push_str("&#x27;"),
            _ => result.push(c),
        }
    }
    result
}

/// Format an optional f64 for display.
fn fmt_opt_f64(v: Option<f64>, suffix: &str) -> String {
    match v {
        Some(val) => format!("{:.3}{}", val, suffix),
        None => "—".to_string(),
    }
}

/// Format an optional u64 for display.
fn fmt_opt_u64(v: Option<u64>, suffix: &str) -> String {
    match v {
        Some(val) => format!("{}{}", val, suffix),
        None => "—".to_string(),
    }
}

/// Render a timing stat section as HTML.
fn render_timing_section(name: &str, stat: Option<&crate::core::schema::TimingStat>) -> String {
    match stat {
        Some(s) => {
            format!(
                r#"<details>
<summary>{}</summary>
<table class="stat-table">
<tr><td>Iterations</td><td class="num">{}</td></tr>
<tr><td>Mean</td><td class="num">{:.3} ms</td></tr>
<tr><td>Median</td><td class="num">{}</td></tr>
<tr><td>Std Dev</td><td class="num">{}</td></tr>
<tr><td>Min</td><td class="num">{:.3} ms</td></tr>
<tr><td>Max</td><td class="num">{:.3} ms</td></tr>
<tr><td>P95</td><td class="num">{}</td></tr>
</table>
</details>"#,
                html_escape(name),
                s.iterations,
                s.mean_ms,
                fmt_opt_f64(s.median_ms, " ms"),
                fmt_opt_f64(s.stddev_ms, " ms"),
                s.min_ms,
                s.max_ms,
                fmt_opt_f64(s.p95_ms, " ms"),
            )
        }
        None => String::new(),
    }
}

/// Render a per-run detail page as static HTML.
///
/// The output is a complete HTML document with:
/// - Header with circuit name and back link
/// - Summary metrics table
/// - Environment/toolchain info
/// - Phase timing details (collapsible)
/// - Raw JSON record (collapsible)
///
/// All user-controlled strings are HTML-escaped.
/// NO JavaScript - uses <details> for interactivity.
pub fn render_run_detail_html(record: &BenchRecord, slug: &str) -> String {
    // Escape all user-controlled strings
    let circuit_name = html_escape(&record.circuit_name);
    let record_id = html_escape(&record.record_id);
    let timestamp = html_escape(&record.timestamp);
    let backend_name = html_escape(&record.backend.name);
    let backend_version = record
        .backend
        .version
        .as_ref()
        .map(|v| html_escape(v))
        .unwrap_or_else(|| "—".to_string());
    let backend_variant = record
        .backend
        .variant
        .as_ref()
        .map(|v| html_escape(v))
        .unwrap_or_else(|| "—".to_string());

    // Environment info
    let os = html_escape(&record.env.os);
    let cpu = record
        .env
        .cpu_model
        .as_ref()
        .map(|v| html_escape(v))
        .unwrap_or_else(|| "—".to_string());
    let cores = record
        .env
        .cpu_cores
        .map(|v| v.to_string())
        .unwrap_or_else(|| "—".to_string());
    let ram_gb = record
        .env
        .total_ram_bytes
        .map(|v| format!("{:.1} GB", v as f64 / 1_000_000_000.0))
        .unwrap_or_else(|| "—".to_string());
    let nargo_version = record
        .env
        .nargo_version
        .as_ref()
        .map(|v| html_escape(v))
        .unwrap_or_else(|| "—".to_string());
    let hostname = record
        .env
        .hostname
        .as_ref()
        .map(|v| html_escape(v))
        .unwrap_or_else(|| "—".to_string());

    // Metrics
    let gates = fmt_opt_u64(record.total_gates, "");
    let acir_opcodes = fmt_opt_u64(record.acir_opcodes, "");
    let subgroup_size = fmt_opt_u64(record.subgroup_size, "");
    let proof_size = fmt_opt_u64(record.proof_size_bytes, " bytes");
    let pk_size = fmt_opt_u64(record.proving_key_size_bytes, " bytes");
    let vk_size = fmt_opt_u64(record.verification_key_size_bytes, " bytes");
    let peak_rss = record
        .peak_rss_mb
        .map(|v| format!("{:.1} MB", v))
        .unwrap_or_else(|| "—".to_string());

    // Timing sections
    let compile_section = render_timing_section("Compile/Load", record.compile_stats.as_ref());
    let witness_section =
        render_timing_section("Witness Generation", record.witness_stats.as_ref());
    let prove_section = render_timing_section("Proving", record.prove_stats.as_ref());
    let verify_section = render_timing_section("Verification", record.verify_stats.as_ref());

    // Raw JSON (escaped for HTML)
    let raw_json = serde_json::to_string_pretty(record).unwrap_or_else(|_| "{}".to_string());
    let raw_json_escaped = html_escape(&raw_json);

    // CLI args
    let cli_args = if record.cli_args.is_empty() {
        "—".to_string()
    } else {
        record
            .cli_args
            .iter()
            .map(|a| html_escape(a))
            .collect::<Vec<_>>()
            .join(" ")
    };

    format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>{circuit_name} - {slug}</title>
<style>
* {{ box-sizing: border-box; margin: 0; padding: 0; }}
body {{
  font-family: system-ui, -apple-system, sans-serif;
  background: #1a1a2e;
  color: #e8e8e8;
  padding: 24px;
  max-width: 900px;
  margin: 0 auto;
}}
a {{ color: #4ecdc4; text-decoration: none; }}
a:hover {{ text-decoration: underline; }}
.back {{ margin-bottom: 16px; font-size: 0.875rem; }}
h1 {{ font-size: 1.5rem; margin-bottom: 8px; }}
.meta {{ color: #9a9a9a; font-size: 0.8125rem; margin-bottom: 24px; }}
.meta code {{ background: #16213e; padding: 2px 6px; border-radius: 3px; font-family: monospace; }}
h2 {{ font-size: 1.125rem; margin: 24px 0 12px 0; color: #9a9a9a; }}
table {{ width: 100%; border-collapse: collapse; font-size: 0.875rem; background: #16213e; margin-bottom: 16px; }}
th, td {{ padding: 8px 12px; text-align: left; border-bottom: 1px solid #2d3a5c; }}
th {{ background: #1a1a2e; color: #9a9a9a; font-weight: 600; font-size: 0.75rem; text-transform: uppercase; }}
.num {{ text-align: right; font-family: monospace; }}
.stat-table {{ margin: 8px 0 8px 16px; width: auto; }}
.stat-table td {{ padding: 4px 12px; }}
details {{ margin: 8px 0; }}
summary {{ cursor: pointer; padding: 8px; background: #16213e; border-radius: 4px; }}
summary:hover {{ background: #1f2b47; }}
pre {{ background: #16213e; padding: 16px; border-radius: 4px; overflow-x: auto; font-size: 0.75rem; line-height: 1.4; white-space: pre-wrap; word-break: break-all; }}
.ok {{ color: #4ecdc4; }}
.error {{ color: #ff6b6b; }}
</style>
</head>
<body>
<div class="back"><a href="../index.html">&larr; Back to History</a></div>
<h1>{circuit_name}</h1>
<div class="meta">
  <code>{record_id}</code> &middot; {timestamp}
</div>

<h2>Summary</h2>
<table>
<tr><th>Metric</th><th class="num">Value</th></tr>
<tr><td>Total Gates</td><td class="num">{gates}</td></tr>
<tr><td>ACIR Opcodes</td><td class="num">{acir_opcodes}</td></tr>
<tr><td>Subgroup Size</td><td class="num">{subgroup_size}</td></tr>
<tr><td>Proof Size</td><td class="num">{proof_size}</td></tr>
<tr><td>Proving Key Size</td><td class="num">{pk_size}</td></tr>
<tr><td>Verification Key Size</td><td class="num">{vk_size}</td></tr>
<tr><td>Peak RSS</td><td class="num">{peak_rss}</td></tr>
</table>

<h2>Environment</h2>
<table>
<tr><th>Property</th><th>Value</th></tr>
<tr><td>OS</td><td>{os}</td></tr>
<tr><td>Hostname</td><td>{hostname}</td></tr>
<tr><td>CPU</td><td>{cpu}</td></tr>
<tr><td>Cores</td><td>{cores}</td></tr>
<tr><td>RAM</td><td>{ram_gb}</td></tr>
<tr><td>Nargo Version</td><td>{nargo_version}</td></tr>
</table>

<h2>Backend</h2>
<table>
<tr><th>Property</th><th>Value</th></tr>
<tr><td>Name</td><td>{backend_name}</td></tr>
<tr><td>Version</td><td>{backend_version}</td></tr>
<tr><td>Variant</td><td>{backend_variant}</td></tr>
</table>

<h2>Run Config</h2>
<table>
<tr><th>Property</th><th class="num">Value</th></tr>
<tr><td>Warmup Iterations</td><td class="num">{warmup}</td></tr>
<tr><td>Measured Iterations</td><td class="num">{measured}</td></tr>
<tr><td>Timeout</td><td class="num">{timeout}</td></tr>
</table>

<h2>Phases</h2>
{compile_section}
{witness_section}
{prove_section}
{verify_section}

<details>
<summary>CLI Arguments</summary>
<pre>{cli_args}</pre>
</details>

<details>
<summary>Raw JSON Record</summary>
<pre>{raw_json_escaped}</pre>
</details>

</body>
</html>"##,
        circuit_name = circuit_name,
        slug = html_escape(slug),
        record_id = record_id,
        timestamp = timestamp,
        gates = gates,
        acir_opcodes = acir_opcodes,
        subgroup_size = subgroup_size,
        proof_size = proof_size,
        pk_size = pk_size,
        vk_size = vk_size,
        peak_rss = peak_rss,
        os = os,
        hostname = hostname,
        cpu = cpu,
        cores = cores,
        ram_gb = ram_gb,
        nargo_version = nargo_version,
        backend_name = backend_name,
        backend_version = backend_version,
        backend_variant = backend_variant,
        warmup = record.config.warmup_iterations,
        measured = record.config.measured_iterations,
        timeout = record
            .config
            .timeout_secs
            .map(|t| format!("{} s", t))
            .unwrap_or_else(|| "—".to_string()),
        compile_section = compile_section,
        witness_section = witness_section,
        prove_section = prove_section,
        verify_section = verify_section,
        cli_args = cli_args,
        raw_json_escaped = raw_json_escaped,
    )
}

/// Write a per-run detail page to a file.
pub fn write_run_detail_html(
    record: &BenchRecord,
    slug: &str,
    output_path: &Path,
) -> Result<(), BenchError> {
    if let Some(parent) = output_path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)
                .map_err(|e| BenchError::Message(format!("failed to create directory: {e}")))?;
        }
    }

    let html = render_run_detail_html(record, slug);
    fs::write(output_path, html).map_err(|e| {
        BenchError::Message(format!("failed to write {}: {e}", output_path.display()))
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::env::EnvironmentInfo;
    use crate::core::schema::{BackendInfo, RunConfig, TimingStat};

    #[test]
    fn test_html_escape_basic() {
        assert_eq!(html_escape("hello"), "hello");
        assert_eq!(html_escape("<script>"), "&lt;script&gt;");
        assert_eq!(html_escape("a & b"), "a &amp; b");
        assert_eq!(html_escape("\"quoted\""), "&quot;quoted&quot;");
        assert_eq!(html_escape("it's"), "it&#x27;s");
    }

    #[test]
    fn test_html_escape_xss_vectors() {
        // Common XSS attack vectors
        assert_eq!(
            html_escape("<script>alert('xss')</script>"),
            "&lt;script&gt;alert(&#x27;xss&#x27;)&lt;/script&gt;"
        );
        assert_eq!(
            html_escape("<img src=x onerror=alert(1)>"),
            "&lt;img src=x onerror=alert(1)&gt;"
        );
        assert_eq!(
            html_escape("</script><script>alert(1)</script>"),
            "&lt;/script&gt;&lt;script&gt;alert(1)&lt;/script&gt;"
        );
        assert_eq!(
            html_escape("javascript:alert(1)"),
            "javascript:alert(1)" // No angle brackets, safe in text content
        );
    }

    #[test]
    fn test_html_escape_unicode() {
        // Unicode should pass through unchanged
        assert_eq!(html_escape("日本語"), "日本語");
        assert_eq!(html_escape("émoji 🎉"), "émoji 🎉");
    }

    fn make_test_record() -> BenchRecord {
        let mut record = BenchRecord::new(
            "test_circuit".to_string(),
            EnvironmentInfo::default(),
            BackendInfo {
                name: "bb".to_string(),
                version: Some("0.62.0".to_string()),
                variant: None,
            },
            RunConfig::default(),
        );
        record.timestamp = "2024-01-15T12:00:00Z".to_string();
        record.record_id = "test-record-id".to_string();
        record.prove_stats = Some(TimingStat::from_samples(&[100.0, 110.0, 120.0]));
        record.total_gates = Some(50000);
        record
    }

    #[test]
    fn test_render_run_detail_html_structure() {
        let record = make_test_record();
        let html = render_run_detail_html(&record, "run_000001");

        // Basic structure
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("<html lang=\"en\">"));
        assert!(html.contains("</html>"));

        // No script tags (static HTML)
        assert!(
            !html.contains("<script"),
            "Detail page should have no JavaScript"
        );

        // Contains expected sections
        assert!(html.contains("<title>test_circuit - run_000001</title>"));
        assert!(html.contains("Back to History"));
        assert!(html.contains("Summary"));
        assert!(html.contains("Environment"));
        assert!(html.contains("Backend"));
        assert!(html.contains("Phases"));
        assert!(html.contains("Raw JSON Record"));

        // Contains collapsible details
        assert!(html.contains("<details>"));
        assert!(html.contains("<summary>"));
    }

    #[test]
    fn test_render_run_detail_html_deterministic() {
        let record = make_test_record();
        let html1 = render_run_detail_html(&record, "run_000001");
        let html2 = render_run_detail_html(&record, "run_000001");
        assert_eq!(html1, html2, "Detail page rendering must be deterministic");
    }

    #[test]
    fn test_render_run_detail_html_escapes_xss() {
        let mut record = make_test_record();
        record.circuit_name = "<script>alert('xss')</script>".to_string();
        record.record_id = "<img onerror=alert(1)>".to_string();

        let html = render_run_detail_html(&record, "run_000001");

        // Dangerous strings should be escaped
        assert!(!html.contains("<script>alert"));
        assert!(!html.contains("<img onerror"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("&lt;img onerror"));
    }

    #[test]
    fn test_render_run_detail_html_back_link() {
        let record = make_test_record();
        let html = render_run_detail_html(&record, "run_000001");

        // Back link should point to parent index
        assert!(html.contains("href=\"../index.html\""));
    }

    #[test]
    fn test_render_run_detail_html_raw_json_escaped() {
        let mut record = make_test_record();
        record.circuit_name = "<dangerous>".to_string();

        let html = render_run_detail_html(&record, "run_000001");

        // Raw JSON should be escaped for HTML
        assert!(html.contains("&lt;dangerous&gt;"));
        // Should not contain raw < or > from the circuit name in the JSON section
        // (The JSON will have the literal string, but it should be HTML-escaped)
    }
}
