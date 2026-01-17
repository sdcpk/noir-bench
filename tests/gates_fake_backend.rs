use std::fs;
use std::os::unix::fs::PermissionsExt;

use nargo::parse_all;
use noirc_driver::{CompileOptions, compile_main, file_manager_with_stdlib, prepare_crate};
use noirc_frontend::hir::Context;
use tempfile::tempdir;

#[test]
fn gates_with_fake_backend() {
    // Compile a tiny program to get a valid artifact
    let root = std::path::Path::new("");
    let file_name = std::path::Path::new("main.nr");
    let mut fm = file_manager_with_stdlib(root);
    fm.add_file_with_source(
        file_name,
        r#"fn main(x: Field) { assert(x == 1); }"#.to_string(),
    )
    .unwrap();
    let parsed = parse_all(&fm);
    let mut cx = Context::new(fm, parsed);
    let crate_id = prepare_crate(&mut cx, file_name);
    let opts = CompileOptions {
        ..Default::default()
    };
    let (compiled, _warnings) = compile_main(&mut cx, crate_id, &opts, None).expect("compile");
    let artifact: noirc_artifacts::program::ProgramArtifact = compiled.into();

    let dir = tempdir().unwrap();
    let program_path = dir.path().join("program.json");
    fs::write(&program_path, serde_json::to_vec(&artifact).unwrap()).unwrap();

    // Create a fake backend script that prints gates JSON to stdout
    let backend_path = dir.path().join("fake_backend.sh");
    let script = r#"#!/usr/bin/env bash
set -euo pipefail
# Accept any args but ignore them; print expected JSON shape
cat <<'JSON'
{"functions":[{"acir_opcodes": 3, "circuit_size": 10, "gates_per_opcode": [4,3,3]}]}
JSON
"#;
    fs::write(&backend_path, script).unwrap();
    let mut perms = fs::metadata(&backend_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&backend_path, perms).unwrap();

    // Run gates_cmd
    noir_bench::gates_cmd::run(
        program_path.clone(),
        Some("fake".to_string()),
        Some(backend_path.clone()),
        vec!["--include_gates_per_opcode".into()],
        None,
        None,
    )
    .unwrap();
}
