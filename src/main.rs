#![forbid(unsafe_code)]

use clap::{Parser, Subcommand};
use tracing_subscriber::{EnvFilter, fmt::format::FmtSpan};

use noir_bench::{CsvExporter, JsonlWriter};
use noir_bench::{
    bench, ci_cmd, compare_cmd, evm_verify_cmd, exec_cmd, gates_cmd, history_cmd, prove_cmd,
    suite_cmd, verify_cmd,
};
use serde_json::Value as JsonValue;

#[derive(Parser, Debug)]
#[command(name = "noir-bench")]
#[command(about = "Benchmark suite for Noir execution and proving", long_about = None)]
struct Cli {
    /// Enable verbose logging (or set NOIR_BENCH_LOG)
    #[arg(long)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
    /// Export results to CSV (where applicable)
    #[arg(long)]
    csv: Option<std::path::PathBuf>,
    /// Export results to Markdown (where applicable)
    #[arg(long)]
    md: Option<std::path::PathBuf>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Bench framework
    Bench {
        #[command(subcommand)]
        sub: BenchCommands,
    },
    /// Benchmark unconstrained execution (Brillig)
    Exec {
        /// Path to program artifact (program.json)
        #[arg(long)]
        artifact: std::path::PathBuf,
        /// Path to Prover inputs (Prover.toml)
        #[arg(long, value_name = "Prover.toml")]
        prover_toml: std::path::PathBuf,
        /// Output directory for artifacts (e.g., flamegraph)
        #[arg(long)]
        output: Option<std::path::PathBuf>,
        /// Write machine-readable JSON report to this file
        #[arg(long)]
        json: Option<std::path::PathBuf>,
        /// Generate flamegraph SVG for Brillig execution
        #[arg(long)]
        flamegraph: bool,
        /// Number of measured iterations to run
        #[arg(long, default_value_t = 1)]
        iterations: usize,
        /// Number of warmup iterations to run before measuring
        #[arg(long, default_value_t = 0)]
        warmup: usize,
    },

    /// Report gates via backend provider
    Gates {
        /// Path to program artifact (program.json)
        #[arg(long)]
        artifact: std::path::PathBuf,
        /// Backend name (e.g., barretenberg)
        #[arg(long)]
        backend: Option<String>,
        /// Path to backend binary (e.g., bb)
        #[arg(long)]
        backend_path: Option<std::path::PathBuf>,
        /// Additional args passed to backend after its gates command
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        backend_args: Vec<String>,
        /// Generic backend command template (use placeholders like {artifact})
        #[arg(long)]
        template: Option<String>,
        /// Write machine-readable JSON report to this file
        #[arg(long)]
        json: Option<std::path::PathBuf>,
    },

    /// Benchmark proving via backend provider
    Prove {
        /// Path to program artifact (program.json)
        #[arg(long)]
        artifact: std::path::PathBuf,
        /// Path to Prover inputs (Prover.toml)
        #[arg(long, value_name = "Prover.toml")]
        prover_toml: Option<std::path::PathBuf>,
        /// Backend name (e.g., barretenberg, mock)
        #[arg(long)]
        backend: Option<String>,
        /// Path to backend binary
        #[arg(long)]
        backend_path: Option<std::path::PathBuf>,
        /// Additional args passed to backend
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        backend_args: Vec<String>,
        /// Generic backend command template (placeholders: {artifact},{witness},{proof},{outdir})
        #[arg(long)]
        template: Option<String>,
        /// Timeout seconds
        #[arg(long, default_value_t = 0)]
        timeout: u64,
        /// Number of measured iterations to run
        #[arg(long, default_value_t = 1)]
        iterations: usize,
        /// Number of warmup iterations to run before measuring
        #[arg(long, default_value_t = 0)]
        warmup: usize,
        /// Write machine-readable JSON report to this file
        #[arg(long)]
        json: Option<std::path::PathBuf>,
    },

    /// Verify a proof via backend provider
    Verify {
        /// Path to program artifact (program.json)
        #[arg(long)]
        artifact: std::path::PathBuf,
        /// Path to proof file
        #[arg(long)]
        proof: std::path::PathBuf,
        /// Backend name (e.g., barretenberg)
        #[arg(long)]
        backend: Option<String>,
        /// Path to backend binary
        #[arg(long)]
        backend_path: Option<std::path::PathBuf>,
        /// Additional args passed to backend
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        backend_args: Vec<String>,
        /// Generic backend command template (placeholders: {artifact},{proof})
        #[arg(long)]
        template: Option<String>,
        /// Number of measured iterations to run
        #[arg(long, default_value_t = 1)]
        iterations: usize,
        /// Number of warmup iterations to run before measuring
        #[arg(long, default_value_t = 0)]
        warmup: usize,
        /// Write machine-readable JSON report to this file
        #[arg(long)]
        json: Option<std::path::PathBuf>,
    },

