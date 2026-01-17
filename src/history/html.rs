//! HTML generator for history index.
//!
//! Generates a single-file HTML that fetches index.json at runtime.
//! Uses textContent for all dynamic data insertion (XSS-safe).
//! SVG chart built via DOM APIs (createElement, setAttribute) - no innerHTML.

use std::fs;
use std::path::Path;

use crate::BenchError;

/// Render the history index HTML.
///
/// The HTML is a single file with embedded CSS and JS that:
/// - Fetches ./index.json at runtime
/// - Renders a table using textContent (not innerHTML) for safety
/// - Renders an SVG trend chart using DOM APIs (createElement, setAttribute)
/// - Is deterministic: same output every time
pub fn render_history_html() -> String {
    // Static template - no dynamic data embedded
    r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>noir-bench History</title>
<style>
* { box-sizing: border-box; margin: 0; padding: 0; }
body {
  font-family: system-ui, -apple-system, sans-serif;
  background: #1a1a2e;
  color: #e8e8e8;
  padding: 24px;
}
h1 { font-size: 1.5rem; margin-bottom: 16px; }
h2 { font-size: 1.125rem; margin: 24px 0 12px 0; color: #9a9a9a; }
#status { color: #9a9a9a; font-size: 0.875rem; margin-bottom: 16px; }
#error { color: #ff6b6b; margin-bottom: 16px; }
#limit-info { color: #9a9a9a; font-size: 0.8125rem; margin-bottom: 12px; font-style: italic; }
#controls { display: flex; gap: 16px; margin-bottom: 16px; flex-wrap: wrap; align-items: center; }
#controls label { font-size: 0.875rem; color: #9a9a9a; }
#controls select, #controls input {
  background: #16213e;
  border: 1px solid #2d3a5c;
  color: #e8e8e8;
  padding: 6px 10px;
  border-radius: 4px;
  font-size: 0.875rem;
}
#controls input[type="number"] { width: 80px; }
#controls select:focus, #controls input:focus { outline: 1px solid #4ecdc4; }
#chart-container {
  background: #16213e;
  border-radius: 8px;
  padding: 16px;
  margin-bottom: 24px;
  min-height: 200px;
}
#chart-message {
  color: #9a9a9a;
  font-size: 0.875rem;
  text-align: center;
  padding: 80px 0;
}
#chart-svg { display: block; width: 100%; height: 180px; }
table { width: 100%; border-collapse: collapse; font-size: 0.875rem; background: #16213e; }
th, td { padding: 10px 12px; text-align: left; border-bottom: 1px solid #2d3a5c; }
th { background: #1a1a2e; color: #9a9a9a; font-weight: 600; font-size: 0.75rem; text-transform: uppercase; }
tr:hover { background: #1f2b47; }
.mono { font-family: monospace; }
.num { text-align: right; }
.ok { color: #4ecdc4; }
.error { color: #ff6b6b; }
a { color: #4ecdc4; text-decoration: none; }
a:hover { text-decoration: underline; }
</style>
</head>
<body>
<h1>noir-bench History</h1>
<div id="status">Loading...</div>
<div id="error"></div>
<div id="controls" style="display:none">
<label for="metric-select">Metric:</label>
<select id="metric-select"></select>
<label for="circuit-filter">Circuit filter:</label>
<input type="text" id="circuit-filter" placeholder="substring match">
<label for="row-limit">Row limit:</label>
<input type="number" id="row-limit" min="1" max="100000" value="500">
</div>
<div id="limit-info" style="display:none"></div>
<h2 id="chart-title" style="display:none">Trend Chart</h2>
<div id="chart-container" style="display:none">
<div id="chart-message"></div>
<svg id="chart-svg" viewBox="0 0 800 180" preserveAspectRatio="xMidYMid meet" style="display:none"></svg>
</div>
<table id="table" style="display:none">
<thead>
<tr>
<th>Timestamp</th>
<th>Circuit</th>
<th>Backend</th>
<th>Status</th>
<th class="num">prove_p50_ms</th>
<th class="num">prove_p95_ms</th>
<th class="num">gates</th>
<th>Details</th>
</tr>
</thead>
<tbody id="tbody"></tbody>
</table>
<script>
var allRecords = [];
var DEFAULT_ROW_LIMIT = 500;
var METRICS = [
  {key: 'prove_ms_p50', label: 'prove_ms_p50'},
  {key: 'prove_ms_p95', label: 'prove_ms_p95'},
  {key: 'verify_ms_p50', label: 'verify_ms_p50'},
  {key: 'gates', label: 'gates'},
  {key: 'peak_rss_bytes', label: 'peak_rss_bytes'}
];

function hasMetric(records, key) {
  for (var i = 0; i < records.length; i++) {
    var m = records[i].metrics || {};
    if (m[key] != null) return true;
  }
  return false;
}

function populateMetricSelect(records) {
  var sel = document.getElementById('metric-select');
  sel.innerHTML = '';
  for (var i = 0; i < METRICS.length; i++) {
    var metric = METRICS[i];
    if (hasMetric(records, metric.key)) {
      var opt = document.createElement('option');
      opt.value = metric.key;
      opt.textContent = metric.label;
      sel.appendChild(opt);
    }
  }
}

function getFilteredRecords() {
  var filter = document.getElementById('circuit-filter').value.toLowerCase();
  if (!filter) return allRecords;
  var result = [];
  for (var i = 0; i < allRecords.length; i++) {
    var r = allRecords[i];
    if (r.circuit_name && r.circuit_name.toLowerCase().indexOf(filter) !== -1) {
      result.push(r);
    }
  }
  return result;
}

function getRowLimit() {
  var input = document.getElementById('row-limit');
  var val = parseInt(input.value, 10);
  if (isNaN(val) || val < 1) return DEFAULT_ROW_LIMIT;
  return val;
}

function getLimitedRecords(filtered) {
  var limit = getRowLimit();
  if (filtered.length <= limit) return { records: filtered, total: filtered.length, limited: false };
  return { records: filtered.slice(0, limit), total: filtered.length, limited: true };
}

function renderChart(records) {
  var svg = document.getElementById('chart-svg');
  var msg = document.getElementById('chart-message');
  var sel = document.getElementById('metric-select');
  var key = sel.value;

  // Clear SVG using DOM (safe)
  while (svg.firstChild) svg.removeChild(svg.firstChild);

  // Extract data points
  var points = [];
  for (var i = 0; i < records.length; i++) {
    var m = records[i].metrics || {};
    if (m[key] != null) {
      points.push({idx: i, val: m[key]});
    }
  }

  if (points.length < 2) {
    svg.style.display = 'none';
    msg.style.display = '';
    msg.textContent = 'Not enough data for chart (need at least 2 points)';
    return;
  }

  msg.style.display = 'none';
  svg.style.display = '';

  // Chart dimensions
  var W = 800, H = 180;
  var padL = 60, padR = 20, padT = 20, padB = 30;
  var chartW = W - padL - padR;
  var chartH = H - padT - padB;

  // Find min/max
  var minVal = points[0].val, maxVal = points[0].val;
  for (var i = 1; i < points.length; i++) {
    if (points[i].val < minVal) minVal = points[i].val;
    if (points[i].val > maxVal) maxVal = points[i].val;
  }
  // Handle flat line
  if (minVal === maxVal) {
    minVal = minVal * 0.9;
    maxVal = maxVal * 1.1;
    if (minVal === 0 && maxVal === 0) { minVal = 0; maxVal = 1; }
  }

  // Scale functions
  function scaleX(idx) {
    if (points.length === 1) return padL + chartW / 2;
    return padL + (idx / (points.length - 1)) * chartW;
  }
  function scaleY(val) {
    return padT + chartH - ((val - minVal) / (maxVal - minVal)) * chartH;
  }

  // Create SVG namespace helper
  var ns = 'http://www.w3.org/2000/svg';

  // Draw axes
  var xAxis = document.createElementNS(ns, 'line');
  xAxis.setAttribute('x1', padL);
  xAxis.setAttribute('y1', H - padB);
  xAxis.setAttribute('x2', W - padR);
  xAxis.setAttribute('y2', H - padB);
  xAxis.setAttribute('stroke', '#2d3a5c');
  xAxis.setAttribute('stroke-width', '1');
  svg.appendChild(xAxis);

  var yAxis = document.createElementNS(ns, 'line');
  yAxis.setAttribute('x1', padL);
  yAxis.setAttribute('y1', padT);
  yAxis.setAttribute('x2', padL);
  yAxis.setAttribute('y2', H - padB);
  yAxis.setAttribute('stroke', '#2d3a5c');
  yAxis.setAttribute('stroke-width', '1');
  svg.appendChild(yAxis);

  // Y-axis labels
  var yLabels = [minVal, (minVal + maxVal) / 2, maxVal];
  for (var i = 0; i < yLabels.length; i++) {
    var lbl = document.createElementNS(ns, 'text');
    lbl.setAttribute('x', padL - 8);
    lbl.setAttribute('y', scaleY(yLabels[i]) + 4);
    lbl.setAttribute('text-anchor', 'end');
    lbl.setAttribute('fill', '#9a9a9a');
    lbl.setAttribute('font-size', '10');
    lbl.setAttribute('font-family', 'monospace');
    lbl.textContent = formatNumber(yLabels[i]);
    svg.appendChild(lbl);
  }

  // X-axis label (record index range)
  var xLbl = document.createElementNS(ns, 'text');
  xLbl.setAttribute('x', W / 2);
  xLbl.setAttribute('y', H - 5);
  xLbl.setAttribute('text-anchor', 'middle');
  xLbl.setAttribute('fill', '#9a9a9a');
  xLbl.setAttribute('font-size', '10');
  xLbl.textContent = 'Record index (oldest to newest)';
  svg.appendChild(xLbl);

  // Build polyline points string
  var polyPoints = '';
  for (var i = 0; i < points.length; i++) {
    var x = scaleX(i);
    var y = scaleY(points[i].val);
    polyPoints += x + ',' + y + ' ';
  }

  // Draw polyline
  var polyline = document.createElementNS(ns, 'polyline');
  polyline.setAttribute('points', polyPoints.trim());
  polyline.setAttribute('fill', 'none');
  polyline.setAttribute('stroke', '#4ecdc4');
  polyline.setAttribute('stroke-width', '2');
  svg.appendChild(polyline);

  // Draw circles at data points
  for (var i = 0; i < points.length; i++) {
    var circle = document.createElementNS(ns, 'circle');
    circle.setAttribute('cx', scaleX(i));
    circle.setAttribute('cy', scaleY(points[i].val));
    circle.setAttribute('r', '4');
    circle.setAttribute('fill', '#4ecdc4');
    svg.appendChild(circle);
  }
}

function formatNumber(n) {
  if (n >= 1000000) return (n / 1000000).toFixed(1) + 'M';
  if (n >= 1000) return (n / 1000).toFixed(1) + 'K';
  if (n === Math.floor(n)) return n.toString();
  return n.toFixed(1);
}

function renderTable(records) {
  var tbody = document.getElementById('tbody');
  var table = document.getElementById('table');
  tbody.innerHTML = '';

  for (var i = 0; i < records.length; i++) {
    var r = records[i];
    var m = r.metrics || {};
    var tr = document.createElement('tr');

    // Timestamp
    var td0 = document.createElement('td');
    td0.className = 'mono';
    td0.textContent = r.timestamp ? r.timestamp.replace('T', ' ').replace('Z', '').slice(0, 19) : '';
    tr.appendChild(td0);

    // Circuit
    var td1 = document.createElement('td');
    td1.textContent = r.circuit_name || '';
    tr.appendChild(td1);

    // Backend
    var td2 = document.createElement('td');
    td2.textContent = r.backend || '';
    tr.appendChild(td2);

    // Status
    var td3 = document.createElement('td');
    td3.textContent = r.status || '';
    td3.className = r.status === 'ok' ? 'ok' : 'error';
    tr.appendChild(td3);

    // prove_p50_ms
    var td4 = document.createElement('td');
    td4.className = 'mono num';
    td4.textContent = m.prove_ms_p50 != null ? m.prove_ms_p50.toFixed(1) : '';
    tr.appendChild(td4);

    // prove_p95_ms
    var td5 = document.createElement('td');
    td5.className = 'mono num';
    td5.textContent = m.prove_ms_p95 != null ? m.prove_ms_p95.toFixed(1) : '';
    tr.appendChild(td5);

    // gates
    var td6 = document.createElement('td');
    td6.className = 'mono num';
    td6.textContent = m.gates != null ? m.gates : '';
    tr.appendChild(td6);

    // Details link
    var td7 = document.createElement('td');
    if (r.detail_href) {
      var link = document.createElement('a');
      link.href = r.detail_href;
      link.textContent = 'View';
      td7.appendChild(link);
    }
    tr.appendChild(td7);

    tbody.appendChild(tr);
  }

  table.style.display = '';
}

function update() {
  var filtered = getFilteredRecords();
  var result = getLimitedRecords(filtered);
  var limitInfo = document.getElementById('limit-info');

  if (result.limited) {
    limitInfo.style.display = '';
    limitInfo.textContent = 'Showing first ' + result.records.length + ' of ' + result.total + ' rows (increase limit to see more)';
  } else {
    limitInfo.style.display = 'none';
    limitInfo.textContent = '';
  }

  renderChart(result.records);
  renderTable(result.records);
}

document.getElementById('metric-select').addEventListener('change', update);
document.getElementById('circuit-filter').addEventListener('input', update);
document.getElementById('row-limit').addEventListener('input', update);

fetch('./index.json')
  .then(function(r) { return r.json(); })
  .then(function(data) {
    allRecords = data;
    document.getElementById('status').textContent = 'Loaded ' + data.length + ' record(s)';
    populateMetricSelect(data);
    document.getElementById('controls').style.display = '';
    document.getElementById('chart-title').style.display = '';
    document.getElementById('chart-container').style.display = '';
    update();
  })
  .catch(function(e) {
    document.getElementById('status').textContent = 'Error';
    document.getElementById('error').textContent = e.message;
  });
</script>
</body>
</html>"##.to_string()
}

/// Write the history HTML to a file.
pub fn write_history_html(output_path: &Path) -> Result<(), BenchError> {
    if let Some(parent) = output_path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)
                .map_err(|e| BenchError::Message(format!("failed to create directory: {e}")))?;
        }
    }

    let html = render_history_html();
    fs::write(output_path, html)
        .map_err(|e| BenchError::Message(format!("failed to write index.html: {e}")))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_html_is_deterministic() {
        let html1 = render_history_html();
        let html2 = render_history_html();
        assert_eq!(html1, html2);
    }

    #[test]
    fn test_html_deterministic_bytes() {
        // More stringent check: byte-for-byte identical
        let html1 = render_history_html();
        let html2 = render_history_html();
        assert_eq!(
            html1.as_bytes(),
            html2.as_bytes(),
            "HTML output should be byte-for-byte identical"
        );
    }

    #[test]
    fn test_html_structure() {
        let html = render_history_html();
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("fetch('./index.json')"));
        assert!(html.contains("<table"));
        assert!(html.contains("</table>"));
    }

    #[test]
    fn test_html_uses_textcontent_for_safety() {
        let html = render_history_html();

        // Must use textContent for all dynamic data (XSS-safe)
        // Count occurrences of textContent assignment for data fields
        let textcontent_count = html.matches(".textContent =").count();
        assert!(
            textcontent_count >= 8,
            "Should use textContent for all 8 data columns (including link text) plus status/error"
        );

        // The render function should NOT use innerHTML for data cells
        // (We do use innerHTML = '' to clear tbody/select, which is safe since it's empty string)
        // Verify we don't interpolate user data via innerHTML
        assert!(
            !html.contains("innerHTML = r."),
            "Should not use innerHTML with record data"
        );
        assert!(
            !html.contains("innerHTML = m."),
            "Should not use innerHTML with metrics data"
        );
    }

    /// Focused safety test: verify no XSS patterns in the template.
    ///
    /// The HTML template should:
    /// 1. Not contain any pattern that would allow data to create <img> or </script> tags
    /// 2. Use textContent (not innerHTML) for inserting user data
    /// 3. Use setAttribute (not attribute interpolation) for SVG elements
    #[test]
    fn test_html_xss_safe_patterns() {
        let html = render_history_html();

        // The static template should not contain patterns that could be exploited
        // if data were somehow injected (defense in depth)

        // Verify all dynamic data insertion uses textContent
        // Each table cell (7 columns) should use textContent
        assert!(
            html.contains("td0.textContent"),
            "timestamp must use textContent"
        );
        assert!(
            html.contains("td1.textContent"),
            "circuit must use textContent"
        );
        assert!(
            html.contains("td2.textContent"),
            "backend must use textContent"
        );
        assert!(
            html.contains("td3.textContent"),
            "status must use textContent"
        );
        assert!(
            html.contains("td4.textContent"),
            "prove_p50 must use textContent"
        );
        assert!(
            html.contains("td5.textContent"),
            "prove_p95 must use textContent"
        );
        assert!(
            html.contains("td6.textContent"),
            "gates must use textContent"
        );

        // Verify no dangerous innerHTML patterns with user data
        // innerHTML = '' is safe (clearing), but innerHTML = someVariable is not
        let lines: Vec<&str> = html.lines().collect();
        for line in lines {
            if line.contains("innerHTML") && !line.contains("innerHTML = ''") {
                panic!("Found potentially unsafe innerHTML usage: {}", line);
            }
        }

        // Verify the template itself doesn't contain XSS test strings
        // (this confirms we're not accidentally embedding test data)
        assert!(
            !html.contains("<img"),
            "Template should not contain <img tags"
        );
        assert!(
            !html.contains("onerror"),
            "Template should not contain onerror handlers"
        );

        // Count the legitimate </script> tags - should be exactly 1 (closing the JS block)
        let script_close_count = html.matches("</script>").count();
        assert_eq!(script_close_count, 1, "Should have exactly 1 </script> tag");
    }

    #[test]
    fn test_html_no_external_assets() {
        let html = render_history_html();

        // No external CSS/JS
        assert!(!html.contains("href=\"http"));
        assert!(!html.contains("src=\"http"));
        assert!(!html.contains("<link"));

        // CSS is inline
        assert!(html.contains("<style>"));
        assert!(html.contains("</style>"));
    }

    #[test]
    fn test_html_has_required_columns() {
        let html = render_history_html();

        // Header columns
        assert!(html.contains(">Timestamp<"));
        assert!(html.contains(">Circuit<"));
        assert!(html.contains(">Backend<"));
        assert!(html.contains(">Status<"));
        assert!(html.contains(">prove_p50_ms<"));
        assert!(html.contains(">prove_p95_ms<"));
        assert!(html.contains(">gates<"));
        assert!(html.contains(">Details<"));
    }

    #[test]
    fn test_html_has_detail_link_support() {
        let html = render_history_html();

        // Should check for detail_href and create link
        assert!(
            html.contains("r.detail_href"),
            "Should check for detail_href"
        );
        assert!(
            html.contains("link.href = r.detail_href"),
            "Should set link href from detail_href"
        );
        assert!(
            html.contains("link.textContent = 'View'"),
            "Should use textContent for link text (XSS-safe)"
        );
    }

    #[test]
    fn test_html_has_chart_controls() {
        let html = render_history_html();

        // Metric dropdown
        assert!(
            html.contains("id=\"metric-select\""),
            "Should have metric dropdown with id metric-select"
        );
        assert!(
            html.contains("<select id=\"metric-select\""),
            "metric-select should be a select element"
        );

        // Circuit filter
        assert!(
            html.contains("id=\"circuit-filter\""),
            "Should have circuit filter input with id circuit-filter"
        );
        assert!(
            html.contains("<input type=\"text\" id=\"circuit-filter\""),
            "circuit-filter should be a text input"
        );

        // Row limit control
        assert!(
            html.contains("id=\"row-limit\""),
            "Should have row limit input with id row-limit"
        );
        assert!(
            html.contains("<input type=\"number\" id=\"row-limit\""),
            "row-limit should be a number input"
        );

        // Chart container
        assert!(
            html.contains("id=\"chart-container\""),
            "Should have chart container"
        );
        assert!(
            html.contains("id=\"chart-svg\""),
            "Should have SVG element for chart"
        );
        assert!(
            html.contains("id=\"chart-message\""),
            "Should have chart message area"
        );

        // Controls container
        assert!(
            html.contains("id=\"controls\""),
            "Should have controls container"
        );
    }

    #[test]
    fn test_html_chart_metrics_defined() {
        let html = render_history_html();

        // All metrics should be defined in the METRICS array
        assert!(
            html.contains("'prove_ms_p50'"),
            "Should define prove_ms_p50 metric"
        );
        assert!(
            html.contains("'prove_ms_p95'"),
            "Should define prove_ms_p95 metric"
        );
        assert!(
            html.contains("'verify_ms_p50'"),
            "Should define verify_ms_p50 metric"
        );
        assert!(html.contains("'gates'"), "Should define gates metric");
        assert!(
            html.contains("'peak_rss_bytes'"),
            "Should define peak_rss_bytes metric"
        );
    }

    #[test]
    fn test_html_svg_uses_safe_dom_apis() {
        let html = render_history_html();

        // SVG should be built using safe DOM APIs
        assert!(
            html.contains("createElementNS"),
            "Should use createElementNS for SVG elements"
        );
        assert!(
            html.contains("setAttribute"),
            "Should use setAttribute for SVG attributes"
        );
        assert!(
            html.contains("http://www.w3.org/2000/svg"),
            "Should use SVG namespace"
        );

        // SVG elements should be created via DOM, not innerHTML
        assert!(
            html.contains("createElementNS(ns, 'line')"),
            "Should create line elements via DOM"
        );
        assert!(
            html.contains("createElementNS(ns, 'polyline')"),
            "Should create polyline elements via DOM"
        );
        assert!(
            html.contains("createElementNS(ns, 'circle')"),
            "Should create circle elements via DOM"
        );
        assert!(
            html.contains("createElementNS(ns, 'text')"),
            "Should create text elements via DOM"
        );

        // Clearing SVG should use safe DOM method
        assert!(
            html.contains("svg.removeChild(svg.firstChild)"),
            "Should clear SVG using removeChild, not innerHTML"
        );
    }

    #[test]
    fn test_html_chart_shows_not_enough_data_message() {
        let html = render_history_html();

        // Should show message when not enough data
        assert!(
            html.contains("Not enough data"),
            "Should have 'not enough data' message"
        );
        assert!(
            html.contains("need at least 2 points"),
            "Should clarify 2 points needed"
        );
    }

    #[test]
    fn test_html_no_random_or_dynamic_ids() {
        let html = render_history_html();

        // All IDs should be constant strings
        // Count ID attributes - they should all be hardcoded
        let id_count = html.matches("id=\"").count();
        assert!(id_count >= 10, "Should have multiple ID attributes");

        // Verify specific IDs are present (ensures they're constant)
        let expected_ids = [
            "status",
            "error",
            "controls",
            "metric-select",
            "circuit-filter",
            "row-limit",
            "limit-info",
            "chart-title",
            "chart-container",
            "chart-message",
            "chart-svg",
            "table",
            "tbody",
        ];
        for id in expected_ids {
            assert!(
                html.contains(&format!("id=\"{}\"", id)),
                "Should have constant id: {}",
                id
            );
        }
    }

    #[test]
    fn test_html_has_row_limit_control() {
        let html = render_history_html();

        // Should have DEFAULT_ROW_LIMIT constant
        assert!(
            html.contains("DEFAULT_ROW_LIMIT = 500"),
            "Should define DEFAULT_ROW_LIMIT constant"
        );

        // Should have row-limit input with correct attributes
        assert!(
            html.contains("id=\"row-limit\""),
            "Should have row-limit input"
        );
        assert!(
            html.contains("<input type=\"number\" id=\"row-limit\""),
            "row-limit should be a number input"
        );
        assert!(
            html.contains("value=\"500\""),
            "row-limit should default to 500"
        );

        // Should have limit-info div for the message
        assert!(
            html.contains("id=\"limit-info\""),
            "Should have limit-info div"
        );

        // Should have getRowLimit and getLimitedRecords functions
        assert!(
            html.contains("function getRowLimit()"),
            "Should have getRowLimit function"
        );
        assert!(
            html.contains("function getLimitedRecords("),
            "Should have getLimitedRecords function"
        );

        // Should show message via textContent (XSS-safe)
        assert!(
            html.contains("limitInfo.textContent ="),
            "limit-info message should use textContent"
        );

        // Should have event listener for row-limit
        assert!(
            html.contains("getElementById('row-limit').addEventListener"),
            "Should have event listener for row-limit"
        );
    }

    #[test]
    fn test_html_row_limit_uses_slice_not_mutation() {
        let html = render_history_html();

        // getLimitedRecords should use slice to limit, not mutate original
        assert!(
            html.contains("filtered.slice(0, limit)"),
            "Should use slice to limit records"
        );

        // Should return object with records, total, and limited flag
        assert!(
            html.contains("records: filtered"),
            "Should return records in result object"
        );
        assert!(
            html.contains("total: filtered.length"),
            "Should include total count"
        );
        assert!(
            html.contains("limited: false") && html.contains("limited: true"),
            "Should include limited flag"
        );
    }

    #[test]
    fn test_html_limit_message_format() {
        let html = render_history_html();

        // The limit message should follow the specified format
        assert!(
            html.contains("Showing first"),
            "Message should start with 'Showing first'"
        );
        assert!(
            html.contains("increase limit to see more"),
            "Message should mention increasing limit"
        );
    }
}
