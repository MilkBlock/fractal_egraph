"""Provenance-preserving access to saved, real structural Math Cells.

Only the native ``lhs_ast`` exporter parses egglog. Probe commands are parser
input, never runtime initialization. In particular, staged union actions remain
whole actions: their constructor arguments must not lose materialization.
"""

from copy import deepcopy
import hashlib
import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
DEFAULT_CELLS = (
    ROOT / "out/observed-transition-math3-verified/events"
    / "saturated-rule-composition/cells"
)
FAMILY = (8, 10, 15, 36, 59)


def load(cell_id, cells_dir=DEFAULT_CELLS):
    """Load a complete exported Cell without changing its closure claim."""
    if isinstance(cell_id, bool) or not isinstance(cell_id, int) or cell_id < 0:
        raise ValueError("cell_id must be a nonnegative integer")
    folder = Path(cells_dir).resolve() / f"CSUnit-{cell_id:06d}"
    names = {
        "ripen": "ripen.json", "snapshot": "saturated-rule-composition.json",
        "entry": "entry.egg", "source": "source.egg",
    }
    missing_files = []
    if (folder / "ripened.egg").is_file():
        names["ripened"] = "ripened.egg"
    else:
        missing_files.append("ripened.egg")
    paths = {key: folder / name for key, name in names.items()}
    raw = {key: path.read_bytes() for key, path in paths.items()}
    report = json.loads(raw["ripen"])
    snapshot = json.loads(raw["snapshot"])
    origin, ripen = report["origin"], report["ripen"]
    if origin.get("candidate_id") != cell_id:
        raise ValueError("Cell ID does not match its saved origin")
    if ripen.get("state") != "Saturated" or ripen.get("updated") is not False:
        raise ValueError("Cell has no completed unchanged native closure")
    if report.get("checks") != "passed" or report.get("native_stop_reason") is not None:
        raise ValueError("Cell replay failed or native execution stopped")
    export = report["saturated_rule_composition"]
    if export.get("status") != "exported":
        raise ValueError("Cell has no complete supported-state export")
    if snapshot.get("version") != 2:
        raise ValueError("unsupported snapshot version")
    if snapshot["scope"].get("semantics") != "positive-constructor-equality/v1":
        raise ValueError("expected the saved positive constructor/equality contract")
    if snapshot["scope"].get("observation_only") is True:
        raise ValueError("projected observation cannot stand in for complete exported state")
    if export["facts"] != len(snapshot["rows"]) or export["values"] != len(snapshot["values"]):
        raise ValueError("export counts disagree with saved state")
    entry = raw["entry"].decode("utf-8")
    if report["source_text"] != entry:
        raise ValueError("entry source differs from the source recorded by replay")
    root_record = origin["root_member"]
    root_members = [member for member in origin["comb_members"] if member["record"] == root_record]
    if len(root_members) != 1:
        raise ValueError("observed root must identify exactly one recorded member")
    root_member = root_members[0]
    return {
        "cell_id": cell_id, "folder": str(folder),
        "paths": {key: str(path) for key, path in paths.items()},
        "missing_files": missing_files,
        "source_hashes": {key: hashlib.sha256(data).hexdigest() for key, data in raw.items()},
        "entry_source": entry, "origin": origin, "snapshot": snapshot,
        "rulesets": sorted(ripen["rulesets"]),
        "root_record": root_record, "root_member_index": root_member["slot"],
        "root_member": root_member,
        "status": {
            "state": ripen["state"], "updated": ripen["updated"],
            "checks": report["checks"], "checks_count": report["checks_count"],
            "native_stop_reason": report["native_stop_reason"], "export": deepcopy(export),
            "semantics": snapshot["scope"]["semantics"],
            "observation_only": snapshot["scope"].get("observation_only"),
            "symbolic_boundary": snapshot["scope"].get("symbolic_boundary"),
            "scope": "saved native closure; no new compiled transition certification implied",
        },
    }


def load_family(cells_dir=DEFAULT_CELLS):
    cells = {cell_id: load(cell_id, cells_dir) for cell_id in FAMILY}
    for cell in cells.values():
        if not set(cell["origin"]["composition"]["parts"]) <= set(cells):
            raise ValueError("selected observed family omits a recorded parent")
    return cells


