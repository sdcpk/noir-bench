use noirc_driver::{compile_main, file_manager_with_stdlib, prepare_crate, CompileOptions};
use noirc_frontend::hir::Context;
use nargo::parse_all;
use tempfile::tempdir;

#[test]
fn prove_with_generic_backend() {
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
    std::fs::write(&program_path, serde_json::to_vec(&artifact).unwrap()).unwrap();
    // Minimal inputs
    let prover_toml = dir.path().join("Prover.toml");
    std::fs::write(&prover_toml, b"x = 1\n").unwrap();

    // Create a generic prover script that writes proof to output path
    let backend_path = dir.path().join("fake_prove.sh");
    let script = r#"#!/usr/bin/env bash
set -euo pipefail
# parse args to find -o/--output or last arg path
out=""
for i in "$@"; do
  if [[ "$i" == "-o" || "$i" == "--output" ]]; then
    shift
    out="$1"
  fi
done
if [[ -z "${out}" ]]; then out="proof.bin"; fi
echo -n 0001 > "${out}"
"#;
    std::fs::write(&backend_path, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&backend_path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&backend_path, perms).unwrap();
    }

    // Template using placeholders
    let template = format!("{} prove -b {{artifact}} -w {{witness}} -o {{proof}}", backend_path.to_string_lossy());

    noir_bench::prove_cmd::run(
        program_path.clone(),
        Some(prover_toml.clone()),
        Some("generic".to_string()),
        None,
        vec![],
        Some(template),
        5,
        Some(1),
        Some(0),
        None,
    )
    .unwrap();
}


