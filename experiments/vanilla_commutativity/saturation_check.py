#!/usr/bin/env python3
"""Saturation check for associativity + commutativity on n distinct atoms.

Runs the chain shape to saturation for small n and compares the observed counts
against `e-classes = 2^n - 1` and `Add e-nodes = 3^n - 2^(n+1) + 1`. At
saturation every non-empty subset of the n leaves is an e-class, so for n = 300
the e-class count alone is 2^300 - 1.
"""
import argparse
import json
import subprocess
import tempfile
from pathlib import Path

import bench

ROOT = Path(__file__).resolve().parents[2]


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--out", type=Path, default=ROOT / "out/vanilla-commutativity/saturation.json")
    p.add_argument("--max-n", type=int, default=8)
    a = p.parse_args()
    rows = []
    for n in range(2, a.max_n + 1):
        with tempfile.TemporaryDirectory() as td:
            egg = Path(td) / "p.egg"
            graph = Path(td) / "g.json"
            egg.write_text(bench.program(n, "chain", "ac", 200))
            summary = json.loads(
                subprocess.run(
                    [str(bench.HARNESS), str(egg), str(graph)], capture_output=True, text=True, check=True
                ).stdout.strip().splitlines()[-1]
            )
            classes = {node["result"] for node in json.loads(graph.read_text())}
        rows.append(
            {
                "n": n,
                "observed_eclasses": len(classes),
                "predicted_eclasses": 2**n - 1,
                "observed_add_enodes": summary["enodes"] - n,
                "predicted_add_enodes": 3**n - 2 ** (n + 1) + 1,
                "matches": summary["matches"],
                "seconds": summary["seconds"],
            }
        )
        ok = (
            rows[-1]["observed_eclasses"] == rows[-1]["predicted_eclasses"]
            and rows[-1]["observed_add_enodes"] == rows[-1]["predicted_add_enodes"]
        )
        print(
            f"n={n}: eclasses={len(classes)} (2^n-1={2**n - 1}) "
            f"Add={summary['enodes'] - n} (3^n-2^(n+1)+1={3**n - 2 ** (n + 1) + 1}) "
            f"{'OK' if ok else 'MISMATCH'} [{summary['seconds']:.3f}s]"
        )
    a.out.parent.mkdir(parents=True, exist_ok=True)
    a.out.write_text(json.dumps({"shape": "chain", "rules": "ac", "rows": rows}, indent=2) + "\n")
    print(f"wrote {a.out}")


if __name__ == "__main__":
    main()
