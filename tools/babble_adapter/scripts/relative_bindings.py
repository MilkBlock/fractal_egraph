"""Reversible root-interface addresses for DAG references and rule-local sites."""
import heapq
import json
import subprocess
import sys
from pathlib import Path
from deep_combs import ROOT, term, stable, decode_graph

OUT = ROOT / "experiments/relative_bindings"

def compose_address(base, suffix):
    """Substitute a root interface, preserving every read-position constraint."""
    assert base["sort"] == suffix["sort"] == "BindingAddress"
    if suffix["op"] == "root":
        return base
    assert suffix["op"] == "via"
    prefix = compose_address(base, suffix["args"][0])
    frame = suffix["args"][1]
    if prefix["op"] == "via" and prefix["args"][1]["op"].startswith("use:") and frame["op"].startswith("use:"):
        current = prefix["args"][1]["op"][4:].split("->")[0]
        required = frame["op"].split("->")[1].split(":", 1)[0]
        assert current == required, "incompatible rule interfaces"
    return term("BindingAddress", "via", [prefix, frame])

def relative_suffix(base, target):
    """None denotes a cross-interface reference, not a fabricated descent."""
    if base == target:
        return term("BindingAddress", "root")
    if target["op"] == "root":
        return None
    suffix = relative_suffix(base, target["args"][0])
    return None if suffix is None else term("BindingAddress", "via", [suffix, target["args"][1]])

def site(p):
    parts = [x["op"] for x in p["args"]]
    if len(parts) < 4 or parts[0] not in ("head", "body") or parts[2] != "expr":
        return term("OpaqueSite", "preserved", [p])
    if len(parts[4:]) % 2 or any(parts[i] != "args" for i in range(4, len(parts), 2)):
        return term("OpaqueSite", "preserved", [p])
    if not parts[1].isdigit() or not parts[3].isdigit() or any(not parts[i].isdigit() for i in range(5,len(parts),2)):
        return term("OpaqueSite", "preserved", [p])
    return term("RelativeSite", "at", [term("RuleAnchor", ":".join((parts[0], parts[1], parts[3]))),
                term("ArgRoute", "descend", [term("ArgIndex", parts[i]) for i in range(5, len(parts), 2)])])

def unsite(p):
    if p["sort"] == "OpaqueSite":
        return p["args"][0]
    side, atom, expr = p["args"][0]["op"].split(":")
    parts = [side, atom, "expr", expr]
    for a in p["args"][1]["args"]:
        parts += ["args", a["op"]]
    return term("Position", "path", [term("PositionPart", x) for x in parts])

def convert(program):
    definitions, _ = decode_graph(program)
    root = int(program["args"][0]["op"])
    pending = [(0, (), root, term("BindingAddress", "root"))]
    addresses, ranks = {}, {}
    while pending:
        distance, keys, ident, address = heapq.heappop(pending)
        if ident in addresses:
            continue
        addresses[ident], ranks[ident] = address, (distance, keys)
        stage = definitions[ident]
        if stage["args"][1]["op"] != "expanded":
            continue
        for edge in stage["args"][2]["args"]:
            args = edge["args"]
            parent = int(args[0]["op"])
            # A receiver read interface, not a concrete match/e-class identifier.
            interface = term("ReadInterface", edge["op"], [args[1],
                term("Sites", "receiver", [site(p) for p in args[3]["args"]]),
                term("Sites", "producer", [site(p) for p in args[2]["args"]])])
            key = stable(interface)
            heapq.heappush(pending, (distance+1, keys+(key,), parent,
                                    term("BindingAddress", "via", [address, interface])))
    assert set(addresses) == set(definitions), "unreachable definitions require a separate interface"
    assert len({stable(a) for a in addresses.values()}) == len(addresses), "ambiguous read interfaces cannot identify distinct events"
    def rewrite(p):
        if p["sort"] == "EventRef":
            return addresses[int(p["op"])]
        if p["sort"] == "Position" and p["op"] == "path":
            return site(p)
        return term(p["sort"], p["op"], [rewrite(a) for a in p["args"]])
    table = term("CombNodes", "nil")
    for ident in reversed(sorted(definitions, key=lambda i: ranks[i])):
        table = term("CombNodes", "cons", [rewrite(definitions[ident]), table])
    relative = term("CombDAG", "root", [addresses[root], table])
    witness = {"original_order": list(definitions), "addresses": {str(i): a for i, a in addresses.items()}}
    assert restore(relative, witness) == program
    return relative, witness

def restore(relative, witness):
    inverse = {stable(a): ident for ident, a in witness["addresses"].items()}
    def visit(p):
        if p["sort"] == "BindingAddress":
            return term("EventRef", inverse[stable(p)])
        if p["sort"] in ("RelativeSite", "OpaqueSite"):
            return unsite(p)
        return term(p["sort"], p["op"], [visit(a) for a in p["args"]])
    restored = visit(relative)
    definitions, _ = decode_graph(restored)
    table = term("CombNodes", "nil")
    for ident in reversed(witness["original_order"]):
        table = term("CombNodes", "cons", [definitions[ident], table])
    return term("CombDAG", "root", [restored["args"][0], table])

def main():
    OUT.mkdir(exist_ok=True)
    subprocess.run([sys.executable, str(Path(__file__).with_name("bootstrap.py"))], check=True)
    corpus = json.loads((ROOT / "experiments/deep_combs/depth-4.corpus.json").read_text())
    witnesses = {}
    for e in corpus["entries"]:
        e["program"], witnesses[e["class"]] = convert(e["program"])
    corpus["schema"] = "root-interface-addresses-v1"
    corpus["scope"] = "reversible relative coordinates; types, local wiring, aliasing and guards retained; not graph equivalence inference"
    (OUT / "corpus.json").write_text(json.dumps(corpus, separators=(",", ":")))
    (OUT / "renaming.json").write_text(json.dumps(witnesses, separators=(",", ":")))
    subprocess.run(["cargo", "build", "--release", "--locked", "--manifest-path", "tools/babble_adapter/Cargo.toml"], cwd=ROOT, check=True)
    subprocess.run([str(ROOT / "tools/babble_adapter/target/release/combine-babble"), str(OUT / "corpus.json"), str(OUT / "learning.json"), "comb-mining"], check=True, timeout=180)

if __name__ == "__main__":
    main()
