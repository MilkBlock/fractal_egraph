#!/usr/bin/env python3
"""What do union-created connections between RipSat cells actually look like?

Mines the observer catalogs written by the cell-evolution experiment
(`EGG_LAYOUT_CELL_EVOLUTION=1`, run by tools/bench/cell_evolution.py) and the
per-round `progress.json` logs. It does not run the engine and does not change
any rule; everything below is read from an existing observed run of
`experiments/math_mini3/source.egg` (6 rounds) and four toy fixtures.

Definitions used here (from src/reuse_evolution.rs):
  * support_cells  -- cells whose facts or input ports were read as witnesses of
                      one match (incidence, not a minimal causal set)
  * touched_slots  -- source-cell interface slots pinned by reads that are not
                      body facts of that cell
  * source_support -- "body_read" if the source cell provided a read fact,
                      "input_refinement" if only its RipenInput output was read
  * committed_math_rows_postcanonical -- committed Math rows, "inserted" (fresh)
                      or "updated" (already present, re-canonicalized)
  * traced_read_value_unions -- unions the trace could tie to this match between
                      two pre-existing named ports with a displaced value
  * loose_interface_contacts_not_expanded -- shared-Math-value contacts that were
                      NOT expanded into a boundary event
"""
import argparse
import collections
import json
import statistics
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CASES = ["boundary", "refinement", "multi_parent", "unrelated", "math"]
SCALARS = [
    "logical_matches",
    "boundary_events",
    "boundary_linked_births",
    "observed_instance_births",
    "canonical_instances",
    "coalesced_cells",
    "observed_slot_partition_changes",
    "multi_support_events",
    "loose_interface_contacts_not_expanded",
    "incomplete_matches",
    "transition_patterns",
    "patterns_repeated_across_instances",
    "families",
]


def load(root, case):
    return json.loads((root / f"{case}-observe/cell-evolution.json").read_text())


