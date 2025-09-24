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

### Generic providers

You can run gates/prove/verify against any binary by providing a command template via `--template`. Placeholders:

- `{artifact}`: path to `program.json`
- `{witness}`: path to generated witness (prove only)
- `{proof}`: output proof path (prove) / input proof path (verify)
- `{outdir}`: output directory if applicable

Examples:

```sh
# Gates via generic template
noir-bench gates --artifact program.json \
  --template "bb gates -b {artifact}" \
  --json out/gates.json

# Prove via generic template
noir-bench prove --artifact program.json --prover-toml Prover.toml \
  --template "bb prove -b {artifact} -w {witness} -o {proof}" \
  --json out/prove.json

# Verify via generic template
noir-bench verify --artifact program.json --proof proof.bin \
  --template "bb verify -b {artifact} -p {proof}" \
  --json out/verify.json
```

## Verify

Verify a proof using Barretenberg or generic provider. Output JSON shape:

```json
{ "verify_time_ms": 123, "ok": true }
```

CLI (Barretenberg; pass public inputs and vk via extra args as needed):

```sh
noir-bench verify --artifact program.json --proof out/proof \
  --backend barretenberg --backend-path bb \
  --json out/verify.json -- -i out/public_inputs -k out/vk/vk -s ultra_honk
```

## Iterations and warmup

For `exec`, you can run multiple iterations with warmup:

```sh
noir-bench exec --artifact program.json --prover-toml Prover.toml \
  --iterations 5 --warmup 2
```

JSON includes per-iteration stats under `iterations`:

```json
{
  "iterations": { "iterations": 5, "warmup": 2, "times_ms": [..], "avg_ms": 1.23, "min_ms": 1, "max_ms": 2, "stddev_ms": 0.1 }
}
```

## System and backend info

All JSON reports now include `system` (CPU model, cores, RAM, OS) and backend `name/version`. CLI args are captured in `meta.cli_args`.

## Logging

Set `NOIR_BENCH_LOG` or pass `--verbose`. Example:

```sh
NOIR_BENCH_LOG=noir_bench=debug ./target/release/noir-bench exec ...
```

## License

Licensed under either of

- Apache License, Version 2.0 ("Apache-2.0"); or
- MIT license ("MIT");

at your option.

SPDX-License-Identifier: MIT OR Apache-2.0