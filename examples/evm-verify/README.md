EVM Verifier Example (Foundry)

This is a minimal Foundry scaffold to measure gas for a verifier's `verify(bytes,bytes)` call.

- `contracts/Verifier.sol` is a placeholder. Replace it with the Solidity verifier generated for your circuit/scheme.
- `test/Verify.t.sol` deploys `Verifier` and calls `verify(proof, publicInputs)`.
- Calldata size is measured via `abi.encodeWithSignature` and logged as `CALDATA_BYTES: <n>`.

Usage:

1) Install forge-std (once):

```bash
forge install foundry-rs/forge-std
```

2) Provide inputs as env vars (hex, no `0x`):

```bash
export PROOF_HEX="..."
export PUB_INPUTS_HEX="..."
forge test --gas-report -m testVerify -vvv
```

Integration with noir-bench `evm-verify`:

```bash
noir-bench evm-verify \
  --foundry-dir /path/to/noir-bench/examples/evm-verify \
  --artifact /path/to/program.json \
  --match testVerify \
  --json out/evm_verify.json
```

- The subcommand parses gas from Foundry outputs and `CALDATA_BYTES` from the test logs.
- If your verifier uses a different function signature, adapt the test accordingly.



