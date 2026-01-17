#!/usr/bin/env bash
set -euo pipefail

# compile examples
for d in examples/simple_hash examples/range_bits examples/merkle_verify; do
  (cd "$d" && nargo compile)
done

# run base suite
noir-bench suite --config examples/suite_base.yml --jsonl out/suite_base.jsonl --summary out/suite_base.json

# run scheme variant suite
noir-bench suite --config examples/suite_ultrahonk.yml --jsonl out/suite_scheme.jsonl --summary out/suite_scheme.json

# demo compare: choose first line from JSONL streams if present
base_json=$(head -n1 out/suite_base.jsonl || true)
scheme_json=$(head -n1 out/suite_scheme.jsonl || true)
if [[ -n "$base_json" && -n "$scheme_json" ]]; then
  echo "$base_json" > /tmp/base.json
  echo "$scheme_json" > /tmp/scheme.json
  noir-bench compare --baseline /tmp/base.json --contender /tmp/scheme.json --fail_on_regress 10 || true
fi










