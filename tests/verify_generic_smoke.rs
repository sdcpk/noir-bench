use std::fs;
use std::os::unix::fs::PermissionsExt;

use noirc_driver::{compile_main, file_manager_with_stdlib, prepare_crate, CompileOptions};
use noirc_frontend::hir::Context;
use nargo::parse_all;
use tempfile::tempdir;

#[test]
fn verify_with_generic_backend() {
    // Compile a tiny program to get a valid artifact
    let root = std::path::Path::new("");
    let file_name = std::path::Path::new("main.nr");
    let mut fm = file_manager_with_stdlib(root);
    fm.add_file_with_source(file_name, r#"fn main(x: Field) { assert(x == 1); }"#.to_string()).unwrap();
    let parsed = parse_all(&fm);
    let mut cx = Context::new(fm, parsed);
    let crate_id = prepare_crate(&mut cx, file_name);
    let opts = CompileOptions { ..Default::default() };
    let (compiled, _warnings) = compile_main(&mut cx, crate_id, &opts, None).expect("compile");
    let artifact: noirc_artifacts::program::ProgramArtifact = compiled.into();

    let dir = tempdir().unwrap();
    let program_path = dir.path().join("program.json");
    fs::write(&program_path, serde_json::to_vec(&artifact).unwrap()).unwrap();

    // Create a fake backend script that exits 0 for verify
    let backend_path = dir.path().join("fake_verify.sh");
    let script = r#"#!/usr/bin/env bash
set -euo pipefail
# accept args; exit 0 to indicate ok
exit 0
"#;
    fs::write(&backend_path, script).unwrap();
    let mut perms = fs::metadata(&backend_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&backend_path, perms).unwrap();

    // Create a dummy proof file
    let proof_path = dir.path().join("proof.bin");
    fs::write(&proof_path, b"deadbeef").unwrap();

    // Use template to call script with placeholders
    let template = format!("{} --verify -b {{artifact}} -p {{proof}}", backend_path.to_string_lossy());

    noir_bench::verify_cmd::run(
        program_path.clone(),
        proof_path.clone(),
        Some("generic".to_string()),
        None,
        vec![],
        Some(template),
        Some(1),
        Some(0),
        None,
    )
    .unwrap();
}


