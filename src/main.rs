 #![forbid(unsafe_code)]

use clap::{Parser, Subcommand};
use tracing_subscriber::{EnvFilter, fmt::format::FmtSpan};

use noir_bench::{exec_cmd, gates_cmd, prove_cmd, verify_cmd, compare_cmd, suite_cmd};
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

    /// Compare two JSON reports and print deltas
    Compare {
        /// Baseline JSON report
        #[arg(long)]
        baseline: std::path::PathBuf,
        /// Contender JSON report
        #[arg(long)]
        contender: std::path::PathBuf,
        /// Fail if percent regression exceeds threshold
        #[arg(long)]
        fail_on_regress: Option<f64>,
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
}

fn init_tracing(verbose: bool) {
    let env = std::env::var("NOIR_BENCH_LOG").unwrap_or_else(|_| {
        if verbose { "noir_bench=debug".to_string() } else { "noir_bench=info".to_string() }
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

    fn write_exports(json_path: &std::path::Path, csv: &Option<std::path::PathBuf>, md: &Option<std::path::PathBuf>) {
        let Ok(bytes) = std::fs::read(json_path) else { return; };
        let Ok(v): Result<JsonValue, _> = serde_json::from_slice(&bytes) else { return; };
        if let Some(csv_path) = csv {
            let mut line = String::new();
            if v.get("execution_time_ms").is_some() {
                line = format!("kind,time_ms,samples\nexec,{},{}\n", v["execution_time_ms"], v["samples_count"]);
            } else if v.get("total_gates").is_some() {
                line = format!("kind,total_gates,acir_opcodes\ngates,{},{}\n", v["total_gates"], v["acir_opcodes"]);
            } else if v.get("prove_time_ms").is_some() {
                line = format!(
                    "kind,prove_time_ms,witness_gen_ms,backend_ms,proof_size,peak_mem\nprove,{},{},{},{},{}\n",
                    v["prove_time_ms"], v.get("witness_gen_time_ms").unwrap_or(&JsonValue::Null), v.get("backend_prove_time_ms").unwrap_or(&JsonValue::Null), v.get("proof_size_bytes").unwrap_or(&JsonValue::Null), v.get("peak_memory_bytes").unwrap_or(&JsonValue::Null)
                );
            } else if v.get("verify_time_ms").is_some() {
                line = format!("kind,verify_time_ms,ok\nverify,{},{}\n", v["verify_time_ms"], v["ok"]);
            }
            if !line.is_empty() { let _ = std::fs::write(csv_path, line.as_bytes()); }
        }
        if let Some(md_path) = md {
            let mut md_s = String::new();
            if v.get("execution_time_ms").is_some() {
                md_s.push_str("| kind | time_ms | samples |\n|---|---:|---:|\n");
                md_s.push_str(&format!("| exec | {} | {} |\n", v["execution_time_ms"], v["samples_count"]));
            } else if v.get("total_gates").is_some() {
                md_s.push_str("| kind | total_gates | acir_opcodes |\n|---|---:|---:|\n");
                md_s.push_str(&format!("| gates | {} | {} |\n", v["total_gates"], v["acir_opcodes"]));
            } else if v.get("prove_time_ms").is_some() {
                md_s.push_str("| kind | prove_ms | witness_ms | backend_ms | proof_size | peak_mem |\n|---|---:|---:|---:|---:|---:|\n");
                md_s.push_str(&format!(
                    "| prove | {} | {} | {} | {} | {} |\n",
                    v["prove_time_ms"], v.get("witness_gen_time_ms").unwrap_or(&JsonValue::Null), v.get("backend_prove_time_ms").unwrap_or(&JsonValue::Null), v.get("proof_size_bytes").unwrap_or(&JsonValue::Null), v.get("peak_memory_bytes").unwrap_or(&JsonValue::Null)
                ));
            } else if v.get("verify_time_ms").is_some() {
                md_s.push_str("| kind | verify_ms | ok |\n|---|---:|:--:|\n");
                md_s.push_str(&format!("| verify | {} | {} |\n", v["verify_time_ms"], v["ok"]));
            }
            if !md_s.is_empty() { let _ = std::fs::write(md_path, md_s.as_bytes()); }
        }
    }

    let result = match cli.command {
        Commands::Exec { artifact, prover_toml, output, json, flamegraph, iterations, warmup } => {
            let r = exec_cmd::run(artifact.clone(), prover_toml.clone(), output.clone(), json.clone(), flamegraph, Some(iterations), Some(warmup));
            if let (Ok(_), Some(j)) = (&r, &json) {
                write_exports(j, &cli.csv, &cli.md);
            }
            r
        }
        Commands::Gates { artifact, backend, backend_path, backend_args, template, json } => {
            let r = gates_cmd::run(artifact.clone(), backend, backend_path, backend_args, template, json.clone());
            if let (Ok(_), Some(j)) = (&r, &json) {
                write_exports(j, &cli.csv, &cli.md);
            }
            r
        }
        Commands::Prove { artifact, prover_toml, backend, backend_path, backend_args, template, timeout, iterations, warmup, json } => {
            let r = prove_cmd::run(artifact, prover_toml, backend, backend_path, backend_args, template, timeout, Some(iterations), Some(warmup), json.clone());
            if let (Ok(_), Some(j)) = (&r, &json) {
                write_exports(j, &cli.csv, &cli.md);
            }
            r
        }
        Commands::Verify { artifact, proof, backend, backend_path, backend_args, template, iterations, warmup, json } => {
            let r = verify_cmd::run(artifact, proof, backend, backend_path, backend_args, template, Some(iterations), Some(warmup), json.clone());
            if let (Ok(_), Some(j)) = (&r, &json) {
                write_exports(j, &cli.csv, &cli.md);
            }
            r
        }
        Commands::Compare { baseline, contender, fail_on_regress } => {
            compare_cmd::run(baseline, contender, fail_on_regress)
        }
        Commands::Suite { config, jsonl, summary } => {
            suite_cmd::run(config, jsonl, summary)
        }
    };

    if let Err(e) = result {
        eprintln!("{:#}", e);
        std::process::exit(1);
    }
} 