def quantiles(values):
    v = sorted(values)
    if not v:
        return {}
    return {
        "min": v[0],
        "p25": v[len(v) // 4],
        "median": statistics.median(v),
        "p75": v[3 * len(v) // 4],
        "p90": v[int(0.9 * len(v))],
        "max": v[-1],
    }


def channel_stats(patterns, channel):
    ps = [p for p in patterns if p["transition"]["source_support"] == channel]
    ext = [sum(1 for r in p["transition"]["read_guard"] if r["location"] == "external_fact") for p in ps]
    body = [sum(1 for r in p["transition"]["read_guard"] if r["location"] == "body_fact") for p in ps]
    occ = sum(p["occurrences"] for p in ps)
    births = sum(p["new_target_occurrences"] for p in ps)
    commits = collections.Counter(
        r["outcome"] for p in ps for r in p["transition"]["committed_math_rows_postcanonical"]
    )
    with_updated = sum(
        1
        for p in ps
        if any(r["outcome"] == "updated" for r in p["transition"]["committed_math_rows_postcanonical"])
    )
    return {
        "patterns": len(ps),
        "occurrences": occ,
        "new_target_occurrences": births,
        "births_per_occurrence": births / occ if occ else None,
        "median_external_reads": statistics.median(ext) if ext else None,
        "median_body_reads": statistics.median(body) if body else None,
        "median_support_cells": statistics.median([len(p["example"]["support_cells"]) for p in ps]),
        "median_touched_slots": statistics.median([len(p["transition"]["touched_slots"]) for p in ps]),
        "patterns_committing_an_update": with_updated,
        "committed_rows": dict(commits),
    }


def fanin(patterns):
    """How many other cells share one interface value of the perturbed cell."""
    rows = []
    for p in patterns:
        ex = p["example"]
        src = ex["source_cell"]
        fan = collections.Counter()
        others = 0
        for sp in ex["support_ports"]:
            if sp["cell"] == src:
                continue
            others += 1
            for _slot, symbol in sp["known_slots"]:
                fan[symbol] += 1
        if not others:
            continue
        top = fan.most_common(1)[0]
        rows.append({"other_cells": others, "top_symbol": top[0], "top_fan_in": top[1], "refs": sum(fan.values())})
    return {
        "patterns_with_other_cells": len(rows),
        "other_cells": quantiles([r["other_cells"] for r in rows]),
        "top_fan_in": quantiles([r["top_fan_in"] for r in rows]),
        "patterns_with_one_value_shared_by_more_than_200_cells": sum(1 for r in rows if r["top_fan_in"] > 200),
        "max_examples": sorted(rows, key=lambda r: -r["top_fan_in"])[:3],
    }


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--out", type=Path, default=ROOT / "out/cell-union-observation")
    p.add_argument("--cases-root", type=Path, default=ROOT / "out/cell-evolution-complete")
    a = p.parse_args()
    a.out.mkdir(parents=True, exist_ok=True)

    report = {"cases_root": str(a.cases_root), "per_case": {}, "math": {}}
    for case in CASES:
        cat = load(a.cases_root, case)
        report["per_case"][case] = {k: cat.get(k) for k in SCALARS}
        report["per_case"][case]["families"] = len(cat.get("families") or [])
        prog = a.cases_root / f"{case}-observe/progress.json"
        if prog.exists():
            report["per_case"][case]["rounds"] = [
                {
                    "round": r["round"],
                    "matches": r["matches"],
                    "enodes": r["graph"]["enodes"],
                    "eclasses": r["graph"]["eclasses"],
                    "seconds": r["execution_seconds"],
                    "us_per_match": 1e6 * r["execution_seconds"] / r["matches"] if r["matches"] else None,
                }
                for r in json.loads(prog.read_text())
            ]

    patterns = load(a.cases_root, "math")["patterns"]
    total_occ = sum(p["occurrences"] for p in patterns)
    total_births = sum(p["new_target_occurrences"] for p in patterns)

    buckets = collections.defaultdict(lambda: {"patterns": 0, "occurrences": 0, "births": 0})

    def bucket(n):
        b = 1
        while b < n:
            b *= 2
        return b

    for p in patterns:
        b = buckets[bucket(len(p["example"]["support_cells"]))]
        b["patterns"] += 1
        b["occurrences"] += p["occurrences"]
        b["births"] += p["new_target_occurrences"]

    report["math"] = {
        "patterns": len(patterns),
        "occurrences": total_occ,
        "new_target_occurrences": total_births,
        "births_per_occurrence": total_births / total_occ,
        "touched_slots": dict(sorted(collections.Counter(len(p["transition"]["touched_slots"]) for p in patterns).items())),
        "traced_read_value_unions_total": sum(len(p["transition"]["traced_read_value_unions"]) for p in patterns),
        "patterns_with_alias_change": sum(
            1 for p in patterns if p["transition"]["before_aliases"] != p["transition"]["after_aliases"]
        ),
        "channels": {c: channel_stats(patterns, c) for c in ("body_read", "input_refinement")},
        "fan_in": fanin(patterns),
        "fanout_buckets": [
            {"support_cells_at_most": k, **v, "occurrence_share": v["occurrences"] / total_occ}
            for k, v in sorted(buckets.items())
        ],
    }
    (a.out / "results.json").write_text(json.dumps(report, indent=2) + "\n")

    for case, s in report["per_case"].items():
        print(
            f"{case:<12} matches={s['logical_matches']:>5} boundary_events={s['boundary_events']:>5} "
            f"births={s['observed_instance_births']:>5} loose_contacts={s['loose_interface_contacts_not_expanded']:>8} "
            f"patterns={s['transition_patterns']:>5} families={s['families']:>3}"
        )
    m = report["math"]
    print(
        f"\nmath: {m['patterns']} patterns / {m['occurrences']:,} occurrences / "
        f"births per occurrence {m['births_per_occurrence']:.3f}"
    )
    print(f"  touched slots: {m['touched_slots']}")
    print(f"  attributable read-value unions: {m['traced_read_value_unions_total']}; "
          f"patterns with alias change: {m['patterns_with_alias_change']}")
    for c, s in m["channels"].items():
        print(f"  {c:<17} patterns={s['patterns']:>5} occ={s['occurrences']:>6,} "
              f"births/occ={s['births_per_occurrence']:.3f} med(body,ext)=({s['median_body_reads']},{s['median_external_reads']}) "
              f"med_support={s['median_support_cells']} rows={s['committed_rows']}")
    f = m["fan_in"]
    print(f"  fan-in: other cells {f['other_cells']}; top value fan-in {f['top_fan_in']}; "
          f">200-cell hubs {f['patterns_with_one_value_shared_by_more_than_200_cells']}")
    print(f"  bucket share of occurrences: " + ", ".join(
        f"<={b['support_cells_at_most']}:{100 * b['occurrence_share']:.0f}%" for b in m["fanout_buckets"]))
    print(f"wrote {a.out / 'results.json'}")


if __name__ == "__main__":
    main()
