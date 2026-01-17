//! History module for derived artifacts from canonical JSONL.
//!
//! This module provides functionality to build derived index artifacts from
//! the canonical JSONL telemetry format. The derived artifacts (index.json, index.html,
//! per-run detail pages) are for visualization and querying - the canonical source remains JSONL.

pub mod build;
pub mod html;
pub mod run_html;
pub mod schema;

pub use build::{assign_detail_slugs, build_index, write_index_json};
pub use html::{render_history_html, write_history_html};
pub use run_html::{html_escape, render_run_detail_html, write_run_detail_html};
pub use schema::{
    RUN_INDEX_SCHEMA_VERSION, RunIndexMetricsV1, RunIndexRecordV1, make_run_href, make_run_slug,
};
