use std::path::{Path, PathBuf};
use std::time::Instant;

use bn254_blackbox_solver::Bn254BlackBoxSolver;
use noir_artifact_cli::fs::{artifact::read_program_from_file, inputs::read_inputs_from_file};
use noirc_artifacts::debug::DebugArtifact;
use tracing::info;

use crate::{BenchError, BenchResult, CommonMeta, ExecReport};

#[cfg(feature = "mem")]
fn capture_peak_mem() -> Option<u64> {
    use sysinfo::{MemoryRefreshKind, RefreshKind, System};
    let mut sys = System::new_with_specifics(RefreshKind::new().with_memory(MemoryRefreshKind::new().with_ram()));
    sys.refresh_memory();
    Some(sys.total_memory() - sys.free_memory())
}

#[cfg(not(feature = "mem"))]
fn capture_peak_mem() -> Option<u64> { None }

fn now_string() -> String {
    time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339).unwrap_or_else(|_| "".to_string())
}

fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> BenchResult<()> {
    if let Some(dir) = path.parent() { std::fs::create_dir_all(dir).map_err(|e| BenchError::Message(e.to_string()))?; }
    let json = serde_json::to_vec_pretty(value).map_err(|e| BenchError::Message(e.to_string()))?;
    std::fs::write(path, json).map_err(|e| BenchError::Message(e.to_string()))
}

pub fn run(
    artifact: PathBuf,
    prover_toml: PathBuf,
    output_dir: Option<PathBuf>,
    json_out: Option<PathBuf>,
    flamegraph: bool,
) -> BenchResult<()> {
    info!("loading artifact");
    let mut program = read_program_from_file(&artifact).map_err(|e| BenchError::Message(e.to_string()))?;

    // Inputs
    let (inputs_map, _) = read_inputs_from_file(&prover_toml.with_extension("toml"), &program.abi)
        .map_err(|e| BenchError::Message(e.to_string()))?;
    let initial_witness = program.abi.encode(&inputs_map, None).map_err(|e| BenchError::Message(e.to_string()))?;

    // Execute with profiling
    info!("executing (profiling)");
    let start = Instant::now();
    let (_witness_stack, mut profiling_samples) = nargo::ops::execute_program_with_profiling(
        &program.bytecode,
        initial_witness,
        &Bn254BlackBoxSolver(false),
        &mut nargo::foreign_calls::DefaultForeignCallBuilder::default().with_output(std::io::stdout()).build(),
    )
    .map_err(|e| BenchError::Message(format!("execution failed: {e}")))?;
    let duration_ms = start.elapsed().as_millis();
    let samples_count = profiling_samples.len();

    // Optional flamegraph
    let mut flamegraph_svg = None;
    if flamegraph {
        let Some(out_dir) = output_dir.as_ref() else {
            return Err(BenchError::Message("--output is required when --flamegraph is set".to_string()));
        };
        std::fs::create_dir_all(out_dir).map_err(|e| BenchError::Message(e.to_string()))?;

        // Build debug artifact view
        let debug_artifact: DebugArtifact = program.clone().into();

        // Convert ACVM profiling samples into profiler-like samples lines
        let samples: Vec<exec_samples::BrilligExecSample> = {
            use acvm::acir::circuit::OpcodeLocation;
            profiling_samples
                .iter_mut()
                .map(|s| {
                    let call_stack = std::mem::take(&mut s.call_stack);
                    let brillig_function_id = std::mem::take(&mut s.brillig_function_id);
                    let last_entry = call_stack.last();
                    let opcode = brillig_function_id
                        .and_then(|id| program.bytecode.unconstrained_functions.get(id.0 as usize))
                        .and_then(|func| {
                            if let Some(OpcodeLocation::Brillig { brillig_index, .. }) = last_entry {
                                func.bytecode.get(*brillig_index)
                            } else { None }
                        })
                        .map(exec_samples::format_brillig_opcode);
                    exec_samples::BrilligExecSample { opcode, call_stack, brillig_function_id }
                })
                .collect()
        };

        let artifact_name = artifact.file_name().and_then(|s| s.to_str()).unwrap_or("artifact");
        let svg_path = out_dir.join(format!("{}_brillig_trace.svg", "main"));
        flame::generate_flamegraph(
            samples,
            &debug_artifact.debug_symbols[0],
            &debug_artifact,
            artifact_name,
            "main",
            &svg_path,
        ).map_err(|e| BenchError::Message(format!("flamegraph failed: {e}")))?;
        flamegraph_svg = Some(svg_path);
    }

    // Build report
    let meta = CommonMeta {
        name: "exec".to_string(),
        timestamp: now_string(),
        noir_version: program.noir_version.clone(),
        artifact_path: artifact.clone(),
    };
    let report = ExecReport { meta, execution_time_ms: duration_ms, samples_count, peak_memory_bytes: capture_peak_mem(), flamegraph_svg };

    // Output JSON
    if let Some(json_path) = json_out { write_json(&json_path, &report)?; }

    // Human summary
    println!(
        "exec: time={}ms samples={}{}",
        report.execution_time_ms,
        report.samples_count,
        if report.flamegraph_svg.is_some() { " (flamegraph)" } else { "" }
    );

    Ok(())
}

