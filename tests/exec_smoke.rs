use noirc_driver::{CompileOptions, compile_main, file_manager_with_stdlib, prepare_crate};
use nargo::parse_all;
use noirc_frontend::hir::Context;
use tempfile::tempdir;

fn compile_unconstrained_program() -> noirc_driver::CompiledProgram {
    let root = std::path::Path::new("");
    let file_name = std::path::Path::new("main.nr");
    let mut fm = file_manager_with_stdlib(root);
    fm.add_file_with_source(file_name, r#"unconstrained fn main(x: Field) { let y = x + 1; }"#.to_string()).unwrap();
    let parsed = parse_all(&fm);
    let mut cx = Context::new(fm, parsed);
    let crate_id = prepare_crate(&mut cx, file_name);
    let opts = CompileOptions { force_brillig: true, ..Default::default() };
    let (compiled, _warnings) = compile_main(&mut cx, crate_id, &opts, None).expect("compile");
    compiled
}

#[test]
fn exec_smoke() {
    let compiled = compile_unconstrained_program();
    let artifact: noirc_artifacts::program::ProgramArtifact = compiled.clone().into();

    let dir = tempdir().unwrap();
    let program_path = dir.path().join("program.json");
    let prover_toml = dir.path().join("Prover.toml");

    // Write artifact
    std::fs::write(&program_path, serde_json::to_vec(&artifact).unwrap()).unwrap();
    // Write inputs
    std::fs::write(&prover_toml, b"x = 1\n").unwrap();

    // Run exec
    let out_dir = dir.path().join("out");
    std::fs::create_dir_all(&out_dir).unwrap();
    noir_bench::exec_cmd::run(program_path, prover_toml, Some(out_dir.clone()), None, false).unwrap();
} 