"""Frozen-library scoring + Rust policy replay over recorded prefix alternatives."""
from dataclasses import dataclass
from functools import lru_cache
import hashlib
import json
from pathlib import Path
import subprocess
from collections import defaultdict
from deep_combs import History, ROOT, stable

OUT = ROOT / "experiments/prefix_feedback"

@dataclass(frozen=True)
class Hole:
    index: int

def tree(dump):
    return dump["op"], tuple(tree(a) for a in dump["args"])

def encode(p):
    # Same typed term encoding as the adapter, validated by expansion below.
    assert set(p) == {"sort", "op", "args"}
    label = "term:" + json.dumps([p["sort"], p["op"]], ensure_ascii=False, separators=(",", ":"))
    return f'Data({json.dumps(label, ensure_ascii=False)}, {len(p["args"])})', tuple(map(encode, p["args"]))

@lru_cache(None)
def size(t):
    return 1 + sum(size(a) for a in t[1])

def codec_bytes(t):
    symbols, ids, nodes = {}, {}, []
    def visit(t):
        args = tuple(visit(a) for a in t[1])
        op = symbols.setdefault(t[0], len(symbols))
        key = (op, args)
        if key not in ids:
            ids[key] = len(nodes)
            nodes.append(key)
        return ids[key]
    root = visit(t)
    return len(json.dumps({"symbols": list(symbols), "nodes": nodes, "root": root}, ensure_ascii=False, separators=(",", ":")).encode())

class FrozenLibrary:
    def __init__(self, report):
        self.definitions = [(d["id"], tree(d["definition"])) for d in report["libraries"]]
        self.patterns, self.arities = {}, {}
        libraries = {}
        def evaluate(t, env, libs):
            op, args = t
            if op == "Lambda":
                return ("closure", args[0], env, dict(libs))
            if op.startswith("Var("):
                return env[int(op[4:-1])]
            if op.startswith("Ref("):
                return libs[int(op[4:-1])]
            if op == "Apply":
                return apply(evaluate(args[0], env, libs), evaluate(args[1], env, libs))
            assert op.startswith("Data("), "only pure, already lifted definitions supported"
            return op, tuple(evaluate(a, env, libs) for a in args)
        def apply(fn, value):
            assert fn[0] == "closure"
            return evaluate(fn[1], (value,) + fn[2], fn[3])
        for ident, definition in self.definitions:
            fn = evaluate(definition, (), libraries)
            libraries[ident] = fn
            body, arity = definition, 0
            while body[0] == "Lambda":
                arity += 1
                body = body[1][0]
            self.arities[ident] = arity
            for i in reversed(range(arity)):
                fn = apply(fn, Hole(i))
            assert not isinstance(fn, Hole)
            self.patterns[ident] = fn

    @staticmethod
    def match(pattern, t, bindings):
        if isinstance(pattern, Hole):
            if pattern.index in bindings:
                return bindings[pattern.index] == t
            bindings[pattern.index] = t
            return True
        return pattern[0] == t[0] and len(pattern[1]) == len(t[1]) and all(
            FrozenLibrary.match(a, b, bindings) for a, b in zip(pattern[1], t[1]))

    @lru_cache(None)
    def rewrite(self, t):
        best = (t[0], tuple(self.rewrite(a) for a in t[1]))
        for ident, pattern in self.patterns.items():
            bindings = {}
            if not self.match(pattern, t, bindings):
                continue
            call = (f"Ref({ident})", ())
            for i in reversed(range(self.arities[ident])):
                call = ("Apply", (call, self.rewrite(bindings[i])))
            if size(call) < size(best):
                best = call
        return best

    def expand(self, t):
        if t[0] == "Apply":
            head, args = t, []
            while head[0] == "Apply":
                args.append(self.expand(head[1][1]))
                head = head[1][0]
            assert head[0].startswith("Ref(")
            ident = int(head[0][4:-1])
            assert len(args) == self.arities[ident]
            def fill(p):
                return args[p.index] if isinstance(p, Hole) else (p[0], tuple(fill(a) for a in p[1]))
            return fill(self.patterns[ident])
        assert t[0].startswith("Data(")
        return t[0], tuple(self.expand(a) for a in t[1])

    def cost(self, p):
        raw = encode(p)
        compressed = self.rewrite(raw)
        assert self.expand(compressed) == raw
        def installed(body):
            for ident, definition in reversed(self.definitions):
                body = (f"Lib({ident})", (definition, body))
            return body
        # Charge the SAME resident model to both representations. Otherwise mere
        # symbol/subtree overlap with the library would masquerade as macro reuse.
        return {"raw_bytes": codec_bytes(installed(raw)), "coded_bytes": codec_bytes(installed(compressed)),
                "raw_ast": size(raw), "coded_body_ast": size(compressed)}

