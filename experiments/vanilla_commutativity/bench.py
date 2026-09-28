#!/usr/bin/env python3
"""E-node counts for Add commutativity over n elements in pristine egglog.

The engine is the immutable ``egglog-baseline`` tag (c7553a6, upstream ebba7bb),
driven through standalone public-API consumers: no tracing, no RipSat analysis,
no egg_layout code in the timed path. Two independent measurements are taken:

* ``bin/pristine`` (experiments/native_compare) enumerates Math-sorted e-nodes and
  therefore also reports e-class counts;
* ``bin/probe`` runs ``(print-size)`` after each iteration, which reads table row
  counts directly and stays cheap while the e-graph is growing.

Configurations (both shapes have n leaves and n-1 Add positions):

* ``chain``    -- right-nested ``(Add (Num 0) (Add (Num 1) ... (Num n-1)))``
* ``balanced`` -- complete binary tree over the same leaves
* ``comm``     -- ``(rewrite (Add a b) (Add b a))`` only
* ``ac``       -- associativity + commutativity, stepped one iteration at a time
  under an external time/RSS guard so the blow-up can be observed, not completed.
"""
import argparse
import json
import os
import re
import signal
import subprocess
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
BIN = ROOT / "out/vanilla-commutativity/bin"
HARNESS = BIN / "pristine"
PROBE = BIN / "probe"

HEADER = "(datatype Math (Num i64) (Add Math Math))"
COMM = "(rewrite (Add a b) (Add b a))"
ASSOC = "(rewrite (Add a (Add b c)) (Add (Add a b) c))"


def chain(n):
    expr = f"(Num {n - 1})"
    for i in range(n - 2, -1, -1):
        expr = f"(Add (Num {i}) {expr})"
    return expr


def balanced(n):
    level = [f"(Num {i})" for i in range(n)]
    while len(level) > 1:
        nxt = []
        for i in range(0, len(level) - 1, 2):
            nxt.append(f"(Add {level[i]} {level[i + 1]})")
        if len(level) % 2:
            nxt.append(level[-1])
        level = nxt
    return level[0]


def program(n, shape, rules, steps, print_size=False):
    expr = chain(n) if shape == "chain" else balanced(n)
    body = [HEADER, COMM]
    if rules == "ac":
        body.append(ASSOC)
    body.append(expr)
    body.append(f"(run {steps})")
    if print_size:
        body.append("(print-size)")
    return "\n".join(body) + "\n"


def run_with_time(cmd, timeout):
    """Run cmd under /usr/bin/time -l in its own process group; kill past `timeout`."""
    start = time.monotonic()
    proc = subprocess.Popen(
        ["/usr/bin/time", "-l", *cmd],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        start_new_session=True,
    )
    reason = None
    try:
        stdout, stderr = proc.communicate(timeout=timeout)
    except subprocess.TimeoutExpired:
        reason = "external_time_guard"
        os.killpg(proc.pid, signal.SIGKILL)
        stdout, stderr = proc.communicate()
    m = re.search(r"(\d+)\s+maximum resident set size", stderr)
    return {
        "stdout": stdout,
        "stderr": stderr,
        "exit_code": proc.returncode,
        "wall_seconds": time.monotonic() - start,
        "peak_rss_bytes": int(m.group(1)) if m else None,
        "stop_reason": reason,
    }


def parse_sizes(stdout):
    """[(Add rows, Num rows), ...] in the order the (print-size) commands ran."""
    adds = [int(m) for m in re.findall(r"\(Add (\d+)\)", stdout)]
    nums = [int(m) for m in re.findall(r"\(Num (\d+)\)", stdout)]
    return list(zip(adds, nums))


