//! Mock backend for testing.

use std::path::Path;
use std::time::Duration;

use crate::BenchResult;

use super::traits::{Backend, Capabilities, GateInfo, ProveOutput, VerifyOutput};

/// Configuration for mock backend responses.
#[derive(Debug, Clone, Default)]
pub struct MockConfig {
    /// Name to report
    pub name: String,
    /// Version to report
    pub version: Option<String>,
    /// Capabilities to report
    pub capabilities: Capabilities,
    /// Prove output to return
    pub prove_output: Option<ProveOutput>,
    /// Verify output to return
    pub verify_output: Option<VerifyOutput>,
    /// Gate info to return
    pub gate_info: Option<GateInfo>,
    /// Whether prove should fail
    pub prove_fails: bool,
    /// Whether verify should fail
    pub verify_fails: bool,
    /// Whether gate_info should fail
    pub gate_info_fails: bool,
}

impl MockConfig {
    /// Create a new mock config with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        MockConfig {
            name: name.into(),
            version: Some("mock-1.0.0".to_string()),
            capabilities: Capabilities::barretenberg(),
            prove_output: Some(ProveOutput {
                prove_time_ms: 100,
                witness_gen_time_ms: Some(10),
                backend_prove_time_ms: Some(90),
                peak_memory_bytes: Some(100_000_000),
                proof_size_bytes: Some(4096),
                proving_key_size_bytes: None,
                verification_key_size_bytes: Some(1024),
                proof_path: None,
                vk_path: None,
            }),
            verify_output: Some(VerifyOutput {
                verify_time_ms: 50,
                success: true,
            }),
            gate_info: Some(GateInfo {
                backend_gates: 1000,
                subgroup_size: Some(1024),
                acir_opcodes: Some(50),
                per_opcode: None,
            }),
            prove_fails: false,
            verify_fails: false,
            gate_info_fails: false,
        }
    }

    /// Set the prove output.
    pub fn with_prove_output(mut self, output: ProveOutput) -> Self {
        self.prove_output = Some(output);
        self
    }

    /// Set the verify output.
    pub fn with_verify_output(mut self, output: VerifyOutput) -> Self {
        self.verify_output = Some(output);
        self
    }

    /// Set the gate info.
    pub fn with_gate_info(mut self, info: GateInfo) -> Self {
        self.gate_info = Some(info);
        self
    }

    /// Make prove fail.
    pub fn prove_fails(mut self) -> Self {
        self.prove_fails = true;
        self
    }

    /// Make verify fail.
    pub fn verify_fails(mut self) -> Self {
        self.verify_fails = true;
        self
    }

    /// Make gate_info fail.
    pub fn gate_info_fails(mut self) -> Self {
        self.gate_info_fails = true;
        self
    }

    /// Set capabilities.
    pub fn with_capabilities(mut self, caps: Capabilities) -> Self {
        self.capabilities = caps;
        self
    }
}

/// Mock backend for unit testing.
///
/// This backend returns configurable fake results without performing
/// any actual proving or verification operations.
pub struct MockBackend {
    config: MockConfig,
}

impl MockBackend {
    /// Create a new mock backend with the given configuration.
    pub fn new(config: MockConfig) -> Self {
        MockBackend { config }
    }

    /// Create a mock backend with default configuration.
    pub fn default_mock() -> Self {
        Self::new(MockConfig::new("mock"))
    }

    /// Create a mock backend that always succeeds with given gate count.
    pub fn with_gates(gates: u64) -> Self {
        let mut config = MockConfig::new("mock");
        config.gate_info = Some(GateInfo::from_gates(gates));
        Self::new(config)
    }
}

impl Backend for MockBackend {
    fn name(&self) -> &str {
        &self.config.name
    }

    fn version(&self) -> Option<String> {
        self.config.version.clone()
    }

    fn capabilities(&self) -> Capabilities {
        self.config.capabilities.clone()
    }

    fn prove(
        &self,
        _artifact: &Path,
        _witness: Option<&Path>,
        _timeout: Duration,
    ) -> BenchResult<ProveOutput> {
        if self.config.prove_fails {
            return Err(crate::BenchError::Message("mock prove failed".into()));
        }
        self.config
            .prove_output
            .clone()
            .ok_or_else(|| crate::BenchError::Message("no prove output configured".into()))
    }

    fn verify(&self, _proof: &Path, _vk: &Path) -> BenchResult<VerifyOutput> {
        if self.config.verify_fails {
            return Err(crate::BenchError::Message("mock verify failed".into()));
        }
        self.config
            .verify_output
            .clone()
            .ok_or_else(|| crate::BenchError::Message("no verify output configured".into()))
    }

    fn gate_info(&self, _artifact: &Path) -> BenchResult<GateInfo> {
        if self.config.gate_info_fails {
            return Err(crate::BenchError::Message("mock gate_info failed".into()));
        }
        self.config
            .gate_info
            .clone()
            .ok_or_else(|| crate::BenchError::Message("no gate info configured".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_mock_backend_default() {
        let backend = MockBackend::default_mock();
        assert_eq!(backend.name(), "mock");
        assert!(backend.version().is_some());
    }

    #[test]
    fn test_mock_backend_prove() {
        let backend = MockBackend::default_mock();
        let result = backend.prove(Path::new("test.json"), None, Duration::from_secs(10));
        assert!(result.is_ok());
        let output = result.unwrap();
        assert_eq!(output.prove_time_ms, 100);
    }

    #[test]
    fn test_mock_backend_prove_fails() {
        let config = MockConfig::new("mock").prove_fails();
        let backend = MockBackend::new(config);
        let result = backend.prove(Path::new("test.json"), None, Duration::from_secs(10));
        assert!(result.is_err());
    }

    #[test]
    fn test_mock_backend_verify() {
        let backend = MockBackend::default_mock();
        let result = backend.verify(Path::new("proof"), Path::new("vk"));
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.success);
        assert_eq!(output.verify_time_ms, 50);
    }

    #[test]
    fn test_mock_backend_gate_info() {
        let backend = MockBackend::with_gates(2048);
        let result = backend.gate_info(Path::new("test.json"));
        assert!(result.is_ok());
        let info = result.unwrap();
        assert_eq!(info.backend_gates, 2048);
        assert_eq!(info.subgroup_size, Some(2048)); // already a power of 2
    }

    #[test]
    fn test_mock_config_builder() {
        let config = MockConfig::new("custom")
            .with_prove_output(ProveOutput {
                prove_time_ms: 500,
                ..Default::default()
            })
            .with_gate_info(GateInfo {
                backend_gates: 5000,
                subgroup_size: Some(8192),
                acir_opcodes: Some(100),
                per_opcode: Some(HashMap::from([
                    ("add".to_string(), 1000),
                    ("mul".to_string(), 2000),
                ])),
            });

        let backend = MockBackend::new(config);

        let prove = backend
            .prove(Path::new("x"), None, Duration::from_secs(1))
            .unwrap();
        assert_eq!(prove.prove_time_ms, 500);

        let gates = backend.gate_info(Path::new("x")).unwrap();
        assert_eq!(gates.backend_gates, 5000);
        assert!(gates.per_opcode.is_some());
    }
}
