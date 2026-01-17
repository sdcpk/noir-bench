//! Toolchain abstraction for Noir compilation and witness generation.
//!
//! A `Toolchain` handles Noir-specific operations that are independent of the proving backend:
//! - Compilation: Noir source -> compiled artifact (ACIR + ABI)
//! - Witness generation: artifact + inputs -> witness
//!
//! This is distinct from `Backend` which handles proving system operations (prove, verify).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::BenchResult;

/// Output from a compilation operation.
#[derive(Debug, Clone)]
pub struct CompileArtifacts {
    /// Path to the compiled artifact (e.g., target/program.json)
    pub artifact_path: PathBuf,
    /// Compilation time in milliseconds
    pub compile_time_ms: u128,
}

/// Output from witness generation.
#[derive(Debug, Clone)]
pub struct WitnessArtifact {
    /// Path to the generated witness file
    pub witness_path: PathBuf,
    /// Witness generation time in milliseconds
    pub witness_gen_time_ms: u128,
}

/// Trait for Noir toolchain operations.
///
/// A toolchain is responsible for:
/// - Compiling Noir programs to artifacts
/// - Generating witnesses from artifacts and inputs
///
/// This is separate from the `Backend` trait which handles proving system operations.
/// The separation allows different Noir toolchains (nargo versions, etc.) to be used
/// with different proving backends (Barretenberg, etc.).
pub trait Toolchain: Send + Sync {
    /// Returns the toolchain name (e.g., "nargo").
    fn name(&self) -> &'static str;

    /// Returns the toolchain version, if detectable.
    fn version(&self) -> crate::BenchResult<String>;

    /// Compile a Noir project to an artifact.
    ///
    /// # Arguments
    /// * `project_dir` - Path to the Noir project directory (containing Nargo.toml)
    ///
    /// # Returns
    /// `CompileArtifacts` with path to compiled artifact and timing info
    fn compile(&self, project_dir: &Path) -> crate::BenchResult<CompileArtifacts>;

    /// Generate a witness from a compiled artifact and prover inputs.
    ///
    /// # Arguments
    /// * `artifact` - Path to the compiled artifact (program.json)
    /// * `prover_toml` - Path to Prover.toml with input values
    ///
    /// # Returns
    /// `WitnessArtifact` with path to witness file and timing info
    fn gen_witness(
        &self,
        artifact: &Path,
        prover_toml: &Path,
    ) -> crate::BenchResult<WitnessArtifact>;
}

/// Nargo toolchain implementation.
///
/// Shells out to the `nargo` CLI for compilation and witness generation.
/// This preserves the existing shell-out architecture (no FFI).
pub struct NargoToolchain {
    /// Path to the nargo binary (default: "nargo" from PATH)
    nargo_path: PathBuf,
    /// Timeout for nargo operations
    timeout: Duration,
}

impl Default for NargoToolchain {
    fn default() -> Self {
        Self::new()
    }
}

impl NargoToolchain {
    /// Create a new NargoToolchain using "nargo" from PATH.
    pub fn new() -> Self {
        NargoToolchain {
            nargo_path: PathBuf::from("nargo"),
            timeout: Duration::from_secs(300), // 5 minute default
        }
    }

    /// Create a NargoToolchain with a specific nargo binary path.
    pub fn with_path(nargo_path: impl Into<PathBuf>) -> Self {
        NargoToolchain {
            nargo_path: nargo_path.into(),
            timeout: Duration::from_secs(300),
        }
    }

    /// Set the timeout for nargo operations.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Get the path to the nargo binary.
    pub fn nargo_path(&self) -> &Path {
        &self.nargo_path
    }
}

/// Parse nargo version from command output.
///
/// Expected formats:
/// - "nargo version = 0.38.0" (older format)
/// - "nargo 0.38.0" (newer format)
/// - Just the version string
///
/// This function is public for testing purposes.
pub fn parse_nargo_version(output: &str) -> Option<String> {
    let output = output.trim();
    if output.is_empty() {
        return None;
    }

    // Try "nargo version = X.Y.Z" format
    if let Some(rest) = output.strip_prefix("nargo version = ") {
        let version = rest.split_whitespace().next()?;
        return Some(version.to_string());
    }

    // Try "nargo X.Y.Z" format
    if let Some(rest) = output.strip_prefix("nargo ") {
        let version = rest.split_whitespace().next()?;
        return Some(version.to_string());
    }

    // Fallback: if it looks like a version (starts with digit), use it
    let first_word = output.split_whitespace().next()?;
    if first_word.chars().next()?.is_ascii_digit() {
        return Some(first_word.to_string());
    }

    // Return the full output as-is if we can't parse it
    Some(output.to_string())
}