def measure_comm(n, shape, timeout):
    """Commutativity only: saturate, then count e-nodes and e-classes exactly."""
    with tempfile.TemporaryDirectory() as td:
        egg = Path(td) / "p.egg"
        graph = Path(td) / "g.json"
        egg.write_text(program(n, shape, "comm", 100))
        start = time.monotonic()
        proc = subprocess.run([str(HARNESS), str(egg), str(graph)], capture_output=True, text=True, timeout=timeout)
        wall = time.monotonic() - start
        summary = json.loads(proc.stdout.strip().splitlines()[-1])
        ops, classes = {}, set()
        for node in json.loads(graph.read_text()):
            ops[node["op"]] = ops.get(node["op"], 0) + 1
            classes.add(node["result"])
        return {
            "n": n,
            "shape": shape,
            "rules": "comm",
            "seconds": summary["seconds"],
            "matches": summary["matches"],
            "enodes": summary["enodes"],
            "eclasses": summary["eclasses"],
            "ops": ops,
            "distinct_results": len(classes),
            "wall_seconds": wall,
        }


def sweep_ac(n, shape, max_steps, timeout, node_ceiling):
    """Associativity + commutativity: (run k) in a fresh guarded process per step.

    The sweep stops as soon as a step exceeds `node_ceiling`, so the next, larger
    iteration (which is what saturates memory) is never started.
    """
    steps_out = []
    stopped = None
    for k in range(1, max_steps + 1):
        with tempfile.TemporaryDirectory() as td:
            egg = Path(td) / "p.egg"
            egg.write_text(program(n, shape, "ac", k, print_size=True))
            res = run_with_time([str(PROBE), str(egg)], timeout)
        sizes = parse_sizes(res["stdout"])
        if res["stop_reason"] or res["exit_code"] != 0 or not sizes:
            stopped = res["stop_reason"] or f"exit_code={res['exit_code']}"
            steps_out.append(
                {
                    "run": k,
                    "add_enodes": None,
                    "total_math_enodes": None,
                    "wall_seconds": res["wall_seconds"],
                    "peak_rss_bytes": res["peak_rss_bytes"],
                    "stopped": stopped,
                    "stderr_tail": res["stderr"][-300:],
                }
            )
            break
        adds, nums = sizes[-1]
        steps_out.append(
            {
                "run": k,
                "add_enodes": adds,
                "num_enodes": nums,
                "total_math_enodes": adds + nums,
                "wall_seconds": res["wall_seconds"],
                "peak_rss_bytes": res["peak_rss_bytes"],
            }
        )
        print(f"  ac {shape} run {k}: Add={adds} total={adds + nums} "
              f"({res['wall_seconds']:.2f}s, {res['peak_rss_bytes'] / 1024**2:.0f}MiB)", flush=True)
        if adds > node_ceiling:
            stopped = f"node_ceiling({node_ceiling})"
            break
    return {
        "n": n,
        "shape": shape,
        "rules": "ac",
        "steps": steps_out,
        "stopped": stopped,
    }


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--n", type=int, default=300)
    p.add_argument("--out", type=Path, default=ROOT / "out/vanilla-commutativity")
    p.add_argument("--timeout", type=float, default=300.0)
    p.add_argument("--ac-steps", type=int, default=7)
    p.add_argument("--node-ceiling", type=int, default=3_000_000)
    a = p.parse_args()
    a.out.mkdir(parents=True, exist_ok=True)
    for b in (HARNESS, PROBE):
        if not b.exists():
            raise SystemExit(f"missing {b}; run experiments/vanilla_commutativity/build.py first")

    report = {
        "n": a.n,
        "engine": "egglog-baseline c7553a6 (upstream ebba7bb), standalone public-API harness",
        "guard": {"seconds_per_step": a.timeout, "node_ceiling": a.node_ceiling},
        "comm": [],
        "ac": [],
    }
    for shape in ("chain", "balanced"):
        rec = measure_comm(a.n, shape, a.timeout)
        report["comm"].append(rec)
        print(f"comm {shape}: Add={rec['ops']['Add']} Num={rec['ops']['Num']} "
              f"enodes={rec['enodes']} eclasses={rec['eclasses']} ({rec['seconds']:.4f}s)", flush=True)
    for shape in ("chain", "balanced"):
        rec = sweep_ac(a.n, shape, a.ac_steps, a.timeout, a.node_ceiling)
        report["ac"].append(rec)
        print(f"ac {shape}: steps={len(rec['steps'])} stopped={rec['stopped']}", flush=True)
        (a.out / "results.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"wrote {a.out / 'results.json'}")


if __name__ == "__main__":
    main()