    /// Compare benchmark results and detect regressions
    ///
    /// Supports comparing single JSON reports or JSONL files containing multiple records.
    /// For CI usage, use --baseline-file and --target-file with JSONL files.
    Compare {
        /// Baseline JSON report (single file comparison)
        #[arg(long, conflicts_with = "baseline_file")]
        baseline: Option<std::path::PathBuf>,
        /// Target/contender JSON report (single file comparison)
        #[arg(long, conflicts_with = "target_file")]
        contender: Option<std::path::PathBuf>,
        /// Baseline JSONL file (multi-record comparison for CI)
        #[arg(long, conflicts_with = "baseline")]
        baseline_file: Option<std::path::PathBuf>,
        /// Target JSONL file (multi-record comparison for CI)
        #[arg(long, conflicts_with = "contender")]
        target_file: Option<std::path::PathBuf>,
        /// Regression threshold percentage (default: 10.0)
        #[arg(long, default_value = "10.0")]
        threshold: f64,
        /// Output format: text, json, markdown
        #[arg(long, default_value = "text")]
        format: String,
        /// Write RegressionReport JSON to this file
        #[arg(long)]
        json_out: Option<std::path::PathBuf>,
        /// Write standalone HTML report to this file
        #[arg(long)]
        html_out: Option<std::path::PathBuf>,
    },

    /// Run a suite from YAML config
    Suite {
        /// Path to suite YAML config
        #[arg(long)]
        config: std::path::PathBuf,
        /// Write JSONL stream of results
        #[arg(long)]
        jsonl: Option<std::path::PathBuf>,
        /// Write a summary JSON file
        #[arg(long)]
        summary: Option<std::path::PathBuf>,
    },

    /// Run a Foundry/Anvil EVM verifier and capture gas usage
    EvmVerify {
        /// Path to Foundry project directory containing verifier + tests
        #[arg(long, value_name = "foundry_dir")]
        foundry_dir: std::path::PathBuf,
        /// Optional Noir program artifact (program.json) to tag meta
        #[arg(long)]
        artifact: Option<std::path::PathBuf>,
        /// Test name/pattern to match (e.g., testVerify)
        #[arg(long, value_name = "pattern")]
        r#match: Option<String>,
        /// Override calldata size in bytes (if test does not log CALDATA_BYTES)
        #[arg(long)]
        calldata_bytes: Option<u64>,
        /// Gas per second to estimate latency (default 1_250_000)
        #[arg(long)]
        gas_per_second: Option<u64>,
        /// Path to forge binary (defaults to `forge` in PATH)
        #[arg(long)]
        forge_bin: Option<std::path::PathBuf>,
        /// Write machine-readable JSON report to this file
        #[arg(long)]
        json: Option<std::path::PathBuf>,
    },

    /// Export JSONL benchmark records to CSV format
    ExportCsv {
        /// Path to input JSONL file containing benchmark records
        #[arg(long)]
        input: std::path::PathBuf,
        /// Path to output CSV file (writes to stdout if not specified)
        #[arg(long)]
        output: Option<std::path::PathBuf>,
    },

    /// Run benchmarks for CI/CD pipelines
    ///
    /// Runs a subset of benchmarks, compares against a baseline, and outputs
    /// results suitable for CI environments. Returns exit code 1 if regressions
    /// are detected above the threshold.
    Ci {
        /// Path to bench-config.toml
        #[arg(long)]
        config: Option<std::path::PathBuf>,
        /// Circuits to benchmark (comma-separated, overrides config)
        #[arg(long, value_delimiter = ',')]
        circuits: Option<Vec<String>>,
        /// Baseline JSONL file for comparison
        #[arg(long)]
        baseline_file: Option<std::path::PathBuf>,
        /// Regression threshold percentage (default: 10.0)
        #[arg(long, default_value = "10.0")]
        threshold: f64,
        /// Number of measured iterations (default: 3)
        #[arg(long)]
        iterations: Option<usize>,
        /// Number of warmup iterations (default: 1)
        #[arg(long)]
        warmup: Option<usize>,
        /// Output file for benchmark results JSONL
        #[arg(long)]
        output: Option<std::path::PathBuf>,
        /// Output format: text, json, markdown (default: markdown)
        #[arg(long, default_value = "markdown")]
        format: String,
        /// Write RegressionReport JSON to this file
        #[arg(long)]
        json_out: Option<std::path::PathBuf>,
        /// Write standalone HTML report to this file
        #[arg(long)]
        html_out: Option<std::path::PathBuf>,
    },