impl Toolchain for NargoToolchain {
    fn name(&self) -> &'static str {
        "nargo"
    }

    fn version(&self) -> BenchResult<String> {
        let output = Command::new(&self.nargo_path)
            .arg("--version")
            .output()
            .map_err(|e| {
                crate::BenchError::Message(format!("failed to run nargo --version: {}", e))
            })?;

        if !output.status.success() {
            return Err(crate::BenchError::Message(format!(
                "nargo --version failed with status: {}",
                output.status
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_nargo_version(&stdout).ok_or_else(|| {
            crate::BenchError::Message("failed to parse nargo version output".into())
        })
    }

    fn compile(&self, project_dir: &Path) -> BenchResult<CompileArtifacts> {
        let start = std::time::Instant::now();

        let status = Command::new(&self.nargo_path)
            .arg("compile")
            .current_dir(project_dir)
            .status()
            .map_err(|e| {
                crate::BenchError::Message(format!("failed to run nargo compile: {}", e))
            })?;

        let compile_time_ms = start.elapsed().as_millis();

        if !status.success() {
            return Err(crate::BenchError::Message(format!(
                "nargo compile failed with status: {}",
                status
            )));
        }

        // nargo compile outputs to target/<project_name>.json
        // For simplicity, look for any .json file in target/
        let target_dir = project_dir.join("target");
        let artifact_path = find_artifact_in_target(&target_dir)?;

        Ok(CompileArtifacts {
            artifact_path,
            compile_time_ms,
        })
    }

    fn gen_witness(&self, artifact: &Path, prover_toml: &Path) -> BenchResult<WitnessArtifact> {
        // For now, witness generation is done in-process using noir_artifact_cli
        // (same as existing prove_cmd.rs behavior).
        //
        // This is a placeholder that delegates to the existing in-process execution.
        // A future iteration could shell out to `nargo execute` instead.
        use bn254_blackbox_solver::Bn254BlackBoxSolver;
        use nargo::foreign_calls::DefaultForeignCallBuilder;
        use noir_artifact_cli::execution::execute as execute_program_artifact;
        use noir_artifact_cli::fs::artifact::read_program_from_file;
        use noir_artifact_cli::fs::witness::save_witness_to_dir;

        let start = std::time::Instant::now();

        // Read the compiled program
        let program = read_program_from_file(artifact)
            .map_err(|e| crate::BenchError::Message(format!("failed to read artifact: {}", e)))?;

        let compiled: noirc_driver::CompiledProgram = program.into();

        // Execute to generate witness
        let exec_res = execute_program_artifact(
            &compiled,
            &Bn254BlackBoxSolver(false),
            &mut DefaultForeignCallBuilder::default().build(),
            prover_toml,
        )
        .map_err(|e| crate::BenchError::Message(format!("witness generation failed: {}", e)))?;

        // Save witness to temp directory
        let tempdir = tempfile::tempdir()
            .map_err(|e| crate::BenchError::Message(format!("failed to create temp dir: {}", e)))?;
        let witness_path = save_witness_to_dir(&exec_res.witness_stack, "witness", tempdir.path())
            .map_err(|e| crate::BenchError::Message(format!("failed to save witness: {}", e)))?;

        let witness_gen_time_ms = start.elapsed().as_millis();

        // Note: The tempdir will be dropped after this function returns.
        // In a real implementation, we'd want to persist this or pass ownership.
        // For now, we copy to a stable location.
        let stable_witness_path =
            std::env::temp_dir().join(format!("noir-bench-witness-{}.gz", std::process::id()));
        std::fs::copy(&witness_path, &stable_witness_path)
            .map_err(|e| crate::BenchError::Message(format!("failed to copy witness: {}", e)))?;

        Ok(WitnessArtifact {
            witness_path: stable_witness_path,
            witness_gen_time_ms,
        })
    }
}

/// Find the compiled artifact in the target directory.
fn find_artifact_in_target(target_dir: &Path) -> BenchResult<PathBuf> {
    if !target_dir.exists() {
        return Err(crate::BenchError::Message(format!(
            "target directory does not exist: {}",
            target_dir.display()
        )));
    }

    // Look for .json files (compiled artifacts)
    let entries = std::fs::read_dir(target_dir)
        .map_err(|e| crate::BenchError::Message(format!("failed to read target dir: {}", e)))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map(|e| e == "json").unwrap_or(false) {
            return Ok(path);
        }
    }

    Err(crate::BenchError::Message(format!(
        "no .json artifact found in {}",
        target_dir.display()
    )))
}

/// Mock toolchain for testing purposes.
///
/// Returns configurable fixed responses without executing any commands.
#[derive(Debug, Clone)]
pub struct MockToolchain {
    /// Name to report
    pub mock_name: &'static str,
    /// Version to report
    pub mock_version: String,
    /// Compile output (if Some)
    pub compile_output: Option<CompileArtifacts>,
    /// Witness output (if Some)
    pub witness_output: Option<WitnessArtifact>,
    /// Whether operations should fail
    pub should_fail: bool,
}

impl Default for MockToolchain {
    fn default() -> Self {
        MockToolchain {
            mock_name: "mock-nargo",
            mock_version: "0.38.0-mock".to_string(),
            compile_output: Some(CompileArtifacts {
                artifact_path: PathBuf::from("/tmp/mock-artifact.json"),
                compile_time_ms: 50,
            }),
            witness_output: Some(WitnessArtifact {
                witness_path: PathBuf::from("/tmp/mock-witness.gz"),
                witness_gen_time_ms: 25,
            }),
            should_fail: false,
        }
    }
}

impl MockToolchain {
    /// Create a new MockToolchain with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the version to report.
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.mock_version = version.into();
        self
    }

    /// Make all operations fail.
    pub fn failing(mut self) -> Self {
        self.should_fail = true;
        self
    }
}

impl Toolchain for MockToolchain {
    fn name(&self) -> &'static str {
        self.mock_name
    }

    fn version(&self) -> BenchResult<String> {
        if self.should_fail {
            return Err(crate::BenchError::Message("mock toolchain failed".into()));
        }
        Ok(self.mock_version.clone())
    }

    fn compile(&self, _project_dir: &Path) -> BenchResult<CompileArtifacts> {
        if self.should_fail {
            return Err(crate::BenchError::Message("mock compile failed".into()));
        }
        self.compile_output
            .clone()
            .ok_or_else(|| crate::BenchError::Message("no compile output configured".into()))
    }

    fn gen_witness(&self, _artifact: &Path, _prover_toml: &Path) -> BenchResult<WitnessArtifact> {
        if self.should_fail {
            return Err(crate::BenchError::Message("mock witness gen failed".into()));
        }
        self.witness_output
            .clone()
            .ok_or_else(|| crate::BenchError::Message("no witness output configured".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_nargo_version_with_equals() {
        let output = "nargo version = 0.38.0\n";
        assert_eq!(parse_nargo_version(output), Some("0.38.0".to_string()));
    }

    #[test]
    fn test_parse_nargo_version_simple() {
        let output = "nargo 0.38.0";
        assert_eq!(parse_nargo_version(output), Some("0.38.0".to_string()));
    }

    #[test]
    fn test_parse_nargo_version_with_extra_info() {
        let output = "nargo version = 0.38.0 (git hash abc123)";
        assert_eq!(parse_nargo_version(output), Some("0.38.0".to_string()));
    }

    #[test]
    fn test_parse_nargo_version_just_version() {
        let output = "0.38.0";
        assert_eq!(parse_nargo_version(output), Some("0.38.0".to_string()));
    }

    #[test]
    fn test_parse_nargo_version_empty() {
        assert_eq!(parse_nargo_version(""), None);
        assert_eq!(parse_nargo_version("   "), None);
    }

    #[test]
    fn test_parse_nargo_version_fallback() {
        // If it doesn't match known patterns and doesn't start with digit,
        // return the full string
        let output = "some-unknown-format";
        assert_eq!(
            parse_nargo_version(output),
            Some("some-unknown-format".to_string())
        );
    }

    #[test]
    fn test_mock_toolchain_defaults() {
        let mock = MockToolchain::new();
        assert_eq!(mock.name(), "mock-nargo");
        assert!(mock.version().is_ok());
        assert_eq!(mock.version().unwrap(), "0.38.0-mock");
    }

    #[test]
    fn test_mock_toolchain_version_override() {
        let mock = MockToolchain::new().with_version("1.0.0-test");
        assert_eq!(mock.version().unwrap(), "1.0.0-test");
    }

    #[test]
    fn test_mock_toolchain_failing() {
        let mock = MockToolchain::new().failing();
        assert!(mock.version().is_err());
        assert!(mock.compile(Path::new("/fake")).is_err());
        assert!(
            mock.gen_witness(Path::new("/fake"), Path::new("/fake"))
                .is_err()
        );
    }

    #[test]
    fn test_mock_toolchain_compile() {
        let mock = MockToolchain::new();
        let result = mock.compile(Path::new("/fake/project"));
        assert!(result.is_ok());
        let artifacts = result.unwrap();
        assert_eq!(artifacts.compile_time_ms, 50);
    }

    #[test]
    fn test_mock_toolchain_witness() {
        let mock = MockToolchain::new();
        let result = mock.gen_witness(Path::new("/artifact.json"), Path::new("/Prover.toml"));
        assert!(result.is_ok());
        let witness = result.unwrap();
        assert_eq!(witness.witness_gen_time_ms, 25);
    }
}