def main():
    OUT.mkdir(exist_ok=True)
    model_path = ROOT / "experiments/deep_combs/depth-4.learning.json"
    digest = hashlib.sha256(model_path.read_bytes()).hexdigest()
    epoch = int(digest[:15], 16)
    model = FrozenLibrary(json.loads(model_path.read_text()))
    profile = json.loads((ROOT / "experiments/annotated_export/math_microbenchmark/profile.json").read_text())
    bindings = json.loads((ROOT / "experiments/binding_program/math.json").read_text())
    history = History(profile, bindings)
    heldout = json.loads((ROOT / "experiments/deep_combs/depth-4.corpus.json").read_text())
    targets = [tuple(map(int, e["class"].replace("scope-", "").replace("match-", "").split(":"))) for e in heldout["entries"] if e["split"] == "test"]
    subprocess.run(["cargo", "build", "--release", "--bin", "prefix_policy"], cwd=ROOT, check=True)
    process = subprocess.Popen([str(ROOT / "target/release/prefix_policy")], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
    def request(value):
        process.stdin.write(json.dumps(value) + "\n")
        process.stdin.flush()
        return json.loads(process.stdout.readline())
    @lru_cache(None)
    def window(key):
        return history.window(key, 4)
    @lru_cache(None)
    def cost(key):
        return model.cost(window(key)["program"])
    scenarios = []
    for target in targets:
        cone = {(n["scope"], n["match"]) for n in window(target)["source_nodes"]}
        successors = defaultdict(set)
        for child in cone:
            for parent in history.parents.get(child, ()):
                if parent in cone:
                    successors[parent].add(child)
        seed = min(k for k in cone if not history.parents.get(k))
        for mode in ("fifo", "compression-with-exploration"):
            request({"reset": True, "epoch": epoch, "period": 4, "max_wait": 8})
            frontier, visited, decisions = {}, {seed}, []
            def enqueue(parent):
                for child in sorted(successors[parent]):
                    if child in visited or child in frontier:
                        continue
                    before, after = cost(parent), cost(child)
                    raw_delta = after["raw_bytes"] - before["raw_bytes"]
                    coded_delta = after["coded_bytes"] - before["coded_bytes"]
                    base_nodes = {(n["scope"], n["match"]) for n in window(parent)["source_nodes"]}
                    frontier[child] = {"id": child[1], "parent": parent, "gain": raw_delta-coded_delta,
                                       "raw_delta": raw_delta, "coded_delta": coded_delta,
                                       "coarse": bool(set(history.parents[child])-base_nodes), "born": len(decisions)}
            enqueue(seed)
            for tick in range(6):
                if not frontier:
                    break
                available = [{k: v for k, v in c.items() if k in ("id", "gain", "parent", "coarse")} for c in frontier.values()]
                if mode == "fifo":
                    child = min(frontier, key=lambda c: (frontier[c]["born"], c))
                    reason = "fifo-control"
                else:
                    choice = request({"epoch": epoch, "candidates": list(frontier.values())})
                    child = next(c for c in frontier if c[1] == choice["candidate"])
                    reason = choice["reason"]
                candidate = frontier.pop(child)
                visited.add(child)
                decisions.append({**candidate, "target": child, "reason": reason, "tick": tick, "available_candidates": available})
                enqueue(child)
            scenarios.append({"anchor": target, "seed": seed, "policy": mode, "decisions": decisions,
                              "reached_anchor": target in visited, "pending": len(frontier)})
    process.stdin.close()
    assert process.wait(timeout=10) == 0
    aggregate = {}
    for mode in ("fifo", "compression-with-exploration"):
        runs = [s for s in scenarios if s["policy"] == mode]
        choices = [d for s in runs for d in s["decisions"]]
        first_positive = [next((d["tick"]+1 for d in s["decisions"] if d["gain"]>0), None) for s in runs]
        positive_runs = [n for n in first_positive if n is not None]
        aggregate[mode] = {"runs_with_positive_choice": len(positive_runs),
                           "mean_first_positive_choice": sum(positive_runs)/len(positive_runs) if positive_runs else None,
                           "choices": len(choices), "positive_gain_choices": sum(d["gain"]>0 for d in choices),
                           "coarse_choices": sum(d["coarse"] for d in choices), "anchors_reached": sum(s["reached_anchor"] for s in runs),
                           "exploration_choices": sum(d["reason"] in ("exploration-quota", "age-priority") for d in choices),
                           "sum_local_priority_estimates": sum(d["gain"] for d in choices)}
    changed = sum([d["id"] for d in a["decisions"]] != [d["id"] for d in b["decisions"]] for a,b in zip(scenarios[::2],scenarios[1::2]))
    report = {"runs_with_different_order": changed, "model_sha256": digest, "model_epoch": epoch, "heldout_anchors": len(targets),
              "settings": {"budget_per_anchor": 6, "exploration_period": 4, "maximum_wait_priority": 8},
              "score": "delta(raw exact-DAG bytes) - delta(frozen-library exact-DAG bytes), relative to each candidate's parent prefix",
              "scope": "offline selection of recorded prefix continuations; coarse support already exists in recorded history; overlapping heldout cones; no runtime rule reordering or tier-0 compression",
              "cost_scope": "same fixed installed library included in BOTH raw and coded baselines; shared original rule dictionary constant and omitted from both; source metadata excluded; local estimates cannot be summed as global memory savings",
              "aggregate": aggregate, "scenarios": scenarios}
    (OUT / "results.json").write_text(json.dumps(report, indent=2)+"\n")
    print(json.dumps(aggregate), flush=True)

if __name__ == "__main__":
    main()
