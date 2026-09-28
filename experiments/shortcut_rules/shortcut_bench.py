#!/usr/bin/env python3
"""Does pre-composing a rule ("shortcut") speed up vanilla egglog saturation?

Baseline (`generic`) is the textbook AC encoding: a commutation rewrite plus an
associativity rewrite, left to saturate over several iterations.

`shortcut` writes the whole three-element AC orbit into a single rule: one match
of `a+(b+c)` directly produces every equivalent three-element form, including the
commuted binary sub-sums, instead of waiting for later iterations to rediscover
them. It is intended to reach the *same* closure as `generic`.

`shortcut_naive` is the same head without the three `union (Add a b) (Add b a)`
actions. It is still sound but does not deduplicate the binary sub-sums, so it is
reported separately and never mixed into the timing comparison.

Everything runs on pristine egglog (`experiments/vanilla_commutativity/build.py`).
Equality of the final e-graph is checked structurally: every Add e-node is
labelled by the leaf sets of its two arguments and of its result, which is
independent of class numbering.
"""
import argparse
import collections
import hashlib
import json
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "experiments/vanilla_commutativity"))
import bench  # noqa: E402  (chain/HEADER generators and binary paths)

HARNESS = bench.HARNESS
PROBE = bench.PROBE

GENERIC = [
    "(rewrite (Add a b) (Add b a))",
    "(rewrite (Add a (Add b c)) (Add (Add a b) c))",
]

# The three-element AC orbit of root = a+(b+c), excluding root itself.
ORBIT = [
    "(Add (Add a b) c)",
    "(Add (Add b a) c)",
    "(Add (Add a c) b)",
    "(Add (Add c a) b)",
    "(Add (Add b c) a)",
    "(Add (Add c b) a)",
    "(Add a (Add c b))",
    "(Add b (Add a c))",
    "(Add b (Add c a))",
    "(Add c (Add a b))",
    "(Add c (Add b a))",
]
PAIRS = [("a", "b"), ("a", "c"), ("b", "c")]


def orbit_rule(pair_unions):
    actions = [f"(union (Add {x} {y}) (Add {y} {x}))" for x, y in PAIRS] if pair_unions else []
    actions += [f"(union root {term})" for term in ORBIT]
    return "(rule ((= root (Add a (Add b c))))\n      (" + "\n       ".join(actions) + "))"


SHORTCUT = [orbit_rule(pair_unions=True)]
SHORTCUT_NAIVE = [orbit_rule(pair_unions=False)]
VARIANTS = {"generic": GENERIC, "shortcut": SHORTCUT, "shortcut_naive": SHORTCUT_NAIVE}


def program(n, variant, steps=200, stepwise=0):
    body = [bench.HEADER, *VARIANTS[variant], bench.chain(n)]
    if stepwise:
        for _ in range(stepwise):
            body.append("(run 1)")
            body.append("(print-size)")
    else:
        body.append(f"(run {steps})")
    return "\n".join(body) + "\n"


def _structure(path):
    """Map every constructor node to class-numbering-independent leaf sets.

    Leaf sets are computed as a monotone fixpoint so cyclic e-classes cannot make
    the traversal diverge.
    """
    nodes = json.loads(Path(path).read_text())
    byres = collections.defaultdict(list)
    for node in nodes:
        byres[node["result"]].append(node)
    leaves = {v: set() for v in byres}
    changed = True
    while changed:
        changed = False
        for v, group in byres.items():
            out = set()
            for node in group:
                if node["op"] == "Num":
                    out.add(node["args"][0][1])
                else:
                    for arg in node["args"]:
                        out |= leaves[arg[1]]
            if out != leaves[v]:
                leaves[v] = out
                changed = True

    struct = collections.Counter()
    for node in nodes:
        if node["op"] == "Add":
            sides = [frozenset(leaves[arg[1]]) for arg in node["args"]]
            struct[("Add", sides[0], sides[1], frozenset(leaves[node["result"]]))] += 1
        else:
            struct[("Num", node["args"][0][1])] += 1
    return struct


def structure_digest(struct):
    payload = sorted((repr(k), v) for k, v in struct.items())
    return hashlib.sha256(repr(payload).encode()).hexdigest()[:16]


def run_harness(n, variant, steps, timeout):
    with tempfile.TemporaryDirectory() as td:
        egg = Path(td) / "p.egg"
        graph = Path(td) / "g.json"
        egg.write_text(program(n, variant, steps))
        start = time.monotonic()
        proc = subprocess.run([str(HARNESS), str(egg), str(graph)], capture_output=True, text=True, timeout=timeout)
        wall = time.monotonic() - start
        if proc.returncode != 0:
            return {"error": proc.stderr[-500:]}
        summary = json.loads(proc.stdout.strip().splitlines()[-1])
        struct = _structure(graph)
    return {
        "n": n,
        "variant": variant,
        "matches": summary["matches"],
        "seconds": summary["seconds"],
        "wall_seconds": wall,
        "enodes": summary["enodes"],
        "add_enodes": summary["enodes"] - n,
        "eclasses": summary["eclasses"],
        "structure": structure_digest(struct),
        "_struct": struct,
    }


