//! Integration tests for the unified Backend trait.
//!
//! These tests verify that the new `Backend` abstraction works correctly
//! with real backends (barretenberg) when available.

use std::path::PathBuf;
use std::process::Command;

use noir_bench::backend::{Backend, BarretenbergBackend, BarretenbergConfig};

/// Check if bb (barretenberg) is available in PATH.
fn bb_available() -> Option<PathBuf> {
    let bb_path = std::env::var_os("BB_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("bb"));
    if Command::new(&bb_path)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        Some(bb_path)
    } else {
        None
    }
}

/// Check if the simple_hash artifact exists.
fn simple_hash_artifact() -> Option<PathBuf> {
    let paths = [
        "examples/simple_hash/target/simple_hash.json",
        "examples/simple_hash/target/main.json",
    ];

    for path in paths {
        let p = PathBuf::from(path);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

#[test]
fn test_barretenberg_backend_gate_info() {
    // Skip if bb is not available
    let bb_path = match bb_available() {
        Some(p) => p,
        None => {
            eprintln!("Skipping test: bb not found in PATH");
            return;
        }
    };

    // Skip if artifact doesn't exist
    let artifact = match simple_hash_artifact() {
        Some(p) => p,
        None => {
            eprintln!(
                "Skipping test: simple_hash artifact not found (run 'nargo compile' in examples/simple_hash first)"
            );
            return;
        }
    };

    // Create backend
    let config = BarretenbergConfig::new(&bb_path);
    let backend = BarretenbergBackend::new(config);

    // Verify backend properties
    assert_eq!(backend.name(), "barretenberg");

    let caps = backend.capabilities();
    assert!(caps.can_prove);
    assert!(caps.can_verify);
    assert!(caps.has_gate_count);

    // Get gate info
    let gate_info = backend
        .gate_info(&artifact)
        .expect("gate_info should succeed");

    // Verify gate info has valid data
    assert!(gate_info.backend_gates > 0, "should have some gates");
    assert!(
        gate_info.subgroup_size.is_some(),
        "should have subgroup size"
    );
    assert!(
        gate_info.acir_opcodes.is_some(),
        "should have acir opcode count"
    );

    // Subgroup size should be a power of 2
    if let Some(sg) = gate_info.subgroup_size {
        assert!(sg.is_power_of_two(), "subgroup size should be power of 2");
        assert!(sg >= gate_info.backend_gates, "subgroup >= gates");
    }

    println!(
        "gate_info result: backend_gates={}, subgroup={:?}, acir_opcodes={:?}",
        gate_info.backend_gates, gate_info.subgroup_size, gate_info.acir_opcodes
    );
}

#[test]
fn test_barretenberg_backend_version() {
    let bb_path = match bb_available() {
        Some(p) => p,
        None => {
            eprintln!("Skipping test: bb not found in PATH");
            return;
        }
    };

    let config = BarretenbergConfig::new(&bb_path);
    let backend = BarretenbergBackend::new(config);

    // Version should be available
    let version = backend.version();
    assert!(version.is_some(), "version should be detected");

    let v = version.unwrap();
    assert!(!v.is_empty(), "version should not be empty");
    println!("bb version: {}", v);
}

/// Test that gate_info with per-opcode breakdown works.
#[test]
fn test_barretenberg_gate_info_with_opcode_breakdown() {
    let bb_path = match bb_available() {
        Some(p) => p,
        None => {
            eprintln!("Skipping test: bb not found in PATH");
            return;
        }
    };

    let artifact = match simple_hash_artifact() {
        Some(p) => p,
        None => {
            eprintln!("Skipping test: simple_hash artifact not found");
            return;
        }
    };

    // Use --include_gates_per_opcode flag
    let config =
        BarretenbergConfig::new(&bb_path).with_args(vec!["--include_gates_per_opcode".into()]);
    let backend = BarretenbergBackend::new(config);

    let gate_info = backend
        .gate_info(&artifact)
        .expect("gate_info should succeed");

    // With the opcode flag, we should get per-opcode breakdown
    if let Some(ref per_opcode) = gate_info.per_opcode {
        assert!(!per_opcode.is_empty(), "per_opcode should have entries");

        // Sum of per-opcode gates should roughly match total (may not be exact due to overhead)
        let sum: u64 = per_opcode.values().sum();
        println!(
            "per_opcode sum: {}, total: {}",
            sum, gate_info.backend_gates
        );
    }
}

/// This test is marked as #[ignore] because it requires bb and compiled artifacts.
/// Run with: cargo test -- --ignored
#[test]
#[ignore]
fn test_barretenberg_prove_requires_witness() {
    let bb_path = bb_available().expect("bb required for this test");
    let artifact = simple_hash_artifact().expect("artifact required for this test");

    let config = BarretenbergConfig::new(&bb_path);
    let backend = BarretenbergBackend::new(config);

    // Prove without witness should error
    let result = backend.prove(&artifact, None, std::time::Duration::from_secs(10));
    assert!(result.is_err(), "prove without witness should fail");
}
