//! Unified backend abstraction for proving systems.
//!
//! This module provides a consolidated `Backend` trait that combines
//! proving, verification, and gate analysis capabilities.

pub mod barretenberg;
pub mod mock;
pub mod traits;

// Re-export key types
pub use barretenberg::{BarretenbergBackend, BarretenbergConfig};
pub use mock::{MockBackend, MockConfig};
pub use traits::{Backend, Capabilities, GateInfo, ProveOutput, VerifyOutput};