// Minimal internal helpers to avoid depending on profiler crate
mod exec_samples {
    use acvm::{FieldElement};
    use acvm::acir::brillig::Opcode as BrilligOpcode;
    use acvm::acir::circuit::{OpcodeLocation, brillig::BrilligFunctionId};

    #[derive(Clone)]
    pub struct BrilligExecSample {
        pub opcode: Option<String>,
        pub call_stack: Vec<OpcodeLocation>,
        pub brillig_function_id: Option<BrilligFunctionId>,
    }

    pub fn format_brillig_opcode(opcode: &BrilligOpcode<FieldElement>) -> String {
        use acvm::acir::brillig::Opcode as Op;
        match opcode {
            Op::CalldataCopy { .. } => "brillig::calldata_copy",
            Op::Const { .. } => "brillig::const",
            Op::BinaryFieldOp { .. } => "brillig::field_op",
            Op::BinaryIntOp { .. } => "brillig::int_op",
            Op::Not { .. } => "brillig::not",
            Op::Cast { .. } => "brillig::cast",
            Op::JumpIf { .. } => "brillig::jump_if",
            Op::JumpIfNot { .. } => "brillig::jump_if_not",
            Op::Jump { .. } => "brillig::jump",
            Op::Mov { .. } => "brillig::mov",
            Op::Cast { .. } => "brillig::cast",
            Op::Not { .. } => "brillig::not",
            Op::JumpIf { .. } => "brillig::jump_if",
            Op::Jump { .. } => "brillig::jump",
            Op::Mov { .. } => "brillig::mov",
            Op::Return { .. } => "brillig::return",
            Op::Store { .. } => "brillig::store",
            Op::Load { .. } => "brillig::load",
            Op::ForeignCall { .. } => "brillig::foreign_call",
            _ => "brillig::op",
        }.to_string()
    }
}

mod flame {
    use std::{io::BufWriter, path::Path};

    use color_eyre::eyre;
    use fm::codespan_files::Files;
    use inferno::flamegraph::{Options, TextTruncateDirection, from_lines};
    use noirc_errors::debug_info::DebugInfo;

    use super::exec_samples::BrilligExecSample;
    use super::profiler_like;

    pub fn generate_flamegraph<'files>(
        samples: Vec<BrilligExecSample>,
        debug_symbols: &DebugInfo,
        files: &'files impl Files<'files, FileId = fm::FileId>,
        artifact_name: &str,
        function_name: &str,
        output_path: &Path,
    ) -> eyre::Result<()> {
        let folded_lines = profiler_like::generate_folded_sorted_lines(samples, debug_symbols, files);
        let flamegraph_file = std::fs::File::create(output_path)?;
        let flamegraph_writer = BufWriter::new(flamegraph_file);

        let mut options = Options::default();
        options.hash = true;
        options.deterministic = true;
        options.title = format!("Artifact: {artifact_name}, Function: {function_name}");
        options.frame_height = 24;
        options.color_diffusion = true;
        options.min_width = 0.0;
        options.count_name = "samples".to_string();
        options.text_truncate_direction = TextTruncateDirection::Right;

        from_lines(&mut options, folded_lines.iter().map(|s| s.as_str()), flamegraph_writer)?;
        Ok(())
    }
}

