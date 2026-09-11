"""Bounded ancestor windows over real rule dependency events; retain DAG sharing."""
import argparse
from collections import Counter, defaultdict, deque
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[3]
OUT = ROOT / "experiments/deep_combs"
def term(sort, op, args=()):
    return {"sort": sort, "op": str(op), "args": list(args)}
def path(s):
    return term("Position", "path", (term("PositionPart", x) for x in s.split("/")))
def stable(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"))

def decode_graph(program):
    assert program["sort"] == "CombDAG" and program["args"][0]["op"] == "0"
    definitions, table = {}, program["args"][1]
    while table["op"] == "cons":
        stage, table = table["args"]
        ident = int(stage["args"][0]["op"])
        assert ident not in definitions
        definitions[ident] = stage
    assert table["op"] == "nil"
    edges = []
    for ident, stage in definitions.items():
        if stage["args"][1]["op"] != "expanded":
            continue
        parents = set()
        for edge in stage["args"][2]["args"]:
            parent = int(edge["args"][0]["op"])
            assert parent in definitions
            producer, target = edge["op"][4:].split("->")
            consumer, relation = target.split(":", 1)
            assert definitions[parent]["op"] == "rule:" + producer
            assert stage["op"] == "rule:" + consumer
            parents.add(parent)
            edges.append({"producer": parent, "consumer": ident, "table": relation,
                          "slot": int(edge["args"][1]["op"])})
        assert {int(role["args"][0]["op"]) for role in stage["args"][3]["args"]} == parents
    return definitions, edges

class History:
    def __init__(self, profile, bindings):
        self.witnesses = {(w["scope"], w["consumer"]): w for w in profile["witnesses"]}
        assert len(self.witnesses) == len(profile["witnesses"])
        self.classes = {c["shape"]: c for c in bindings["classes"]}
        self.parents, self.rules, self.roles = {}, {}, {}
        for key, w in self.witnesses.items():
            body, target = w["motif"][1:].split("] -> ")
            self.name(key, target)
            entries = body.split(" + ")
            assert len(entries) == len(w["support"])
            self.parents[key], self.roles[key] = [], {}
            for entry, support in zip(entries, w["support"]):
                rule, tail = entry.split("@", 1)
                role, tail = tail.split(":", 1)
                table, tail = tail.split("#", 1)
                slot = int(tail.split("{", 1)[0])
                assert table == support["table"] and slot == support["slot"]
                parent = (key[0], support["producer"])
                self.name(parent, rule)
                if role in self.roles[key]:
                    assert self.roles[key][role] == parent
                self.roles[key][role] = parent
                self.parents[key].append(parent)
        self.depths = {}
        self.visiting = set()
        for key in self.rules:
            self.depth(key)

    def name(self, key, rule):
        if key in self.rules:
            assert self.rules[key] == rule
        self.rules[key] = rule

    def depth(self, key):
        if key in self.depths:
            return self.depths[key]
        assert key not in self.visiting, "event dependency cycle; not a rule-name self recurrence"
        self.visiting.add(key)
        depth = max((1 + self.depth(p) for p in self.parents.get(key, ())), default=0)
        self.visiting.remove(key)
        self.depths[key] = depth
        return depth

    def window(self, root, limit):
        assert limit >= 1
        # BFS first arrival gives minimum distance even when paths overlap.
        distance, ids = {root: 0}, {root: 0}
        queue = deque([root])
        while queue:
            key = queue.popleft()
            if distance[key] == limit:
                continue
            for p in self.parents.get(key, ()):
                if p not in ids:
                    ids[p] = len(ids)
                    distance[p] = distance[key] + 1
                    queue.append(p)
        definitions, edges, evidence = [], [], []
        incoming_consumers = defaultdict(set)
        cut = missing = 0
        for key, ident in ids.items():
            ref = term("EventRef", ident)
            w = self.witnesses.get(key)
            if distance[key] == limit:
                status = "depth-frontier"
                cut += 1
            elif w is None:
                status = "no-recorded-incoming"
                missing += 1
            else:
                status = "expanded"
            args = [ref, term("Coverage", status)]
            if status == "expanded":
                c = self.classes[w["motif"]]
                links = []
                for p, support in zip(self.parents[key], w["support"]):
                    assert p in ids
                    incoming_consumers[p].add(key)
                    links.append(term("CombEdge", f"use:{self.rules[p]}->{self.rules[key]}:{support['table']}", [
                        term("EventRef", ids[p]), term("ReadSlot", support["slot"]),
                        term("Positions", "producer", map(path, support["producer_sites"])),
                        term("Positions", "consumer", map(path, support["consumer_sites"])),
                        term("SupportKind", support["kind"]),
                        term("UnionSupport", "external" if support["union_dependencies"] else "none"),
                    ]))
                    edges.append({"producer": ids[p], "consumer": ident, "table": support["table"], "slot": support["slot"]})
                aliases = term("RoleMap", "producer-roles", (
                    term("Role", role, [term("EventRef", ids[p])]) for role, p in sorted(self.roles[key].items())))
                args += [term("CombSupport", "supports", links), aliases, c["normalized"],
                         term("PortLabels", "inputs", (term("PortLabel", x) for x in c["producer_port_labels"])),
                         term("PortLabels", "outputs", (term("PortLabel", x) for x in c["consumer_port_labels"])),
                         term("Boundary", "ports", (term("PortIndex", x) for x in c["boundary_consumer_ports"])),
                         term("Coverage", f"unexplained-reads:{w['boundary_read_count']}")]
                evidence.append({"node": ident, "scope": key[0], "match": key[1], "witness": w})
            definitions.append(term("CombStage", f"rule:{self.rules[key]}", args))
        # Explicit definition table: shared ancestors occur once, referenced by ID.
        table = term("CombNodes", "nil")
        for definition in reversed(definitions):
            table = term("CombNodes", "cons", [definition, table])
        program = term("CombDAG", "root", [term("EventRef", 0), table])
        decoded, decoded_edges = decode_graph(program)
        assert len(decoded) == len(ids) and decoded_edges == edges
        for key, ident in ids.items():
            if decoded[ident]["args"][1]["op"] == "expanded":
                assert decoded[ident]["args"][4] == self.classes[self.witnesses[key]["motif"]]["normalized"]
        return {
            "program": program, "root": list(root),
            "source_nodes": [{"local": i, "scope": k[0], "match": k[1], "rule": self.rules[k], "distance": distance[k]} for k, i in ids.items()],
            "edges": edges, "evidence": evidence,
            "stats": {"nodes": len(ids), "edges": len(edges), "shared_ancestors": sum(len(cs)>1 for cs in incoming_consumers.values()),
                      "depth_frontier": cut, "missing_history_frontier": missing,
                      "available_history_depth": self.depths[root]},
        }

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--learn", action="store_true")
    args = parser.parse_args()
    profile = json.loads((ROOT / "experiments/annotated_export/math_microbenchmark/profile.json").read_text())
    bindings = json.loads((ROOT / "experiments/binding_program/math.json").read_text())
    history = History(profile, bindings)
    OUT.mkdir(exist_ok=True)
    by_class = defaultdict(list)
    for key, w in history.witnesses.items():
        by_class[w["motif"]].append(key)
    # Hold anchor count and identities fixed across depths. Keep all-root windows too.
    anchors = [max(by_class[c["shape"]], key=lambda k: (history.depths[k], -k[0], -k[1])) for c in bindings["classes"]]
    summaries = []
    if args.learn:
        subprocess.run([sys.executable, str(ROOT / "tools/babble_adapter/scripts/bootstrap.py")], check=True)
        subprocess.run(["cargo", "build", "--release", "--locked", "--manifest-path", "tools/babble_adapter/Cargo.toml"], cwd=ROOT, check=True)
    for depth in range(1, 5):
        windows = {key: history.window(key, depth) for key in sorted(history.witnesses)}
        (OUT / f"depth-{depth}.windows.json").write_text(json.dumps(list(windows.values()), separators=(",", ":")))
        entries = [{"class": f"scope-{k[0]}:match-{k[1]}", "split": "test" if i%5==0 else "train",
                    "occurrences": 1, "program": windows[k]["program"]} for i,k in enumerate(anchors)]
        corpus = {"schema": "deep-rule-comb-dag-v1", "rule_dictionary": profile["rule_labels"], "entries": entries,
                  "scope": "fixed 124 deepest representative roots across depth 1-4; all 582 windows also exported; overlapping history, not independent heldout runs"}
        corpus_path = OUT / f"depth-{depth}.corpus.json"
        corpus_path.write_text(json.dumps(corpus, separators=(",", ":")))
        membership = Counter((n["scope"],n["match"]) for w in windows.values() for n in w["source_nodes"])
        summary = {"depth": depth, "roots": len(windows), "unique_window_encodings": len({stable(w["program"]) for w in windows.values()}),
                   "max_nodes": max(w["stats"]["nodes"] for w in windows.values()), "max_edges": max(w["stats"]["edges"] for w in windows.values()),
                   "windows_with_shared_ancestors": sum(w["stats"]["shared_ancestors"]>0 for w in windows.values()),
                   "events_in_multiple_windows": sum(v>1 for v in membership.values()),
                   "learning_anchors": len(anchors), "anchor_mean_nodes": sum(windows[k]["stats"]["nodes"] for k in anchors)/len(anchors)}
        if args.learn:
            output = OUT / f"depth-{depth}.learning.json"
            try:
                subprocess.run([str(ROOT / "tools/babble_adapter/target/release/combine-babble"), str(corpus_path), str(output), "comb-mining"], check=True, timeout=180)
                result = json.loads(output.read_text())
                summary["learning"] = {key: result[key] for key in ["train", "test", "selected_train_libraries", "lossless_expansion", "exact_dag_control", "comb_mining"]}
                summary["library_order_normalization"] = result["library_order_normalization"]
                summary["learning_status"] = "completed"
            except subprocess.TimeoutExpired:
                summary["learning_status"] = "180-second-budget-exhausted"
            except subprocess.CalledProcessError as error:
                summary["learning_status"] = f"failed-with-exit-{error.returncode}"
        summaries.append(summary)
        (OUT / "summary.json").write_text(json.dumps(summaries, indent=2)+"\n")
        print(json.dumps(summary), flush=True)
        if depth == 4:
            example = max(windows.values(), key=lambda w: (w["stats"]["shared_ancestors"], w["stats"]["nodes"]))
            (OUT / "example.json").write_text(json.dumps(example, indent=2)+"\n")
            lines = ["# 一个深度 4 的共享依赖窗口", "", "箭头表示 producer → consumer；同一事件只画一次。", "", "```mermaid", "graph LR"]
            for n in example["source_nodes"]:
                lines.append(f'  n{n["local"]}["{n["rule"]} · event {n["match"]}"]')
            for e in example["edges"]:
                lines.append(f'  n{e["producer"]} -->|"{e["table"]} #{e["slot"]}"| n{e["consumer"]}')
            lines += ["```", "", "共享祖先数量：" + str(example["stats"]["shared_ancestors"]), ""]
            (OUT / "example.md").write_text("\n".join(lines).rstrip()+"\n")

if __name__ == "__main__":
    main()
