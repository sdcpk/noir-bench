use noirc_driver::{CompileOptions, compile_main, file_manager_with_stdlib, prepare_crate};
use nargo::parse_all;
use noirc_frontend::hir::Context;
use tempfile::tempdir;

fn compile_program() -> noirc_driver::CompiledProgram {
    let root = std::path::Path::new("");
    let file_name = std::path::Path::new("main.nr");
    let mut fm = file_manager_with_stdlib(root);
    fm.add_file_with_source(file_name, r#"fn main(x: Field) { assert(x == x); }"#.to_string()).unwrap();
    let parsed = parse_all(&fm);
    let mut cx = Context::new(fm, parsed);
    let crate_id = prepare_crate(&mut cx, file_name);
    let opts = CompileOptions { ..Default::default() };
    let (compiled, _warnings) = compile_main(&mut cx, crate_id, &opts, None).expect("compile");
    compiled
}

#[test]
fn gates_smoke_with_mock_backend() {
    let compiled = compile_program();
    let artifact: noirc_artifacts::program::ProgramArtifact = compiled.clone().into();

    let dir = tempdir().unwrap();
    let program_path = dir.path().join("program.json");
    let out_json = dir.path().join("gates.json");

    // Write artifact
    std::fs::write(&program_path, serde_json::to_vec(&artifact).unwrap()).unwrap();

    // Create a mock backend script
    let backend = dir.path().join("bb-mock.sh");
    let script = r#"#!/bin/sh
# ignore args; just print expected JSON to stdout
cat <<'JSON'
{"functions":[{"acir_opcodes":3,"circuit_size":100,"gates_per_opcode":[10,20,70]}]}
JSON
"#;
    std::fs::write(&backend, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&backend).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&backend, perms).unwrap();
    }

    // Run gates
    noir_bench::gates_cmd::run(
        program_path,
        Some("barretenberg".into()),
        Some(backend),
        vec!["--include_gates_per_opcode".into()],
        Some(out_json.clone()),
    )
    .unwrap();

    // Validate JSON report
    let bytes = std::fs::read(&out_json).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["total_gates"], 100);
    assert_eq!(v["acir_opcodes"], 3);
} 