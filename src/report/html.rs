//! Single-file HTML report generator for regression reports.
//!
//! Produces a standalone HTML file with embedded CSS and JS that renders:
//! - Summary cards (regressions, improvements, ok, missing)
//! - Per-circuit table with status and deltas vs threshold
//! - Expandable per-circuit details
//! - Provenance section with baseline vs target comparison
//! - Version mismatch warnings
//! - Simple filters: text search, status toggles, threshold breaches only

use std::path::Path;

use crate::report::RegressionReport;

/// Escape JSON for safe embedding inside an HTML `<script type="application/json">` tag.
///
/// This function takes already-serialized JSON and escapes characters that could
/// terminate or alter HTML parsing:
/// - `<` is replaced with `\u003c` to prevent `</script>` from breaking out
///
/// The output remains valid JSON that can be parsed by `JSON.parse()`.
fn escape_json_for_html_script(json: &str) -> String {
    // Replace '<' with '\u003c' - this is valid in JSON strings and prevents
    // any HTML-significant sequences like </script> or <!-- from being interpreted.
    // We do a byte-level replacement which is safe because '<' is a single ASCII byte
    // and '\u003c' is pure ASCII.
    json.replace('<', "\\u003c")
}

/// Render a RegressionReport as a standalone HTML string.
///
/// The HTML includes embedded CSS and JS, with the report JSON embedded as a
/// JavaScript constant. Circuits and warnings are sorted deterministically.
pub fn render_html(report: &RegressionReport) -> String {
    // Clone and sort for deterministic output
    let mut sorted_report = report.clone();
    sorted_report.circuits.sort_by(|a, b| {
        a.circuit_name
            .cmp(&b.circuit_name)
            .then_with(|| a.params.cmp(&b.params))
    });
    sorted_report
        .version_mismatches
        .sort_by(|a, b| a.tool.cmp(&b.tool));

    // Serialize report to JSON with stable formatting
    let report_json =
        serde_json::to_string_pretty(&sorted_report).unwrap_or_else(|_| "{}".to_string());
    // Escape for safe embedding in HTML <script type="application/json"> tag
    let escaped_json = escape_json_for_html_script(&report_json);

    // Build HTML
    let mut html = String::with_capacity(32 * 1024);

    html.push_str(r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>noir-bench Regression Report</title>
<style>
:root {
  --bg: #1a1a2e;
  --surface: #16213e;
  --surface-hover: #1f2b47;
  --text: #e8e8e8;
  --text-muted: #9a9a9a;
  --accent: #4f8cff;
  --red: #ff6b6b;
  --green: #4ecdc4;
  --yellow: #ffd93d;
  --border: #2d3a5c;
}
* { box-sizing: border-box; margin: 0; padding: 0; }
body {
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, sans-serif;
  background: var(--bg);
  color: var(--text);
  line-height: 1.5;
  padding: 24px;
  min-height: 100vh;
}
.container { max-width: 1400px; margin: 0 auto; }
h1 { font-size: 1.75rem; margin-bottom: 8px; }
h2 { font-size: 1.25rem; margin: 24px 0 12px; color: var(--text-muted); }
h3 { font-size: 1rem; margin: 16px 0 8px; }

/* Header */
.header { margin-bottom: 24px; }
.header-status { display: flex; align-items: center; gap: 12px; margin-bottom: 16px; }
.status-badge {
  padding: 4px 12px;
  border-radius: 4px;
  font-weight: 600;
  font-size: 0.875rem;
}
.status-badge.fail { background: var(--red); color: #fff; }
.status-badge.pass { background: var(--green); color: #1a1a2e; }
.meta-table { display: grid; grid-template-columns: auto 1fr; gap: 4px 16px; font-size: 0.875rem; }
.meta-label { color: var(--text-muted); }
.meta-value { font-family: monospace; }

/* Summary Cards */
.summary-cards {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
  gap: 12px;
  margin-bottom: 24px;
}
.card {
  background: var(--surface);
  border-radius: 8px;
  padding: 16px;
  text-align: center;
  border: 1px solid var(--border);
}
.card-value { font-size: 2rem; font-weight: 700; }
.card-label { font-size: 0.75rem; color: var(--text-muted); text-transform: uppercase; }
.card.regressions .card-value { color: var(--red); }
.card.improvements .card-value { color: var(--green); }
.card.warnings .card-value { color: var(--yellow); }

/* Filters */
.filters {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
  align-items: center;
  margin-bottom: 16px;
  padding: 12px;
  background: var(--surface);
  border-radius: 8px;
  border: 1px solid var(--border);
}
.search-input {
  flex: 1;
  min-width: 200px;
  padding: 8px 12px;
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: 4px;
  color: var(--text);
  font-size: 0.875rem;
}
.search-input:focus { outline: none; border-color: var(--accent); }
.filter-group { display: flex; gap: 8px; flex-wrap: wrap; }
.filter-btn {
  padding: 6px 12px;
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: 4px;
  color: var(--text-muted);
  font-size: 0.75rem;
  cursor: pointer;
  transition: all 0.15s;
}
.filter-btn:hover { border-color: var(--accent); }
.filter-btn.active { background: var(--accent); color: #fff; border-color: var(--accent); }

/* Warnings */
.warnings-section {
  background: rgba(255, 217, 61, 0.1);
  border: 1px solid var(--yellow);
  border-radius: 8px;
  padding: 16px;
  margin-bottom: 24px;
}
.warnings-section h3 { color: var(--yellow); margin-top: 0; }
.warning-item { font-size: 0.875rem; margin: 8px 0; font-family: monospace; }

/* Table */
.table-container {
  background: var(--surface);
  border-radius: 8px;
  border: 1px solid var(--border);
  overflow: hidden;
}
table { width: 100%; border-collapse: collapse; font-size: 0.875rem; }
th, td { padding: 12px 16px; text-align: left; border-bottom: 1px solid var(--border); }
th { background: var(--bg); color: var(--text-muted); font-weight: 600; text-transform: uppercase; font-size: 0.75rem; }
tr:hover { background: var(--surface-hover); }
tr:last-child td { border-bottom: none; }
.status-cell { font-weight: 600; }
.status-cell.exceeded { color: var(--red); }
.status-cell.improved { color: var(--green); }
.status-cell.ok { color: var(--text-muted); }
.status-cell.missing { color: var(--yellow); }
.delta-positive { color: var(--red); }
.delta-negative { color: var(--green); }
.mono { font-family: monospace; }

/* Expandable rows */
.expand-btn {
  background: none;
  border: none;
  color: var(--accent);
  cursor: pointer;
  font-size: 0.875rem;
  padding: 0;
}
.expand-btn:hover { text-decoration: underline; }
.details-row { display: none; }
.details-row.visible { display: table-row; }
.details-cell { background: var(--bg); padding: 16px 24px; }
.details-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 16px; }
.detail-item { font-size: 0.813rem; }
.detail-label { color: var(--text-muted); display: block; }
.detail-value { font-family: monospace; }

/* Provenance */
.provenance-section {
  background: var(--surface);
  border-radius: 8px;
  border: 1px solid var(--border);
  padding: 16px;
  margin-top: 24px;
}
.provenance-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 24px; }
.provenance-col h4 { color: var(--text-muted); margin-bottom: 12px; font-size: 0.875rem; }
.prov-item { margin: 8px 0; font-size: 0.813rem; }
.prov-label { color: var(--text-muted); }
.prov-value { font-family: monospace; }

/* Footer */
.footer {
  margin-top: 32px;
  padding-top: 16px;
  border-top: 1px solid var(--border);
  font-size: 0.75rem;
  color: var(--text-muted);
  text-align: center;
}

/* Responsive */
@media (max-width: 768px) {
  body { padding: 12px; }
  .provenance-grid { grid-template-columns: 1fr; }
  th, td { padding: 8px 12px; }
}
</style>
</head>
<body>
<div class="container" id="app"></div>
<script type="application/json" id="report-data">"#);

    html.push_str(&escaped_json);

    html.push_str(r#"</script>
<script>
// Parse report data from non-executing JSON container
const REPORT = JSON.parse(document.getElementById('report-data').textContent);

// Format numeric value based on metric type
function formatValue(value, metric) {
  if (metric.includes('size') || metric.includes('mem') || metric.includes('rss')) {
    if (metric.includes('rss_mb')) return value.toFixed(1) + ' MB';
    if (value >= 1e9) return (value / 1e9).toFixed(1) + ' GB';
    if (value >= 1e6) return (value / 1e6).toFixed(1) + ' MB';
    if (value >= 1e3) return (value / 1e3).toFixed(1) + ' KB';
    return Math.round(value) + ' B';
  }
  if (metric.includes('ms')) {
    if (value >= 1000) return (value / 1000).toFixed(2) + 's';
    return Math.round(value) + 'ms';
  }
  if (metric.includes('gates')) {
    if (value >= 1e6) return (value / 1e6).toFixed(2) + 'M';
    if (value >= 1e3) return (value / 1e3).toFixed(1) + 'K';
    return Math.round(value).toString();
  }
  return value.toFixed(2);
}

// Status to CSS class
function statusClass(status) {
  const map = {
    'exceeded_threshold': 'exceeded',
    'improved': 'improved',
    'ok': 'ok',
    'missing_baseline': 'missing',
    'error': 'exceeded',
    'skipped': 'ok'
  };
  return map[status] || 'ok';
}

// Status to display text
function statusText(status) {
  const map = {
    'exceeded_threshold': 'REGRESS',
    'improved': 'IMPROVED',
    'ok': 'OK',
    'missing_baseline': 'NO_BASE',
    'error': 'ERROR',
    'skipped': 'SKIP'
  };
  return map[status] || status;
}

// Escape HTML (including single quotes for attribute contexts)
function esc(s) {
  if (typeof s !== 'string') return s;
  return s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;').replace(/'/g,'&#39;');
}

// App state
let state = {
  search: '',
  showRegress: true,
  showImproved: true,
  showOk: true,
  showMissing: true,
  onlyThreshold: false,
  expanded: {}
};

function render() {
  const r = REPORT;
  const s = r.summary;
  const hasFail = s.regressions > 0 || s.errors > 0;

  // Filter circuits
  let circuits = r.circuits.filter(c => {
    const name = c.circuit_name.toLowerCase();
    const searchMatch = !state.search || name.includes(state.search.toLowerCase());
    if (!searchMatch) return false;

    // Status filter
    const hasRegress = c.metrics.some(m => m.status === 'exceeded_threshold');
    const hasImproved = c.metrics.some(m => m.status === 'improved');
    const hasMissing = c.metrics.some(m => m.status === 'missing_baseline');
    const allOk = !hasRegress && !hasImproved && !hasMissing;

    if (state.onlyThreshold) return hasRegress;
    if (hasRegress && state.showRegress) return true;
    if (hasImproved && state.showImproved) return true;
    if (hasMissing && state.showMissing) return true;
    if (allOk && state.showOk) return true;
    return false;
  });

  let html = `
    <div class="header">
      <div class="header-status">
        <h1>noir-bench Regression Report</h1>
        <span class="status-badge ${hasFail ? 'fail' : 'pass'}">${hasFail ? 'REGRESSIONS' : 'PASS'}</span>
      </div>
      <div class="meta-table">
        <span class="meta-label">Baseline</span><span class="meta-value">${esc(r.metadata.baseline_id)}</span>
        <span class="meta-label">Target</span><span class="meta-value">${esc(r.metadata.target_id)}</span>
        <span class="meta-label">Threshold</span><span class="meta-value">${r.metadata.threshold_percent.toFixed(1)}%</span>
        <span class="meta-label">Generated</span><span class="meta-value">${esc(r.metadata.generated_at.slice(0,19).replace('T',' '))}</span>
      </div>
    </div>

    <div class="summary-cards">
      <div class="card regressions"><div class="card-value">${s.regressions}</div><div class="card-label">Regressions</div></div>
      <div class="card improvements"><div class="card-value">${s.improvements}</div><div class="card-label">Improvements</div></div>
      <div class="card"><div class="card-value">${s.unchanged}</div><div class="card-label">Unchanged</div></div>
      <div class="card"><div class="card-value">${s.missing_baselines}</div><div class="card-label">Missing</div></div>
      <div class="card warnings"><div class="card-value">${r.version_mismatches.length}</div><div class="card-label">Warnings</div></div>
    </div>`;

  // Version mismatch warnings
  if (r.version_mismatches && r.version_mismatches.length > 0) {
    html += `<div class="warnings-section"><h3>Tool Version Mismatches</h3>`;
    for (const m of r.version_mismatches) {
      html += `<div class="warning-item">${esc(m.tool)}: ${esc(m.baseline_version || '-')} → ${esc(m.target_version || '-')}</div>`;
    }
    html += `</div>`;
  }

  // Filters
  html += `
    <div class="filters">
      <input type="text" class="search-input" placeholder="Search circuits..." value="${esc(state.search)}" oninput="updateSearch(this.value)">
      <div class="filter-group">
        <button class="filter-btn ${state.showRegress ? 'active' : ''}" onclick="toggle('showRegress')">Regressions</button>
        <button class="filter-btn ${state.showImproved ? 'active' : ''}" onclick="toggle('showImproved')">Improvements</button>
        <button class="filter-btn ${state.showOk ? 'active' : ''}" onclick="toggle('showOk')">OK</button>
        <button class="filter-btn ${state.showMissing ? 'active' : ''}" onclick="toggle('showMissing')">Missing</button>
      </div>
      <button class="filter-btn ${state.onlyThreshold ? 'active' : ''}" onclick="toggle('onlyThreshold')">Only Threshold Breaches</button>
    </div>`;

  // Circuit table
  html += `<div class="table-container"><table>
    <thead><tr><th>Circuit</th><th>Metric</th><th>Baseline</th><th>Target</th><th>Delta</th><th>Status</th><th></th></tr></thead>
    <tbody>`;

  for (const c of circuits) {
    const cid = c.circuit_name + (c.params || '');
    const isExp = state.expanded[cid];
    for (let i = 0; i < c.metrics.length; i++) {
      const m = c.metrics[i];
      const deltaClass = m.delta_pct > 0 ? 'delta-positive' : m.delta_pct < 0 ? 'delta-negative' : '';
      const deltaStr = m.delta_abs === 0 ? '0' : (m.delta_pct > 0 ? '+' : '') + m.delta_pct.toFixed(1) + '%';

      html += `<tr>
        <td>${i === 0 ? esc(c.circuit_name) + (c.params ? ' [' + esc(String(c.params)) + ']' : '') : ''}</td>
        <td class="mono">${esc(m.metric)}</td>
        <td class="mono">${formatValue(m.baseline, m.metric)}</td>
        <td class="mono">${formatValue(m.target, m.metric)}</td>
        <td class="mono ${deltaClass}">${deltaStr}</td>
        <td class="status-cell ${statusClass(m.status)}">${statusText(m.status)}</td>
        <td>${i === 0 ? '<button class="expand-btn" data-cid="' + esc(cid) + '" onclick="toggleExpand(this.dataset.cid)">' + (isExp ? 'Hide' : 'Details') + '</button>' : ''}</td>
      </tr>`;
    }

    // Details row - use data-cid attribute instead of id with user content
    html += `<tr class="details-row ${isExp ? 'visible' : ''}" data-details-cid="${esc(cid)}">
      <td colspan="7" class="details-cell">
        <div class="details-grid">
          <div class="detail-item"><span class="detail-label">Circuit</span><span class="detail-value">${esc(c.circuit_name)}</span></div>
          ${c.params ? '<div class="detail-item"><span class="detail-label">Params</span><span class="detail-value">' + esc(String(c.params)) + '</span></div>' : ''}
          <div class="detail-item"><span class="detail-label">Status</span><span class="detail-value">${statusText(c.status)}</span></div>
          <div class="detail-item"><span class="detail-label">Threshold</span><span class="detail-value">${r.metadata.threshold_percent.toFixed(1)}%</span></div>
        </div>
        <h4 style="margin-top:16px;color:var(--text-muted);">All Metrics</h4>
        <div class="details-grid">`;
    for (const m of c.metrics) {
      html += `<div class="detail-item">
        <span class="detail-label">${esc(m.metric)}</span>
        <span class="detail-value">${formatValue(m.baseline, m.metric)} → ${formatValue(m.target, m.metric)}</span>
      </div>`;
    }
    html += `</div></td></tr>`;
  }

  html += `</tbody></table></div>`;

  // Provenance section
  const bp = r.metadata.baseline_provenance;
  const tp = r.metadata.target_provenance;
  if (bp || tp) {
    html += `<div class="provenance-section"><h3>Provenance</h3><div class="provenance-grid">`;

    if (bp) {
      html += `<div class="provenance-col"><h4>Baseline</h4>`;
      if (bp.tool_info) {
        html += `<div class="prov-item"><span class="prov-label">noir-bench: </span><span class="prov-value">${esc(bp.tool_info.noir_bench_version || '-')}</span></div>`;
        html += `<div class="prov-item"><span class="prov-label">nargo: </span><span class="prov-value">${esc(bp.tool_info.nargo_version || '-')}</span></div>`;
        html += `<div class="prov-item"><span class="prov-label">bb: </span><span class="prov-value">${esc(bp.tool_info.bb_version || '-')}</span></div>`;
      }
      if (bp.system_info) {
        html += `<div class="prov-item"><span class="prov-label">OS: </span><span class="prov-value">${esc(bp.system_info.os || '-')} ${esc(bp.system_info.arch || '')}</span></div>`;
        html += `<div class="prov-item"><span class="prov-label">CPU: </span><span class="prov-value">${esc(bp.system_info.cpu_model || '-')}</span></div>`;
      }
      html += `</div>`;
    }

    if (tp) {
      html += `<div class="provenance-col"><h4>Target</h4>`;
      if (tp.tool_info) {
        html += `<div class="prov-item"><span class="prov-label">noir-bench: </span><span class="prov-value">${esc(tp.tool_info.noir_bench_version || '-')}</span></div>`;
        html += `<div class="prov-item"><span class="prov-label">nargo: </span><span class="prov-value">${esc(tp.tool_info.nargo_version || '-')}</span></div>`;
        html += `<div class="prov-item"><span class="prov-label">bb: </span><span class="prov-value">${esc(tp.tool_info.bb_version || '-')}</span></div>`;
      }
      if (tp.system_info) {
        html += `<div class="prov-item"><span class="prov-label">OS: </span><span class="prov-value">${esc(tp.system_info.os || '-')} ${esc(tp.system_info.arch || '')}</span></div>`;
        html += `<div class="prov-item"><span class="prov-label">CPU: </span><span class="prov-value">${esc(tp.system_info.cpu_model || '-')}</span></div>`;
      }
      html += `</div>`;
    }

    html += `</div></div>`;
  }

  // Footer
  html += `<div class="footer">Generated by noir-bench v${esc(REPORT.version ? REPORT.version.toString() : '1')} | Report schema v${REPORT.version || 1}</div>`;

  document.getElementById('app').innerHTML = html;
}

function updateSearch(v) { state.search = v; render(); }
function toggle(key) { state[key] = !state[key]; render(); }
function toggleExpand(cid) { state.expanded[cid] = !state.expanded[cid]; render(); }

// Initial render
render();
</script>
</body>
</html>"#);

    html
}

/// Write a RegressionReport as a standalone HTML file.
pub fn write_html(path: &Path, report: &RegressionReport) -> anyhow::Result<()> {
    let html = render_html(report);
    std::fs::write(path, html)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{CircuitRegression, MetricDelta, RegressionReport, RegressionStatus};

    fn create_test_report() -> RegressionReport {
        let mut report = RegressionReport::new("baseline.jsonl", "target.jsonl", 10.0);

        report.add_circuit(CircuitRegression {
            circuit_name: "test-circuit".to_string(),
            params: None,
            metrics: vec![
                MetricDelta {
                    metric: "prove_ms".to_string(),
                    baseline: 100.0,
                    target: 120.0,
                    delta_abs: 20.0,
                    delta_pct: 20.0,
                    threshold: 10.0,
                    status: RegressionStatus::ExceededThreshold,
                },
                MetricDelta {
                    metric: "gates".to_string(),
                    baseline: 1000.0,
                    target: 1000.0,
                    delta_abs: 0.0,
                    delta_pct: 0.0,
                    threshold: 10.0,
                    status: RegressionStatus::Ok,
                },
            ],
            status: RegressionStatus::ExceededThreshold,
        });

        report.add_circuit(CircuitRegression {
            circuit_name: "fast-circuit".to_string(),
            params: Some(42),
            metrics: vec![MetricDelta {
                metric: "prove_ms".to_string(),
                baseline: 200.0,
                target: 150.0,
                delta_abs: -50.0,
                delta_pct: -25.0,
                threshold: 10.0,
                status: RegressionStatus::Improved,
            }],
            status: RegressionStatus::Improved,
        });

        report.finalize();
        report
    }

    #[test]
    fn test_escape_json_for_html_script() {
        // Should escape < to prevent </script> breakout
        assert_eq!(escape_json_for_html_script("</script>"), "\\u003c/script>");
        // Should preserve valid JSON structure
        assert_eq!(
            escape_json_for_html_script(r#"{"key": "<value>"}"#),
            r#"{"key": "\u003cvalue>"}"#
        );
        // Should not modify strings without <
        assert_eq!(
            escape_json_for_html_script(r#"{"key": "value"}"#),
            r#"{"key": "value"}"#
        );
        // Verify the output is valid JSON when parsed (< is safe in JSON strings)
        let json = r#"{"name": "</script><img onerror=alert(1)>"}"#;
        let escaped = escape_json_for_html_script(json);
        assert!(
            !escaped.contains("</script>"),
            "Should not contain literal </script>"
        );
        // The escaped JSON should still be parseable
        let _: serde_json::Value =
            serde_json::from_str(&escaped).expect("escaped JSON should be valid");
    }

    #[test]
    fn test_render_html_contains_structure() {
        let report = create_test_report();
        let html = render_html(&report);

        // Check basic structure
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("<html"));
        assert!(html.contains("</html>"));
        assert!(html.contains("noir-bench Regression Report"));

        // Check report data is embedded in application/json container
        assert!(html.contains(r#"<script type="application/json" id="report-data">"#));
        assert!(html.contains("JSON.parse(document.getElementById('report-data').textContent)"));
        assert!(html.contains("test-circuit"));
        assert!(html.contains("fast-circuit"));

        // Check CSS and JS are inline
        assert!(html.contains("<style>"));
        assert!(html.contains("</style>"));
        assert!(html.contains("<script>"));
        assert!(html.contains("</script>"));
    }

    #[test]
    fn test_render_html_deterministic() {
        let report = create_test_report();

        // Render twice and compare
        let html1 = render_html(&report);
        let html2 = render_html(&report);

        assert_eq!(html1, html2, "HTML output should be deterministic");
    }

    #[test]
    fn test_render_html_sorted_circuits() {
        let mut report = RegressionReport::new("base", "target", 10.0);

        // Add circuits in non-alphabetical order
        report.add_circuit(CircuitRegression {
            circuit_name: "zebra".to_string(),
            params: None,
            metrics: vec![],
            status: RegressionStatus::Ok,
        });
        report.add_circuit(CircuitRegression {
            circuit_name: "alpha".to_string(),
            params: None,
            metrics: vec![],
            status: RegressionStatus::Ok,
        });
        report.finalize();

        let html = render_html(&report);

        // Alpha should appear before zebra in sorted output
        let alpha_pos = html.find("alpha").unwrap();
        let zebra_pos = html.find("zebra").unwrap();
        assert!(
            alpha_pos < zebra_pos,
            "Circuits should be sorted alphabetically"
        );
    }

    #[test]
    fn test_render_html_escapes_user_content() {
        let mut report = RegressionReport::new("<script>alert(1)</script>", "target", 10.0);
        report.add_circuit(CircuitRegression {
            circuit_name: "<img onerror=alert(1)>".to_string(),
            params: None,
            metrics: vec![],
            status: RegressionStatus::Ok,
        });
        report.finalize();

        let html = render_html(&report);

        // Should not contain unescaped HTML tags from user content
        assert!(!html.contains("<script>alert"));
        assert!(!html.contains("<img onerror"));
    }

    #[test]
    fn test_write_html() {
        let report = create_test_report();
        let temp_dir = std::env::temp_dir();
        let path = temp_dir.join("test-report.html");

        let result = write_html(&path, &report);
        assert!(result.is_ok());

        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains("<!DOCTYPE html>"));

        // Cleanup
        let _ = std::fs::remove_file(&path);
    }

    // =======================================================================
    // XSS and escaping regression tests
    // =======================================================================

    #[test]
    fn test_xss_single_quote_in_circuit_name() {
        // Test: single quote in user content should be safe
        // Since HTML is rendered by JS at runtime, we verify:
        // 1. The JSON blob contains the data correctly
        // 2. No literal </script> can break out of the JSON container
        // 3. The JS esc() function handles single quotes (verified by code inspection)
        let mut report = RegressionReport::new("baseline", "target", 10.0);
        report.add_circuit(CircuitRegression {
            circuit_name: "x' y".to_string(),
            params: None,
            metrics: vec![],
            status: RegressionStatus::Ok,
        });
        report.finalize();

        let html = render_html(&report);

        // Verify the JSON contains the circuit name (single quote is valid in JSON)
        assert!(
            html.contains(r#""circuit_name": "x' y""#),
            "JSON should contain the raw name with single quote"
        );

        // Verify no script breakout is possible
        assert!(
            !html.contains("</script><"),
            "Should not be able to break out of script tag"
        );

        // Verify the JS esc() function escapes single quotes
        assert!(
            html.contains(".replace(/'/g,'&#39;')"),
            "JS esc() should escape single quotes"
        );

        // Verify we use data attributes instead of inline JS interpolation
        assert!(
            html.contains("data-cid="),
            "Should use data-cid attribute for expand buttons"
        );
        assert!(
            html.contains("this.dataset.cid"),
            "Should read cid from dataset, not inline string"
        );

        // Output should be deterministic
        let html2 = render_html(&report);
        assert_eq!(html, html2);
    }

    #[test]
    fn test_xss_script_injection_in_circuit_name() {
        // Test: </script><img src=x onerror=alert(1)> should not execute
        // The key protection is that < is escaped to \u003c in the JSON blob
        let malicious = "</script><img src=x onerror=alert(1)>";
        let mut report = RegressionReport::new("baseline", "target", 10.0);
        report.add_circuit(CircuitRegression {
            circuit_name: malicious.to_string(),
            params: None,
            metrics: vec![],
            status: RegressionStatus::Ok,
        });
        report.finalize();

        let html = render_html(&report);

        // CRITICAL: The literal </script> should NOT appear in the JSON blob
        // Count occurrences of </script> - should only be the legitimate closing tags
        let script_close_count = html.matches("</script>").count();
        // We expect exactly 2: one closing the application/json tag, one closing the JS
        assert_eq!(
            script_close_count, 2,
            "Should only have 2 </script> tags (json + js), not user content"
        );

        // The JSON should have < escaped as \u003c
        assert!(
            html.contains(r#"\u003c/script>\u003cimg"#),
            "JSON should have < escaped as \\u003c"
        );

        // Verify no raw <img tag from user content
        assert!(
            !html.contains("<img src=x"),
            "Should not contain unescaped img tag from user content"
        );

        // Output should be deterministic
        let html2 = render_html(&report);
        assert_eq!(html, html2);
    }

    #[test]
    fn test_xss_ampersand_and_less_than() {
        // Test: < and & should be properly escaped in the JSON blob
        let mut report = RegressionReport::new("base & <target>", "target", 10.0);
        report.add_circuit(CircuitRegression {
            circuit_name: "a < b & c".to_string(),
            params: None,
            metrics: vec![],
            status: RegressionStatus::Ok,
        });
        report.finalize();

        let html = render_html(&report);

        // In JSON context, < must be escaped (& is valid in JSON strings)
        assert!(
            html.contains(r#""baseline_id": "base & \u003ctarget>""#),
            "JSON should escape < but preserve &"
        );

        // Circuit name should have < escaped in JSON
        assert!(
            html.contains(r#""circuit_name": "a \u003c b & c""#),
            "Circuit name should have < escaped in JSON"
        );

        // No literal < from user content should appear outside the escaped JSON
        // (The JS template uses the esc() function for all user content in HTML)

        // Output should be deterministic
        let html2 = render_html(&report);
        assert_eq!(html, html2);
    }

    #[test]
    fn test_json_embedding_is_valid_json() {
        // Ensure the JSON blob embedded in HTML is actually parseable
        let mut report = RegressionReport::new("baseline", "target", 10.0);
        report.add_circuit(CircuitRegression {
            circuit_name: "test</script>\"'&<>".to_string(),
            params: Some(42),
            metrics: vec![MetricDelta {
                metric: "gates".to_string(),
                baseline: 1000.0,
                target: 1100.0,
                delta_abs: 100.0,
                delta_pct: 10.0,
                threshold: 5.0,
                status: RegressionStatus::ExceededThreshold,
            }],
            status: RegressionStatus::ExceededThreshold,
        });
        report.finalize();

        let html = render_html(&report);

        // Extract the JSON blob from between the script tags
        let start_marker = r#"<script type="application/json" id="report-data">"#;
        let end_marker = "</script>";
        let start = html.find(start_marker).expect("should find JSON start") + start_marker.len();
        let remaining = &html[start..];
        let end = remaining.find(end_marker).expect("should find JSON end");
        let json_blob = &remaining[..end];

        // The JSON blob should be valid JSON (the browser would parse this)
        // Note: We need to unescape \u003c back to < for JSON parsing in Rust,
        // but actually \u003c IS valid JSON unicode escape, so it should parse directly
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(json_blob);
        assert!(
            parsed.is_ok(),
            "Embedded JSON should be valid: {:?}",
            parsed.err()
        );

        // Verify the circuit name is preserved correctly
        let value = parsed.unwrap();
        let circuits = value["circuits"].as_array().expect("should have circuits");
        assert_eq!(circuits.len(), 1);
        assert_eq!(
            circuits[0]["circuit_name"].as_str().unwrap(),
            "test</script>\"'&<>"
        );
    }

    #[test]
    fn test_deterministic_output_with_special_chars() {
        // Comprehensive test that output is deterministic even with special characters
        let mut report = RegressionReport::new("base</script>", "target<img onerror=x>", 10.0);
        report.add_circuit(CircuitRegression {
            circuit_name: "circuit'with\"quotes&amps".to_string(),
            params: Some(123),
            metrics: vec![],
            status: RegressionStatus::Ok,
        });
        report.add_circuit(CircuitRegression {
            circuit_name: "another<circuit>".to_string(),
            params: None,
            metrics: vec![],
            status: RegressionStatus::Ok,
        });
        report.finalize();

        // Render multiple times
        let html1 = render_html(&report);
        let html2 = render_html(&report);
        let html3 = render_html(&report);

        assert_eq!(html1, html2, "First two renders should match");
        assert_eq!(html2, html3, "Second and third renders should match");
    }

    /// Focused regression test for JSON embedding safety and XSS hardening.
    ///
    /// Tests three dangerous strings:
    /// 1. "O'Reilly" - single quote (JS string breakout)
    /// 2. "</script><img src=x onerror=alert(1)>" - script injection
    /// 3. "<tag>&stuff" - HTML special characters
    #[test]
    fn test_xss_hardening_comprehensive() {
        // Dangerous test strings
        const SINGLE_QUOTE: &str = "O'Reilly";
        const SCRIPT_INJECTION: &str = "</script><img src=x onerror=alert(1)>";
        const HTML_SPECIAL: &str = "<tag>&stuff";

        // Build a minimal report with all dangerous strings in various fields
        let mut report = RegressionReport::new(
            SCRIPT_INJECTION, // baseline_id contains script injection
            HTML_SPECIAL,     // target_id contains HTML special chars
            10.0,
        );

        // Circuit with single quote in name
        report.add_circuit(CircuitRegression {
            circuit_name: SINGLE_QUOTE.to_string(),
            params: None,
            metrics: vec![MetricDelta {
                metric: "prove_ms".to_string(),
                baseline: 100.0,
                target: 110.0,
                delta_abs: 10.0,
                delta_pct: 10.0,
                threshold: 5.0,
                status: RegressionStatus::ExceededThreshold,
            }],
            status: RegressionStatus::ExceededThreshold,
        });

        // Circuit with script injection in name
        report.add_circuit(CircuitRegression {
            circuit_name: SCRIPT_INJECTION.to_string(),
            params: Some(42),
            metrics: vec![],
            status: RegressionStatus::Ok,
        });

        // Circuit with HTML special chars in name
        report.add_circuit(CircuitRegression {
            circuit_name: HTML_SPECIAL.to_string(),
            params: None,
            metrics: vec![],
            status: RegressionStatus::Ok,
        });

        report.finalize();

        let html = render_html(&report);

        // ===================================================================
        // ASSERTION 1: No raw "</script>" from user data
        // ===================================================================
        // Count </script> occurrences - should only be the 2 legitimate closing tags
        // (one for application/json, one for the JS block)
        let script_close_count = html.matches("</script>").count();
        assert_eq!(
            script_close_count, 2,
            "Should have exactly 2 </script> tags (json container + js block), \
             found {}. User data must not inject raw </script>",
            script_close_count
        );

        // ===================================================================
        // ASSERTION 2: No unescaped single quotes in JS string literals
        // ===================================================================
        // The old vulnerable pattern was: onclick="toggleExpand('...')"
        // With user data containing ', this would break out.
        // We now use data attributes, so there should be no JS string literals
        // containing user data. Verify the safe pattern is used.
        assert!(
            html.contains("this.dataset.cid"),
            "Should use dataset API instead of inline JS string interpolation"
        );
        // Verify data-cid attributes exist (user data goes into HTML attributes, not JS)
        assert!(
            html.contains("data-cid="),
            "Should use data-cid attributes for circuit identifiers"
        );
        // The JS esc() function should escape single quotes for HTML attribute safety
        assert!(
            html.contains(".replace(/'/g,'&#39;')"),
            "JS esc() function must escape single quotes"
        );

        // ===================================================================
        // ASSERTION 3: JSON blob contains properly escaped data
        // ===================================================================
        // In JSON context, < must be escaped as \u003c to prevent </script> breakout
        assert!(
            html.contains(r#"\u003c/script>\u003cimg"#),
            "Script injection string should have < escaped as \\u003c in JSON"
        );
        assert!(
            html.contains(r#"\u003ctag>"#),
            "HTML special chars should have < escaped as \\u003c in JSON"
        );
        // Single quotes and & are valid in JSON strings, no escaping needed there
        assert!(
            html.contains(r#""circuit_name": "O'Reilly""#),
            "Single quote should appear as-is in JSON (valid JSON)"
        );
        assert!(
            html.contains("&stuff"),
            "Ampersand should appear as-is in JSON (valid JSON)"
        );

        // ===================================================================
        // ASSERTION 4: JSON blob is valid and parseable
        // ===================================================================
        let start_marker = r#"<script type="application/json" id="report-data">"#;
        let end_marker = "</script>";
        let start_idx = html
            .find(start_marker)
            .expect("HTML should contain application/json script tag")
            + start_marker.len();
        let remaining = &html[start_idx..];
        let end_idx = remaining
            .find(end_marker)
            .expect("JSON script tag should have closing tag");
        let json_blob = &remaining[..end_idx];

        // Parse the JSON
        let parsed: serde_json::Value =
            serde_json::from_str(json_blob).expect("Embedded JSON must be valid and parseable");

        // Verify the data survived the round-trip correctly
        let circuits = parsed["circuits"]
            .as_array()
            .expect("Parsed JSON should have circuits array");
        assert_eq!(circuits.len(), 3, "Should have 3 circuits");

        // Check each circuit name was preserved correctly after JSON parsing
        let circuit_names: Vec<&str> = circuits
            .iter()
            .filter_map(|c| c["circuit_name"].as_str())
            .collect();
        assert!(
            circuit_names.contains(&SINGLE_QUOTE),
            "O'Reilly should be preserved in parsed JSON"
        );
        assert!(
            circuit_names.contains(&SCRIPT_INJECTION),
            "</script>... should be preserved in parsed JSON"
        );
        assert!(
            circuit_names.contains(&HTML_SPECIAL),
            "<tag>&stuff should be preserved in parsed JSON"
        );

        // Verify metadata also survived
        assert_eq!(
            parsed["metadata"]["baseline_id"].as_str().unwrap(),
            SCRIPT_INJECTION,
            "baseline_id should preserve script injection string after JSON parse"
        );
        assert_eq!(
            parsed["metadata"]["target_id"].as_str().unwrap(),
            HTML_SPECIAL,
            "target_id should preserve HTML special chars after JSON parse"
        );

        // ===================================================================
        // ASSERTION 5: Output is deterministic
        // ===================================================================
        let html2 = render_html(&report);
        assert_eq!(html, html2, "Output must be deterministic across renders");
    }
}
