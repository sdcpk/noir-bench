//! Engine module: orchestrates toolchains and backends for benchmark workflows.
//!
//! # Architecture
//!
//! This module introduces a clean separation between:
//!
//! - **Toolchain**: Responsible for Noir-specific operations (compile, witness generation).
//!   Examples: `NargoToolchain` (shells out to `nargo`).
//!
//! - **Backend**: Responsible for proving system operations (prove, verify, gate analysis).
//!   Defined in `crate::backend` - examples: `BarretenbergBackend`, `MockBackend`.
//!
//! The `workflow` submodule composes these to execute complete benchmark workflows
//! (e.g., compile -> witness -> prove) while collecting timing statistics.
//!
//! # Boundaries
//!
//! - `Toolchain` does NOT know about proving/verification - that's the Backend's job.
//! - `Backend` does NOT know about Noir source compilation - that's the Toolchain's job.
//! - Workflow functions orchestrate both to produce `BenchRecord` outputs.

pub mod provenance;
pub mod toolchain;
pub mod workflow;

// Re-export key types for convenience
pub use toolchain::{CompileArtifacts, MockToolchain, NargoToolchain, Toolchain, WitnessArtifact};
pub use workflow::{
    FullBenchmarkResult, ProveInputs, full_benchmark, prove_only, prove_with_iterations,
};
