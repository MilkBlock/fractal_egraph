"""Replay bounded bridge reservations on known history, not speculative mutation."""
import argparse
from collections import defaultdict
from functools import lru_cache
import hashlib
import json
import subprocess
from deep_combs import History, ROOT
from prefix_feedback import FrozenLibrary
from relative_bindings import convert
from bridge_plans import plan, Reservation

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--relative", action="store_true")
    parser.add_argument("--ignore-startup-cost", action="store_true")
    args = parser.parse_args()
    out = ROOT / "experiments/bridge_lookahead"
    out.mkdir(exist_ok=True)
    path = ROOT / ("experiments/relative_bindings/learning.json" if args.relative else "experiments/deep_combs/depth-4.learning.json")
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    epoch = int(digest[:15], 16)
    model = FrozenLibrary(json.loads(path.read_text()))
    history = History(json.loads((ROOT / "experiments/annotated_export/math_microbenchmark/profile.json").read_text()),
                      json.loads((ROOT / "experiments/binding_program/math.json").read_text()))
    corpus = json.loads((ROOT / "experiments/deep_combs/depth-4.corpus.json").read_text())
    targets = [tuple(map(int, e["class"].replace("scope-", "").replace("match-", "").split(":"))) for e in corpus["entries"] if e["split"] == "test"]
    @lru_cache(None)
    def window(key):
        return history.window(key, 4)
    @lru_cache(None)
    def score(key):
        p = window(key)["program"]
        if args.relative:
            p, _ = convert(p)
        return model.cost(p)
    def gain(a, b):
        a, b = score(a), score(b)
        return (b["raw_bytes"]-a["raw_bytes"])-(b["coded_bytes"]-a["coded_bytes"])
    def candidate(base, child):
        base_nodes = {(n["scope"], n["match"]) for n in window(base)["source_nodes"]}
        return {"id": child[1], "parent": base, "gain": gain(base,child),
                "coarse": bool(set(history.parents[child])-base_nodes)}
    subprocess.run(["cargo", "build", "--release", "--bin", "prefix_policy"], cwd=ROOT, check=True)
    process = subprocess.Popen([str(ROOT / "target/release/prefix_policy")], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
    def request(v):
        process.stdin.write(json.dumps(v)+"\n"); process.stdin.flush()
        return json.loads(process.stdout.readline())
    runs = []
    for target in targets:
        cone = {(n["scope"],n["match"]) for n in window(target)["source_nodes"]}
        successors = defaultdict(set)
        for child in cone:
            for parent in history.parents.get(child, ()):
                if parent in cone:
                    successors[parent].add(child)
        seed = min(k for k in cone if not history.parents.get(k))
        for mode in ("immediate", "three-step-bridge"):
            request({"reset": True, "epoch": epoch, "period": 4, "max_wait": 8})
            frontier, visited, decisions = {}, {seed}, []
            reservation, active_plan, last = None, None, None
            completed, proposals, expanded = [], [], 0
            def enqueue(parent):
                for child in sorted(successors[parent]):
                    if child not in visited and child not in frontier:
                        frontier[child] = candidate(parent, child)
            enqueue(seed)
            for tick in range(6):
                if not frontier:
                    break
                reserved = reservation.take(frontier, epoch, 0) if reservation else None
                if reserved is not None:
                    # The protected plan's predecessor is its base, even if a
                    # different incoming edge discovered this target earlier.
                    frontier[reserved] = candidate(last, reserved)
                    choice = request({"epoch": epoch, "candidates": list(frontier.values()), "reserved": reserved[1]})
                    child = reserved
                else:
                    plans, scored = {}, []
                    for child, c in frontier.items():
                        item = dict(c)
                        best, work = plan(c["parent"], child, successors, score, visited,
                                          min(3,6-tick) if mode=="three-step-bridge" else 1,
                                          charge_startup=not args.ignore_startup_cost)
                        expanded += work["explored_states"]
                        item["gain"] = best.net_gain
                        if mode == "three-step-bridge":
                            plans[child] = best
                        scored.append(item)
                    choice = request({"epoch": epoch, "candidates": scored})
                    child = next(c for c in frontier if c[1] == choice["candidate"])
                    if child in plans and len(plans[child].path)>1:
                        p = plans[child]
                        active_plan = {"base": frontier[child]["parent"], "path": p.path, "predicted_gain": p.net_gain,
                                       "compression_gain":p.compression_gain,"startup_bytes":p.startup_bytes,
                                       "first_gain": frontier[child]["gain"], "library_entry": p.library_entry}
                        proposals.append(active_plan)
                        reservation = Reservation(epoch, 0, list(p.path[1:]))
                c = frontier.pop(child)
                decisions.append({"target": child, "base": c["parent"], "immediate_gain": c["gain"],
                                  "selection_score": choice["gain"], "reason": choice["reason"], "coarse": c["coarse"]})
                visited.add(child); enqueue(child); last = child
                if active_plan and reservation and not reservation.pending:
                    realized = gain(active_plan["base"], child)-active_plan["startup_bytes"]
                    assert realized == active_plan["predicted_gain"]
                    completed.append({**active_plan, "realized_gain": realized})
                    active_plan = None; reservation = None
            runs.append({"anchor": target, "mode": mode, "decisions": decisions, "plans": proposals,
                         "completed_plans": completed, "planning_states": expanded, "reached_anchor": target in visited})
    process.stdin.close(); assert process.wait(timeout=10)==0
    aggregate = {}
    for mode in ("immediate", "three-step-bridge"):
        selected = [r for r in runs if r["mode"]==mode]
        plans = [p for r in selected for p in r["completed_plans"]]
        aggregate[mode] = {"choices": sum(len(r["decisions"]) for r in selected), "anchors_reached": sum(r["reached_anchor"] for r in selected),
                           "completed_multi_step_plans": len(plans), "nonpositive_first_step_plans": sum(p["first_gain"]<=0 for p in plans),
                           "negative_first_step_plans": sum(p["first_gain"]<0 for p in plans), "planning_states": sum(r["planning_states"] for r in selected)}
    report = {"encoding": "relative-interface" if args.relative else "local-ids", "model_sha256": digest, "aggregate": aggregate, "runs": runs,
              "scope": "known-history lookahead, same 25 overlapping heldout cones; successful library entry is a finite reuse witness, not a fractal invariant or online speedup",
              "budget": {"total_adoptions":6,"bridge_steps":3,"states_per_head":128},
              "startup_charged":not args.ignore_startup_cost,
              "cost": "compression advantage minus positive encoded-prefix growth before first library entry; conservative startup fee, not tier-0 bytes; fixed metadata cancels within each encoding"}
    filename=("relative" if args.relative else "raw")+("-unpriced" if args.ignore_startup_cost else "")+".json"
    (out / filename).write_text(json.dumps(report, indent=2)+"\n")
    print(json.dumps(aggregate), flush=True)

if __name__ == "__main__":
    main()