mod profiler_like {
    use std::collections::BTreeMap;

    use acvm::acir::circuit::{AcirOpcodeLocation, OpcodeLocation};
    use fm::codespan_files::Files;
    use noirc_errors::Location;
    use noirc_errors::{debug_info::DebugInfo, reporter::line_and_column_from_span};

    use super::exec_samples::BrilligExecSample;

    #[derive(Default)]
    struct FoldedStackItem { total: usize, children: BTreeMap<String, FoldedStackItem> }

    pub fn generate_folded_sorted_lines<'files>(
        samples: Vec<BrilligExecSample>,
        debug_symbols: &DebugInfo,
        files: &'files impl Files<'files, FileId = fm::FileId>,
    ) -> Vec<String> {
        let mut root: BTreeMap<String, FoldedStackItem> = BTreeMap::new();
        for s in samples {
            let mut labels: Vec<String> = Vec::new();
            for loc in s.call_stack.iter() {
                labels.extend(find_callsite_labels(debug_symbols, loc, s.brillig_function_id, files));
            }
            if let Some(op) = s.opcode.as_ref() { labels.push(op.clone()); }
            add(&mut root, labels, 1);
        }
        to_lines(&root, im::Vector::new())
    }

    fn add(root: &mut BTreeMap<String, FoldedStackItem>, labels: Vec<String>, count: usize) {
        let mut map = root;
        for (i, l) in labels.iter().enumerate() {
            let entry = map.entry(l.clone()).or_default();
            if i == labels.len() - 1 { entry.total += count; }
            map = &mut entry.children;
        }
    }

    fn to_lines(root: &BTreeMap<String, FoldedStackItem>, parents: im::Vector<String>) -> Vec<String> {
        let mut out = Vec::new();
        for (label, item) in root.iter() {
            if item.total > 0 {
                let frames: Vec<String> = parents.iter().cloned().chain(std::iter::once(label.clone())).collect();
                out.push(format!("{} {}", frames.join(";"), item.total));
            }
            let mut ps = parents.clone();
            ps.push_back(label.clone());
            out.extend(to_lines(&item.children, ps));
        }
        out
    }

    fn find_callsite_labels<'files>(
        debug_symbols: &DebugInfo,
        opcode_location: &OpcodeLocation,
        brillig_function_id: Option<acvm::acir::circuit::brillig::BrilligFunctionId>,
        files: &'files impl Files<'files, FileId = fm::FileId>,
    ) -> Vec<String> {
        match opcode_location {
            OpcodeLocation::Acir(idx) => debug_symbols
                .acir_opcode_location(&AcirOpcodeLocation::new(*idx))
                .unwrap_or_default()
                .into_iter()
                .map(|loc| location_to_label(loc, files))
                .collect(),
            OpcodeLocation::Brillig { .. } => {
                if let (Some(brillig_function_id), Some(brillig_location)) =
                    (brillig_function_id, opcode_location.to_brillig_location())
                {
                    if let Some(brillig_locations) = debug_symbols.brillig_locations.get(&brillig_function_id) {
                        if let Some(call_stack) = brillig_locations.get(&brillig_location) {
                            return debug_symbols
                                .location_tree
                                .get_call_stack(*call_stack)
                                .into_iter()
                                .map(|loc| location_to_label(loc, files))
                                .collect();
                        }
                    }
                }
                vec![]
            }
        }
    }

    fn location_to_label<'files>(location: Location, files: &'files impl Files<'files, FileId = fm::FileId>) -> String {
        let filename = std::path::Path::new(&files.name(location.file).expect("file path").to_string())
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or("invalid_path".to_string());
        let source = files.source(location.file).expect("file source");
        let code_slice: String = source
            .as_ref()
            .chars()
            .skip(location.span.start() as usize)
            .take(location.span.end() as usize - location.span.start() as usize)
            .collect();
        let code_slice = code_slice.replace(';', "\u{037E}");
        let (line, column) = line_and_column_from_span(source.as_ref(), &location.span);
        format!("{filename}:{line}:{column}::{code_slice}")
    }
} 