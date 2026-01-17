//! Backend trait and output types for the unified backend abstraction.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::BenchResult;

/// Capabilities that a backend may support.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Capabilities {
    /// Can generate proofs
    pub can_prove: bool,
    /// Can verify proofs
    pub can_verify: bool,
    /// Can compile/load circuits
    pub can_compile: bool,
    /// Reports total gate count
    pub has_gate_count: bool,
    /// Reports per-opcode gate breakdown
    pub has_per_opcode_breakdown: bool,
    /// Reports PK/VK sizes
    pub has_pk_vk_sizes: bool,
}

impl Capabilities {
    /// Capabilities for the Barretenberg backend (all features supported).
    pub fn barretenberg() -> Self {
        Capabilities {
            can_prove: true,
            can_verify: true,
            can_compile: true,
            has_gate_count: true,
            has_per_opcode_breakdown: true,
            has_pk_vk_sizes: true,
        }
    }

    /// Minimal capabilities (only gate count).
    pub fn gates_only() -> Self {
        Capabilities {
            can_prove: false,
            can_verify: false,
            can_compile: false,
            has_gate_count: true,
            has_per_opcode_breakdown: false,
            has_pk_vk_sizes: false,
        }
    }
}

/// Output from a prove operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProveOutput {
    /// Total proving time in milliseconds (witness gen + backend prove)
    pub prove_time_ms: u128,
    /// Time spent generating witness (if measurable separately)
    pub witness_gen_time_ms: Option<u128>,
    /// Time spent in backend proving (if measurable separately)
    pub backend_prove_time_ms: Option<u128>,
    /// Peak memory usage in bytes
    pub peak_memory_bytes: Option<u64>,
    /// Size of the generated proof in bytes
    pub proof_size_bytes: Option<u64>,
    /// Size of the proving key in bytes
    pub proving_key_size_bytes: Option<u64>,
    /// Size of the verification key in bytes
    pub verification_key_size_bytes: Option<u64>,
    /// Path to the generated proof file
    pub proof_path: Option<PathBuf>,
    /// Path to the verification key file
    pub vk_path: Option<PathBuf>,
}

impl Default for ProveOutput {
    fn default() -> Self {
        ProveOutput {
            prove_time_ms: 0,
            witness_gen_time_ms: None,
            backend_prove_time_ms: None,
            peak_memory_bytes: None,
            proof_size_bytes: None,
            proving_key_size_bytes: None,
            verification_key_size_bytes: None,
            proof_path: None,
            vk_path: None,
        }
    }
}

/// Output from a verify operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyOutput {
    /// Verification time in milliseconds
    pub verify_time_ms: u128,
    /// Whether verification succeeded
    pub success: bool,
}

/// Gate information from circuit analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateInfo {
    /// Total backend gates (circuit size)
    pub backend_gates: u64,
    /// Subgroup size (next power of 2 >= backend_gates)
    pub subgroup_size: Option<u64>,
    /// Number of ACIR opcodes
    pub acir_opcodes: Option<u64>,
    /// Per-opcode gate breakdown (opcode name -> gate count)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub per_opcode: Option<HashMap<String, u64>>,
}

impl GateInfo {
    /// Create GateInfo with just the total gate count.
    pub fn from_gates(gates: u64) -> Self {
        let subgroup_size = if gates > 0 {
            Some(gates.next_power_of_two())
        } else {
            None
        };
        GateInfo {
            backend_gates: gates,
            subgroup_size,
            acir_opcodes: None,
            per_opcode: None,
        }
    }
}

/// Unified backend trait for proving systems.
///
/// This trait consolidates the functionality from ProverProvider and GatesProvider
/// into a single abstraction that can be implemented by different backends.
pub trait Backend: Send + Sync {
    /// Returns the backend name (e.g., "barretenberg", "mock").
    fn name(&self) -> &str;

    /// Returns the backend version, if available.
    fn version(&self) -> Option<String>;

    /// Returns the capabilities supported by this backend.
    fn capabilities(&self) -> Capabilities;

    /// Generate a proof for the given artifact.
    ///
    /// # Arguments
    /// * `artifact` - Path to the compiled circuit artifact
    /// * `witness` - Optional path to a pre-generated witness file
    /// * `timeout` - Maximum time to wait for proving
    ///
    /// # Returns
    /// ProveOutput containing timing and size information
    fn prove(
        &self,
        artifact: &Path,
        witness: Option<&Path>,
        timeout: Duration,
    ) -> BenchResult<ProveOutput>;

    /// Verify a proof.
    ///
    /// # Arguments
    /// * `proof` - Path to the proof file
    /// * `vk` - Path to the verification key file
    ///
    /// # Returns
    /// VerifyOutput with timing and success status
    fn verify(&self, proof: &Path, vk: &Path) -> BenchResult<VerifyOutput>;

    /// Get gate information for a circuit.
    ///
    /// # Arguments
    /// * `artifact` - Path to the compiled circuit artifact
    ///
    /// # Returns
    /// GateInfo with gate counts and optional breakdown
    fn gate_info(&self, artifact: &Path) -> BenchResult<GateInfo>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capabilities_barretenberg() {
        let caps = Capabilities::barretenberg();
        assert!(caps.can_prove);
        assert!(caps.can_verify);
        assert!(caps.can_compile);
        assert!(caps.has_gate_count);
        assert!(caps.has_per_opcode_breakdown);
        assert!(caps.has_pk_vk_sizes);
    }

    #[test]
    fn test_gate_info_from_gates() {
        let info = GateInfo::from_gates(17);
        assert_eq!(info.backend_gates, 17);
        assert_eq!(info.subgroup_size, Some(32)); // next power of 2
        assert!(info.acir_opcodes.is_none());
        assert!(info.per_opcode.is_none());
    }

    #[test]
    fn test_gate_info_zero_gates() {
        let info = GateInfo::from_gates(0);
        assert_eq!(info.backend_gates, 0);
        assert!(info.subgroup_size.is_none());
    }
}