    /// Build derived history artifacts from JSONL
    ///
    /// Reads canonical JSONL telemetry and produces:
    /// - index.json: derived run index for querying
    /// - index.html: single-file HTML viewer (fetches index.json at runtime)
    History {
        #[command(subcommand)]
        sub: HistoryCommands,
    },
}

#[derive(Subcommand, Debug)]
enum HistoryCommands {
    /// Build index.json and index.html from JSONL
    Build {
        /// Path to input JSONL file containing BenchRecords
        #[arg(long)]
        jsonl: std::path::PathBuf,
        /// Output directory for index.json and index.html
        #[arg(long)]
        out: std::path::PathBuf,
    },
}

#[derive(Subcommand, Debug)]
enum BenchCommands {
    /// List circuits from bench-config.toml
    List {
        /// Path to bench-config.toml (default: bench-config.toml)
        #[arg(long)]
        config: Option<std::path::PathBuf>,
    },
    /// Run compile->prove->verify for a circuit
    Run {
        /// Circuit name from config
        #[arg(long)]
        circuit: String,
        /// Backend: bb|evm (default: bb)
        #[arg(long)]
        backend: Option<String>,
        /// Params value to select (optional)
        #[arg(long)]
        params: Option<u64>,
        /// Number of measured iterations to run
        #[arg(long, default_value_t = 1)]
        iterations: usize,
        /// Number of warmup iterations to run before measuring
        #[arg(long, default_value_t = 0)]
        warmup: usize,
        /// Path to bench-config.toml
        #[arg(long)]
        config: Option<std::path::PathBuf>,
        /// CSV output (default: out/bench.csv)
        #[arg(long)]
        csv: Option<std::path::PathBuf>,
        /// JSONL output (default: out/bench.jsonl)
        #[arg(long)]
        jsonl: Option<std::path::PathBuf>,
    },
    /// Run across all circuits and params in config
    RunAll {
        /// Backend: bb|evm (default: bb)
        #[arg(long)]
        backend: Option<String>,
        /// Number of measured iterations to run
        #[arg(long, default_value_t = 1)]
        iterations: usize,
        /// Number of warmup iterations to run before measuring
        #[arg(long, default_value_t = 0)]
        warmup: usize,
        /// Path to bench-config.toml
        #[arg(long)]
        config: Option<std::path::PathBuf>,
        /// CSV output (default: out/bench.csv)
        #[arg(long)]
        csv: Option<std::path::PathBuf>,
        /// JSONL output (default: out/bench.jsonl)
        #[arg(long)]
        jsonl: Option<std::path::PathBuf>,
    },
    /// Export CSV from JSONL records
    ExportCsv {
        /// JSONL input (default: out/bench.jsonl)
        #[arg(long)]
        jsonl: Option<std::path::PathBuf>,
        /// CSV output (default: out/bench.csv)
        #[arg(long)]
        csv: Option<std::path::PathBuf>,
    },
    /// Run EVM verification against a circuit's foundry project
    EvmVerify {
        /// Circuit name from config
        #[arg(long)]
        circuit: String,
        /// Path to bench-config.toml
        #[arg(long)]
        config: Option<std::path::PathBuf>,
        /// CSV output (default: out/bench.csv)
        #[arg(long)]
        csv: Option<std::path::PathBuf>,
    },
}

fn init_tracing(verbose: bool) {
    let env = std::env::var("NOIR_BENCH_LOG").unwrap_or_else(|_| {
        if verbose {
            "noir_bench=debug".to_string()
        } else {
            "noir_bench=info".to_string()
        }
    });
    let _ = tracing_subscriber::fmt()
        .with_span_events(FmtSpan::ACTIVE)
        .with_writer(std::io::stderr)
        .with_ansi(true)
        .with_env_filter(EnvFilter::new(env))
        .try_init();
}

