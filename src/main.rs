 #![forbid(unsafe_code)]

use clap::{Parser, Subcommand};
use tracing_subscriber::{EnvFilter, fmt::format::FmtSpan};

use noir_bench::{exec_cmd, gates_cmd, prove_cmd, verify_cmd};

#[derive(Parser, Debug)]
#[command(name = "noir-bench")] 
#[command(about = "Benchmark suite for Noir execution and proving", long_about = None)]
struct Cli {
    /// Enable verbose logging (or set NOIR_BENCH_LOG)
    #[arg(long)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
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

    let result = match cli.command {
        Commands::Exec { artifact, prover_toml, output, json, flamegraph, iterations, warmup } => {
            exec_cmd::run(artifact, prover_toml, output, json, flamegraph, Some(iterations), Some(warmup))
        }
        Commands::Gates { artifact, backend, backend_path, backend_args, template, json } => gates_cmd::run(
            artifact,
            backend,
            backend_path,
            backend_args,
            template,
            json,
        ),
        Commands::Prove { artifact, prover_toml, backend, backend_path, backend_args, template, timeout, iterations, warmup, json } => {
            prove_cmd::run(artifact, prover_toml, backend, backend_path, backend_args, template, timeout, Some(iterations), Some(warmup), json)
        }
        Commands::Verify { artifact, proof, backend, backend_path, backend_args, template, iterations, warmup, json } => {
            verify_cmd::run(artifact, proof, backend, backend_path, backend_args, template, Some(iterations), Some(warmup), json)
        }
    };

    if let Err(e) = result {
        eprintln!("{:#}", e);
        std::process::exit(1);
    }
} 