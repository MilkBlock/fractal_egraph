"""Read completed real eggcc Cells and prepare native-parser-only probes.

This module never runs egglog, invents Cell inputs, or upgrades an observed
snapshot to a certified transition. Root candidates come from the recorded
injection immediately before the root member, not a term-size heuristic.
"""

from copy import deepcopy
import hashlib
import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
DEFAULT_CELLS = (
    ROOT / "out/eggcc-full-replay-20261004-stack16m/run"
    / "saturated-rule-composition/cells"
)
FAMILY = (1729, 1730, 1865, 1893, 1916)


def load(cell_id, cells_dir=DEFAULT_CELLS):
    """Load one saved Cell, rejecting incomplete or inconsistent evidence.

The return value intentionally retains the full original ``origin`` and
``snapshot``. Use :func:`compact_evidence` for a small serializable report.
Hashes cover the actual input bytes, including the native replay source.
"""
    if isinstance(cell_id, bool) or not isinstance(cell_id, int) or cell_id < 0:
        raise ValueError("cell_id must be a nonnegative integer")
    folder = Path(cells_dir).resolve() / f"CSUnit-{cell_id:06d}"
    filenames = {
        "ripen": "ripen.json",
        "snapshot": "observed-saturated-state.json",
        "trigger": "trigger-state.json",
        "entry": "entry.egg",
        "ripened": "ripened.egg",
    }
    paths = {name: folder / filename for name, filename in filenames.items()}
    raw = {name: path.read_bytes() for name, path in paths.items()}
    report = json.loads(raw["ripen"])
    snapshot = json.loads(raw["snapshot"])
    origin = report["origin"]
    ripen = report["ripen"]

    if origin.get("candidate_id") != cell_id:
        raise ValueError(f"Cell {cell_id}: candidate ID disagrees with directory")
    if ripen.get("state") != "Saturated" or ripen.get("updated") is not False:
        raise ValueError(f"Cell {cell_id}: no completed unchanged native closure")
    if report.get("checks") != "passed":
        raise ValueError(f"Cell {cell_id}: source replay checks did not pass")
    if "native_stop_reason" not in report or report["native_stop_reason"] is not None:
        raise ValueError(f"Cell {cell_id}: native execution stopped before completion")
    if snapshot.get("version") != 2:
        raise ValueError(f"Cell {cell_id}: unsupported saved snapshot version")

    export = report["saturated_rule_composition"]
    observed = export.get("observed_state", {})
    if observed.get("file") != filenames["snapshot"]:
        raise ValueError(f"Cell {cell_id}: export does not identify the loaded snapshot")
    if observed.get("certified") is not False:
        raise ValueError(f"Cell {cell_id}: expected explicitly uncertified observed export")
    if snapshot.get("scope", {}).get("observation_only") is not True:
        raise ValueError(f"Cell {cell_id}: expected explicitly observation-only snapshot")
    for key, collection in (("facts", "rows"), ("values", "values")):
        if observed.get(key) != len(snapshot[collection]):
            raise ValueError(f"Cell {cell_id}: exported {key} count disagrees with snapshot")

    entry_source = raw["entry"].decode("utf-8")
    if report.get("source_text") != entry_source:
        raise ValueError(f"Cell {cell_id}: entry.egg differs from recorded replay source")
    root_member = origin["root_member"]
    stages = origin["staged_injections"]
    root_stages = [stage for stage in stages if stage["before_record"] == root_member]
    if len(root_stages) != 1 or stages[-1] != root_stages[0]:
        raise ValueError(f"Cell {cell_id}: root member has no unique final injection")
    if not root_stages[0]["facts"]:
        raise ValueError(f"Cell {cell_id}: root injection contains no candidate facts")
    typed_tokens = [(value["sort"], value["token"]) for value in origin["symbolic_values"]]
    if len(set(typed_tokens)) != len(typed_tokens):
        raise ValueError(f"Cell {cell_id}: duplicate typed symbolic token")

    return {
        "cell_id": cell_id,
        "folder": str(folder),
        "paths": {name: str(path) for name, path in paths.items()},
        "source_hashes": {
            name: hashlib.sha256(content).hexdigest() for name, content in raw.items()
        },
        "entry_source": entry_source,
        "origin": origin,
        "snapshot": snapshot,
        "root_stage": root_stages[0],
        "rulesets": sorted(ripen["rulesets"]),
        "status": {
            "state": ripen["state"],
            "updated": ripen["updated"],
            "checks": report["checks"],
            "checks_count": report["checks_count"],
            "native_stop_reason": report["native_stop_reason"],
            "export": deepcopy(export),
            "observation_only": snapshot["scope"]["observation_only"],
            "semantics": snapshot["scope"]["semantics"],
            "merge_contract": snapshot["scope"].get("merge_contract"),
            "symbolic_boundary": snapshot["scope"].get("symbolic_boundary"),
        },
    }


def prepare_probe_source(cell):
    """Return ``(source, name_to_evidence)`` for egglog's ``lhs_ast`` exporter.

    Append parse-only lets for every recorded symbolic value and every exact
    root-stage fact. These are parser probes, never executable initialization.
    The caller uses native AST signatures to select constructor expressions
    returning Expr among the ``candidate_fact`` probes, and resolves their
    typed values against snapshot rows. Unsupported/non-expression facts must
    fail that selection; this module does not parse or guess their meaning.
    """
    source = cell["entry_source"]
    bindings = {}
    probes = []
    for index, value in enumerate(cell["origin"]["symbolic_values"]):
        name = f"audit_value_{index}"
        bindings[name] = {"kind": "typed_token", **deepcopy(value)}
        probes.append((name, value["term"]))
    for index, fact in enumerate(cell["root_stage"]["facts"]):
        name = f"audit_root_{index}"
        bindings[name] = {
            "kind": "candidate_fact",
            "before_record": cell["root_stage"]["before_record"],
            "fact_index": index,
            "term": fact,
        }
        probes.append((name, fact))
    for name, _ in probes:
        if name in source:
            raise ValueError(f"Cell {cell['cell_id']}: parser probe name already occurs: {name}")
    suffix = "\n; Native parser probes only; do not execute these commands.\n"
    suffix += "\n".join(f"(let {name} {term})" for name, term in probes) + "\n"
    return source + suffix, bindings


def compact_evidence(cell):
    """Return provenance and typed bindings without repeating the full ruleset."""
    origin = cell["origin"]
    member_fields = (
        "slot", "record", "event", "parents", "external_parents", "rule_name",
        "input_roles", "output_roles", "aliases", "binding",
    )
    return {
        "cell_id": cell["cell_id"],
        "paths": deepcopy(cell["paths"]),
        "source_hashes": deepcopy(cell["source_hashes"]),
        "status": deepcopy(cell["status"]),
        "rulesets": list(cell["rulesets"]),
        "parents": list(origin["composition"]["parts"]),
        "composition": deepcopy(origin["composition"]),
        "root_member": origin["root_member"],
        "members": list(origin["members"]),
        "comb_members": [
            {key: deepcopy(member[key]) for key in member_fields if key in member}
            for member in origin["comb_members"]
        ],
        "parameters": deepcopy(origin["parameters"]),
        "typed_tokens": deepcopy(origin["symbolic_values"]),
        "root_stage": deepcopy(cell["root_stage"]),
        "set_update_witnesses": deepcopy(origin.get("set_update_witnesses", [])),
        "snapshot_counts": {
            "values": len(cell["snapshot"]["values"]),
            "rows": len(cell["snapshot"]["rows"]),
        },
    }
