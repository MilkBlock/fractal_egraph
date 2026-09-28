#!/usr/bin/env python3
"""Where does vanilla egglog spend its time on AC saturation?

Runs each encoding as a sequence of `(run 1)` commands through the profiling
probe, which prints the engine's own per-iteration phase timings (ReportLevel
`TimeOnly`): search+apply, merge and rebuild, plus per-rule time and matches.

`search_and_apply` runs the query plans and instantiates the actions.
`merge` flushes the produced tuples into the tables (dedup/insert) and propagates
them to dependent tables. `rebuild` canonicalizes ids after new unions.
"""
import argparse
import collections
import json
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "experiments/shortcut_rules"))
import shortcut_bench as sb  # noqa: E402

PROFILE = ROOT / "out/shortcut-rules/bin/profile"


def phases(n, variant, iterations, timeout):
    body = [sb.bench.HEADER, *sb.VARIANTS[variant], sb.bench.chain(n)]
    body += ["(run 1)"] * iterations
    with tempfile.TemporaryDirectory() as td:
        egg = Path(td) / "p.egg"
        egg.write_text("\n".join(body) + "\n")
        proc = subprocess.run([str(PROFILE), str(egg)], capture_output=True, text=True, timeout=timeout)
        if proc.returncode != 0:
            raise SystemExit(proc.stderr[-500:])
        iters = json.loads(proc.stdout)
    total = {k: sum(it[k] for it in iters) for k in ("search_apply_s", "merge_s", "rebuild_s")}
    grand = sum(total.values())
    per_rule = collections.defaultdict(lambda: {"search_apply_s": 0.0, "matches": 0})
    for it in iters:
        for rule in it["rules"]:
            per_rule[rule["rule"]]["search_apply_s"] += rule["search_apply_s"]
            per_rule[rule["rule"]]["matches"] += rule["matches"]
    return {
        "n": n,
        "variant": variant,
        "iterations": iters,
        "totals": total,
        "total_s": grand,
        "shares": {k: (v / grand if grand else 0.0) for k, v in total.items()},
        "per_rule": dict(per_rule),
    }


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--n", type=int, nargs="+", default=[8, 10, 12])
    p.add_argument("--iterations", type=int, default=14)
    p.add_argument("--timeout", type=float, default=300.0)
    p.add_argument("--out", type=Path, default=ROOT / "out/shortcut-rules/phases.json")
    a = p.parse_args()
    if not PROFILE.exists():
        raise SystemExit(f"missing {PROFILE}; run experiments/shortcut_rules/build_profile.py first")

    report = {"engine_phases": "search+apply / merge / rebuild from the egglog RunReport", "runs": []}
    for n in a.n:
        for variant in ("generic", "shortcut"):
            rec = phases(n, variant, a.iterations, a.timeout)
            report["runs"].append(rec)
            t = rec["totals"]
            print(
                f"n={n:>2} {variant:<9} total={rec['total_s']:.3f}s  "
                f"search+apply {t['search_apply_s']:.3f}s ({100 * rec['shares']['search_apply_s']:.0f}%)  "
                f"merge {t['merge_s']:.3f}s ({100 * rec['shares']['merge_s']:.0f}%)  "
                f"rebuild {t['rebuild_s']:.3f}s ({100 * rec['shares']['rebuild_s']:.0f}%)",
                flush=True,
            )
            for name, stats in sorted(rec["per_rule"].items(), key=lambda kv: -kv[1]["matches"]):
                label = " ".join(name.split())[:70]
                print(f"      {label:<70} matches={stats['matches']:>12,}", flush=True)
    a.out.parent.mkdir(parents=True, exist_ok=True)
    a.out.write_text(json.dumps(report, indent=2) + "\n")
    print(f"wrote {a.out}")


if __name__ == "__main__":
    main()
