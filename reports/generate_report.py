import argparse
import os

import pandas as pd
import matplotlib.pyplot as plt


def ensure_dir(p):
    d = os.path.dirname(p)
    if d and not os.path.exists(d):
        os.makedirs(d, exist_ok=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--csv", default="out/bench.csv")
    parser.add_argument("--outdir", default="reports/output")
    args = parser.parse_args()

    df = pd.read_csv(args.csv)
    ensure_dir(args.outdir + "/dummy")

    # Prove time vs constraints
    if {"prove_ms", "constraints"}.issubset(df.columns):
        fig, ax = plt.subplots()
        sub = df.dropna(subset=["prove_ms", "constraints"])
        ax.scatter(sub["constraints"], sub["prove_ms"])
        ax.set_xlabel("constraints")
        ax.set_ylabel("prove_ms")
        ax.set_title("Prove time vs constraints")
        fig.tight_layout()
        fig.savefig(os.path.join(args.outdir, "prove_vs_constraints.png"))
        plt.close(fig)

    # Memory vs params
    if {"memory_mb", "params"}.issubset(df.columns):
        fig, ax = plt.subplots()
        sub = df[df["params"].notna()]
        ax.scatter(sub["params"], sub["memory_mb"])
        ax.set_xlabel("params")
        ax.set_ylabel("memory_mb")
        ax.set_title("Memory vs params")
        fig.tight_layout()
        fig.savefig(os.path.join(args.outdir, "memory_vs_params.png"))
        plt.close(fig)

    # EVM gas vs params
    if {"evm_gas", "params"}.issubset(df.columns):
        fig, ax = plt.subplots()
        sub = df.dropna(subset=["evm_gas", "params"])
        ax.scatter(sub["params"], sub["evm_gas"])
        ax.set_xlabel("params")
        ax.set_ylabel("evm_gas")
        ax.set_title("EVM gas vs params")
        fig.tight_layout()
        fig.savefig(os.path.join(args.outdir, "evm_gas_vs_params.png"))
        plt.close(fig)

    print(f"Reports written to {args.outdir}")


if __name__ == "__main__":
    main()