fn main() {
    color_eyre::install().ok();
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    fn write_exports(
        json_path: &std::path::Path,
        csv: &Option<std::path::PathBuf>,
        md: &Option<std::path::PathBuf>,
    ) {
        let Ok(bytes) = std::fs::read(json_path) else {
            return;
        };
        let Ok(v): Result<JsonValue, _> = serde_json::from_slice(&bytes) else {
            return;
        };
        if let Some(csv_path) = csv {
            let mut line = String::new();
            if v.get("execution_time_ms").is_some() {
                line = format!(
                    "kind,time_ms,samples\nexec,{},{}\n",
                    v["execution_time_ms"], v["samples_count"]
                );
            } else if v.get("total_gates").is_some() {
                line = format!(
                    "kind,total_gates,acir_opcodes\ngates,{},{}\n",
                    v["total_gates"], v["acir_opcodes"]
                );
            } else if v.get("prove_time_ms").is_some() {
                line = format!(
                    "kind,prove_time_ms,witness_gen_ms,backend_ms,proof_size,peak_mem\nprove,{},{},{},{},{}\n",
                    v["prove_time_ms"],
                    v.get("witness_gen_time_ms").unwrap_or(&JsonValue::Null),
                    v.get("backend_prove_time_ms").unwrap_or(&JsonValue::Null),
                    v.get("proof_size_bytes").unwrap_or(&JsonValue::Null),
                    v.get("peak_memory_bytes").unwrap_or(&JsonValue::Null)
                );
            } else if v.get("verify_time_ms").is_some() {
                line = format!(
                    "kind,verify_time_ms,ok\nverify,{},{}\n",
                    v["verify_time_ms"], v["ok"]
                );
            } else if v.get("gas_used").is_some() {
                line = format!(
                    "kind,gas_used,calldata_bytes,est_latency_ms\nevm-verify,{},{},{}\n",
                    v["gas_used"],
                    v.get("calldata_bytes").unwrap_or(&JsonValue::Null),
                    v.get("est_latency_ms").unwrap_or(&JsonValue::Null)
                );
            }
            if !line.is_empty() {
                let _ = std::fs::write(csv_path, line.as_bytes());
            }
        }
        if let Some(md_path) = md {
            let mut md_s = String::new();
            if v.get("execution_time_ms").is_some() {
                md_s.push_str("| kind | time_ms | samples |\n|---|---:|---:|\n");
                md_s.push_str(&format!(
                    "| exec | {} | {} |\n",
                    v["execution_time_ms"], v["samples_count"]
                ));
            } else if v.get("total_gates").is_some() {
                md_s.push_str("| kind | total_gates | acir_opcodes |\n|---|---:|---:|\n");
                md_s.push_str(&format!(
                    "| gates | {} | {} |\n",
                    v["total_gates"], v["acir_opcodes"]
                ));
            } else if v.get("prove_time_ms").is_some() {
                md_s.push_str("| kind | prove_ms | witness_ms | backend_ms | proof_size | peak_mem |\n|---|---:|---:|---:|---:|---:|\n");
                md_s.push_str(&format!(
                    "| prove | {} | {} | {} | {} | {} |\n",
                    v["prove_time_ms"],
                    v.get("witness_gen_time_ms").unwrap_or(&JsonValue::Null),
                    v.get("backend_prove_time_ms").unwrap_or(&JsonValue::Null),
                    v.get("proof_size_bytes").unwrap_or(&JsonValue::Null),
                    v.get("peak_memory_bytes").unwrap_or(&JsonValue::Null)
                ));
            } else if v.get("verify_time_ms").is_some() {
                md_s.push_str("| kind | verify_ms | ok |\n|---|---:|:--:|\n");
                md_s.push_str(&format!(
                    "| verify | {} | {} |\n",
                    v["verify_time_ms"], v["ok"]
                ));
            } else if v.get("gas_used").is_some() {
                md_s.push_str(
                    "| kind | gas_used | calldata_bytes | est_latency_ms |\n|---|---:|---:|---:|\n",
                );
                md_s.push_str(&format!(
                    "| evm-verify | {} | {} | {} |\n",
                    v["gas_used"],
                    v.get("calldata_bytes").unwrap_or(&JsonValue::Null),
                    v.get("est_latency_ms").unwrap_or(&JsonValue::Null)
                ));
            }
            if !md_s.is_empty() {
                let _ = std::fs::write(md_path, md_s.as_bytes());
            }
        }
    }

    let result = match cli.command {
        Commands::Bench { sub } => match sub {
            BenchCommands::List { config } => bench::bench_cmd::list(config),
            BenchCommands::Run {
                circuit,
                backend,
                params,
                iterations,
                warmup,
                config,
                csv,
                jsonl,
            } => bench::bench_cmd::run(
                circuit,
                backend,
                params,
                config,
                csv,
                jsonl,
                Some(iterations),
                Some(warmup),
            ),
            BenchCommands::RunAll {
                backend,
                iterations,
                warmup,
                config,
                csv,
                jsonl,
            } => bench::bench_cmd::run_all(
                backend,
                config,
                csv,
                jsonl,
                Some(iterations),
                Some(warmup),
            ),
            BenchCommands::ExportCsv { jsonl, csv } => bench::bench_cmd::export_csv(jsonl, csv),
            BenchCommands::EvmVerify {
                circuit,
                config,
                csv,
            } => bench::bench_cmd::evm_verify(circuit, config, csv),
        },
        Commands::Exec {
            artifact,
            prover_toml,
            output,
            json,
            flamegraph,
            iterations,
            warmup,
        } => {
            let r = exec_cmd::run(
                artifact.clone(),
                prover_toml.clone(),
                output.clone(),
                json.clone(),
                flamegraph,
                Some(iterations),
                Some(warmup),
            );
            if let (Ok(_), Some(j)) = (&r, &json) {
                write_exports(j, &cli.csv, &cli.md);
            }
            r
        }
        Commands::Gates {
            artifact,
            backend,
            backend_path,
            backend_args,
            template,
            json,
        } => {
            let r = gates_cmd::run(
                artifact.clone(),
                backend,
                backend_path,
                backend_args,
                template,
                json.clone(),
            );
            if let (Ok(_), Some(j)) = (&r, &json) {
                write_exports(j, &cli.csv, &cli.md);
            }
            r
        }
        Commands::Prove {
            artifact,
            prover_toml,
            backend,
            backend_path,
            backend_args,
            template,
            timeout,
            iterations,
            warmup,
            json,
        } => {
            let r = prove_cmd::run(
                artifact,
                prover_toml,
                backend,
                backend_path,
                backend_args,
                template,
                timeout,
                Some(iterations),
                Some(warmup),
                json.clone(),
            );
            if let (Ok(_), Some(j)) = (&r, &json) {
                write_exports(j, &cli.csv, &cli.md);
            }
            r
        }
        Commands::Verify {
            artifact,
            proof,
            backend,
            backend_path,
            backend_args,
            template,
            iterations,
            warmup,
            json,
        } => {
            let r = verify_cmd::run(
                artifact,
                proof,
                backend,
                backend_path,
                backend_args,
                template,
                Some(iterations),
                Some(warmup),
                json.clone(),
            );
            if let (Ok(_), Some(j)) = (&r, &json) {
                write_exports(j, &cli.csv, &cli.md);
            }
            r
        }
        Commands::Compare {
            baseline,
            contender,
            baseline_file,
            target_file,
            threshold,
            format,
            json_out,
            html_out,
        } => {
            match compare_cmd::run(
                baseline,
                contender,
                baseline_file,
                target_file,
                threshold,
                format,
                json_out,
                html_out,
            ) {
                Ok(result) => {
                    if result.ci_exit_code != 0 {
                        std::process::exit(result.ci_exit_code);
                    }
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }
        Commands::Suite {
            config,
            jsonl,
            summary,
        } => suite_cmd::run(config, jsonl, summary),
        Commands::EvmVerify {
            foundry_dir,
            artifact,
            r#match,
            calldata_bytes,
            gas_per_second,
            forge_bin,
            json,
        } => {
            let r = evm_verify_cmd::run(
                foundry_dir,
                artifact,
                r#match,
                calldata_bytes,
                gas_per_second,
                forge_bin,
                json.clone(),
            );
            if let (Ok(_), Some(j)) = (&r, &json) {
                write_exports(j, &cli.csv, &cli.md);
            }
            r
        }
        Commands::ExportCsv { input, output } => {
            let reader = JsonlWriter::new(&input);
            let records = reader.read_all();
            match records {
                Ok(records) => {
                    let exporter = CsvExporter::new();
                    match output {
                        Some(path) => {
                            let r = exporter.export(&records, &path);
                            if r.is_ok() {
                                eprintln!(
                                    "Exported {} records to {}",
                                    records.len(),
                                    path.display()
                                );
                            }
                            r
                        }
                        None => exporter.export_to_stdout(&records),
                    }
                }
                Err(e) => Err(e),
            }
        }
        Commands::Ci {
            config,
            circuits,
            baseline_file,
            threshold,
            iterations,
            warmup,
            output,
            format,
            json_out,
            html_out,
        } => {
            match ci_cmd::run(
                config,
                circuits,
                baseline_file,
                Some(threshold),
                iterations,
                warmup,
                output,
                format,
                json_out,
                html_out,
            ) {
                Ok(exit_code) => {
                    if exit_code != 0 {
                        std::process::exit(exit_code);
                    }
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }
        Commands::History { sub } => match sub {
            HistoryCommands::Build { jsonl, out } => history_cmd::build(jsonl, out),
        },
    };

    if let Err(e) = result {
        eprintln!("{:#}", e);
        std::process::exit(1);
    }
}
