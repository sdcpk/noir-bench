# noir-bench

Developer-friendly benchmarking suite for Noir end-to-end performance, focused on client-side steps: unconstrained execution (Brillig) and proving. Outputs human-readable summaries and machine-readable JSON; can optionally generate flamegraphs.

## Build

```sh
cargo build -p noir_bench --release
```

## Exec (unconstrained/Brillig)

```sh
./target/release/noir-bench exec \
  --artifact path/to/program.json \
  --prover-toml path/to/Prover.toml \
  --output out \
  --json out/exec.json \
  --flamegraph
```

## Gates (backend-driven)

```sh
./target/release/noir-bench gates \
  --artifact path/to/program.json \
  --backend barretenberg \
  --backend-path bb \
  --json out/gates.json -- --include_gates_per_opcode
```

## Prove (backend-driven)

Currently supports Barretenberg by shelling out to `bb`.

```sh
./target/release/noir-bench prove \
  --artifact path/to/program.json \
  --prover-toml path/to/Prover.toml \
  --backend barretenberg --backend-path bb \
  --timeout 600 \
  --json out/prove.json
```

- Metrics: `prove_time_ms`, `proof_size_bytes`, optional `peak_memory_bytes` with `--features mem`.
- We generate `witness.gz` in a temp dir and pass it to `bb prove`.
- Other backends can be added by implementing `ProverProvider`/`GatesProvider` and selecting via `--backend` and `--backend-path`.

## Backends

- Use `--backend` to select (e.g., `barretenberg`, `mock`, etc.).
- Use `--backend-path` to point to the backend binary.
- Any extra backend flags can be appended after `--` and will be forwarded.

## Logging

Set `NOIR_BENCH_LOG` or pass `--verbose`. Example:

```sh
NOIR_BENCH_LOG=noir_bench=debug ./target/release/noir-bench exec ...
```