def run_stepwise(n, variant, max_steps, timeout):
    with tempfile.TemporaryDirectory() as td:
        egg = Path(td) / "p.egg"
        egg.write_text(program(n, variant, stepwise=max_steps))
        proc = subprocess.run([str(PROBE), str(egg)], capture_output=True, text=True, timeout=timeout)
    sizes = bench.parse_sizes(proc.stdout)
    curve, previous, saturated_at = [], None, None
    for i, (adds, _nums) in enumerate(sizes, start=1):
        curve.append({"iteration": i, "add_enodes": adds})
        if previous == adds and saturated_at is None:
            saturated_at = i - 1
        previous = adds
    return {"curve": curve, "saturated_at_iteration": saturated_at}


def run_harness_timed(n, variant, steps, timeout):
    """Engine time and logical matches without graph export (cheap, repeatable)."""
    with tempfile.TemporaryDirectory() as td:
        egg = Path(td) / "p.egg"
        egg.write_text(program(n, variant, steps))
        proc = subprocess.run([str(HARNESS), str(egg)], capture_output=True, text=True, timeout=timeout)
        if proc.returncode != 0:
            return {"error": proc.stderr[-300:]}
        return json.loads(proc.stdout.strip().splitlines()[-1])


def repeat_timing(n, variants, steps, repeats, timeout):
    out = {}
    for variant in variants:
        runs = [run_harness_timed(n, variant, steps, timeout) for _ in range(repeats)]
        seconds = sorted(r["seconds"] for r in runs if "seconds" in r)
        matches = sorted({r["matches"] for r in runs if "matches" in r})
        if not seconds:
            continue
        out[variant] = {
            "seconds": seconds,
            "median_seconds": seconds[len(seconds) // 2],
            "min_seconds": seconds[0],
            "max_seconds": seconds[-1],
            "matches": matches,
        }
        print(
            f"n={n:>2} {variant:<14} median={out[variant]['median_seconds']:.4f}s "
            f"min={seconds[0]:.4f}s max={seconds[-1]:.4f}s matches={matches}",
            flush=True,
        )
    return out


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--n", type=int, nargs="+", default=[4, 6, 8, 10, 12])
    p.add_argument("--steps", type=int, default=200)
    p.add_argument("--timeout", type=float, default=300.0)
    p.add_argument("--repeat", type=int, default=1, help="timed reruns per variant (no graph export)")
    p.add_argument("--out", type=Path, default=ROOT / "out/shortcut-rules")
    a = p.parse_args()
    a.out.mkdir(parents=True, exist_ok=True)
    commit = subprocess.check_output(["git", "rev-parse", "egglog-baseline^{}"], cwd=ROOT, text=True).strip()
    report = {
        "engine": {"baseline_commit": commit, "harness": str(HARNESS), "probe": str(PROBE)},
        "runs": [],
        "equivalence": [],
        "stepwise": [],
        "timing": [],
    }

    for n in a.n:
        by_variant = {}
        for variant in ("generic", "shortcut", "shortcut_naive"):
            rec = run_harness(n, variant, a.steps, a.timeout)
            if "error" in rec:
                print(f"n={n} {variant}: FAILED {rec['error']}", flush=True)
                continue
            by_variant[variant] = rec
            report["runs"].append({k: v for k, v in rec.items() if not k.startswith("_")})
            print(
                f"n={n:>2} {variant:<14} matches={rec['matches']:>9} engine={rec['seconds']:.4f}s "
                f"Add={rec['add_enodes']:>8} classes={rec['eclasses']:>5}",
                flush=True,
            )
        if "generic" in by_variant:
            for other in ("shortcut", "shortcut_naive"):
                if other in by_variant:
                    same = by_variant["generic"]["_struct"] == by_variant[other]["_struct"]
                    report["equivalence"].append({"n": n, other: same})
                    print(f"n={n:>2} generic == {other} closure: {same}", flush=True)

        for variant in ("generic", "shortcut"):
            steps = min(24, max(8, n + 6))
            rec = run_stepwise(n, variant, steps, a.timeout)
            rec.update({"n": n, "variant": variant})
            report["stepwise"].append(rec)
            print(
                f"n={n:>2} {variant:<14} saturates at iteration {rec['saturated_at_iteration']}: "
                f"{[c['add_enodes'] for c in rec['curve']]}",
                flush=True,
            )
        if a.repeat > 1:
            timing = repeat_timing(n, ("generic", "shortcut"), a.steps, a.repeat, a.timeout)
            report["timing"].append({"n": n, "variants": timing})
        (a.out / "results.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"wrote {a.out / 'results.json'}")


if __name__ == "__main__":
    main()
