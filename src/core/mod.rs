//! Core types and schemas for noir-bench.
//!
//! This module contains the canonical `BenchRecord` schema (v1) used for all benchmark outputs.

pub mod env;
pub mod schema;

// Re-export key types for convenience
pub use env::EnvironmentInfo;
pub use schema::{BackendInfo, BenchRecord, RunConfig, SCHEMA_VERSION, TimingStat};
