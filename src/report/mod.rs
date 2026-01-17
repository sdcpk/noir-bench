//! Reporting module for benchmark results and regression analysis.
//!
//! This module provides:
//! - `RegressionReport`: Stable machine-readable regression report structure
//! - Markdown rendering for PR comments
//! - HTML rendering for standalone reports
//! - JSON output for CI pipelines

pub mod html;
pub mod regression;

// Re-export key types
pub use html::{render_html, write_html};
pub use regression::{
    CircuitRegression, MetricDelta, RegressionReport, RegressionStatus, ReportMetadata,
    ReportSummary, compute_delta_status, format_value, render_markdown,
};