def prepare_probe_source(cell):
    """Return source and probe metadata for a parser-only native AST export.

    Typed terms use lets. Every staged fact remains its original top-level
    command between named sentinels. Even an unconnected union keeps both of
    its argument expressions intact. Stage requirements remain in metadata.
    """
    source = cell["entry_source"]
    probes = {"typed_tokens": {}, "stages": []}
    commands = []
    prefix = f"structural_audit_{cell['cell_id']}_"
    if prefix in source:
        raise ValueError("parser probe prefix already occurs in saved source")
    for index, value in enumerate(cell["origin"]["symbolic_values"]):
        name = f"{prefix}value_{index}"
        probes["typed_tokens"][name] = deepcopy(value)
        commands.append(f"(let {name} {value['term']})")
    for index, stage in enumerate(cell["origin"]["staged_injections"]):
        begin, end = f"{prefix}stage_{index}_begin", f"{prefix}stage_{index}_end"
        probes["stages"].append({
            "stage_index": index, "begin": begin, "end": end, **deepcopy(stage),
        })
        commands.extend([f"(let {begin} 0)", *stage["facts"], f"(let {end} 0)"])
    return source + "\n; Native parser probes only; never execute this suffix.\n" + "\n".join(commands) + "\n", probes


def parsed_probes(native_ast, probes):
    """Recover typed-term and staged-action ASTs solely from native output."""
    if native_ast.get("format") != "egglog-lhs-ast/v1" or native_ast.get("parse_only") is not True:
        raise ValueError("native parser-only AST required")
    actions = native_ast["actions"]
    lets = {item["action"]["name"]: item for item in actions if item["action"]["kind"] == "let"}
    typed = [
        {**deepcopy(value), "probe": name, "expr": deepcopy(lets[name]["action"]["expr"])}
        for name, value in probes["typed_tokens"].items()
    ]
    stages = []
    for stage in probes["stages"]:
        low = lets[stage["begin"]]["command_ordinal"]
        high = lets[stage["end"]]["command_ordinal"]
        selected = [item for item in actions if low < item["command_ordinal"] < high]
        if high - low - 1 != len(stage["facts"]) or len(selected) != len(stage["facts"]):
            raise ValueError("staged fact is not exactly one native top-level action")
        stages.append({
            **deepcopy(stage),
            "actions": [
                {"source": source, **deepcopy(item)}
                for source, item in zip(stage["facts"], selected)
            ],
        })
    return {"typed_tokens": typed, "stages": stages}


def environment_source(cell, native_ast):
    """Keep the actual declaration and all original rules, excluding replay.

    This saved family has exactly one Math datatype followed by its source
    rules. Native command ordinals validate that layout; native byte spans
    delimit the rules. No line splitting, S-expression parser, or constructor
    schema approximation is used to extract the environment.
    """
    if native_ast.get("format") != "egglog-lhs-ast/v1" or native_ast.get("parse_only") is not True:
        raise ValueError("native parser-only AST required")
    declarations = native_ast["declarations"]
    if len(declarations) != 1 or declarations[0].get("kind") != "datatype" or declarations[0].get("name") != "Math":
        raise ValueError("environment extraction supports this saved Math datatype layout only")
    rules = sorted(native_ast["rules"], key=lambda rule: rule["command_ordinal"])
    if len(rules) != cell["origin"]["all_source_rules"]:
        raise ValueError("native AST does not contain the recorded whole rule environment")
    if [rule["command_ordinal"] for rule in rules] != list(range(1, len(rules) + 1)):
        raise ValueError("source rules are interleaved with initialization commands")
    if any(command["command_ordinal"] < len(rules) + 1 for command in native_ast["other_commands"]):
        raise ValueError("unexpected non-rule command inside environment prefix")
    data = cell["entry_source"].encode("utf-8")
    first = rules[0]["source"]["byte_start"]
    texts = []
    for rule in rules:
        span = rule["source"]
        text = data[span["byte_start"]:span["byte_end"]].decode("utf-8")
        if text != span["text"]:
            raise ValueError("AST source span differs from hashed entry source")
        texts.append(text)
    return data[:first].decode("utf-8").rstrip() + "\n" + "\n".join(texts) + "\n"


def compact_evidence(cell):
    origin = cell["origin"]
    return {
        "cell_id": cell["cell_id"], "paths": deepcopy(cell["paths"]),
        "missing_files": list(cell["missing_files"]),
        "source_hashes": deepcopy(cell["source_hashes"]), "status": deepcopy(cell["status"]),
        "rulesets": list(cell["rulesets"]), "composition": deepcopy(origin["composition"]),
        "root_record": cell["root_record"], "root_member_index": cell["root_member_index"],
        "root_rule_name": cell["root_member"]["rule_name"],
        "root_rule_source": cell["root_member"]["rule"],
        "members": list(origin["members"]), "comb_members": deepcopy(origin["comb_members"]),
        "parameters": deepcopy(origin["parameters"]), "typed_tokens": deepcopy(origin["symbolic_values"]),
        "staged_injections": deepcopy(origin["staged_injections"]),
        "reuse_recipe": deepcopy(origin["reuse_recipe"]),
        "snapshot_counts": {"values": len(cell["snapshot"]["values"]), "rows": len(cell["snapshot"]["rows"])},
    }
