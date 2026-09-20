#!/usr/bin/env python3
"""Go/no-go probe for the "pack a discovered composition into an opaque
constructor" direction.

Question this answers
---------------------
For the packing scheme to be sound without per-template induction, a composite's
interior members must not be *observable* by anything outside the composite. If
an interior member's output is read by an external consumer, that consumer either
has to be rewritten onto a projection (a whole-program change) or it silently
loses its match (a semantics change).

So: for every active Use, does any consumer *outside* the Use read a non-root
member's output?

Definitions (encoded to match comb_reuse::push, native_analyze.rs side is irrelevant here)
-----------------------------------------------------------------------------------------
owner[event] = (use, member)      reconstructed from the active cover in `roots`
read(u, m, o) exists              residual j has a binding slot referencing
                                  ResidualPort{event: p, output: o} with owner[p] = (u, m)
external read                     j is not a member of u
interior read                     m != pattern.root of u's template
closed(u)                         u has no external interior read

Caveat that must travel with every number below: closing at the *history* level is
necessary but NOT sufficient for the rule-level "dead intermediate" property. It
only says no observed consumer read it. It does not say no rule *could* match it.

Input is read-only; nothing is executed.
"""

import argparse
import json
from collections import Counter, defaultdict
from pathlib import Path

DEFAULT_ANALYSIS = Path(
    "/Users/mineralsteins/Repos/egg_related/egg_layout"
    "/out/adaptive-reuse-final/full/analysis.json"
)


def parse_ref(ref):
    """Return ('port', event, output) | ('other', kind) for one binding slot."""
    if not isinstance(ref, dict) or len(ref) != 1:
        return ("other", "malformed")
    kind, payload = next(iter(ref.items()))
    if kind == "ResidualPort":
        return ("port", payload["event"], payload["output"])
    return ("other", kind)


def load(analysis_path):
    doc = json.loads(Path(analysis_path).read_text())
    layers = doc["layers"]
    reuse = layers["reuse"]
    return doc, layers, reuse


def build_owner(reuse):
    """Reconstruct the active cover: event -> (use, member_index)."""
    active = [entry["Use"] for entry in reuse["roots"] if "Use" in entry]
    owner = {}
    conflicts = []
    for u in active:
        for member_index, event in enumerate(reuse["uses"][u]["members"]):
            if event in owner and owner[event][0] != u:
                conflicts.append((event, owner[event], (u, member_index)))
            owner[event] = (u, member_index)
    return active, owner, conflicts


def root_of(reuse, u):
    template = reuse["uses"][u]["template"]
    return reuse["templates"][template]["pattern"]["root"]


def collect_reads(reuse, active, owner):
    """Every external read that passes through an active Use."""
    members_of = {u: set(reuse["uses"][u]["members"]) for u in active}
    roots = {u: root_of(reuse, u) for u in active}

    reads = []  # (use, member, output, consumer_kind, consumer_id)
    skipped_internal = 0
    unresolved = 0

    for j, residual in enumerate(reuse["residuals"]):
        for slot, ref in enumerate(residual["binding"]):
            kind, *rest = parse_ref(ref)
            if kind != "port":
                continue
            _, event, output = parse_ref(ref)
            hit = owner.get(event)
            if hit is None:
                unresolved += 1
                continue
            u, member = hit
            if j in members_of[u]:
                # An edge inside the composite. comb_reuse::push never counts these
                # through a Use, because the Use did not exist when j was pushed.
                skipped_internal += 1
                continue
            reads.append((u, member, output, "residual", j))

    return reads, roots, skipped_internal, unresolved


def summarize(reuse, active, reads, roots):
    per_use = defaultdict(set)
    for u, member, output, _, consumer in reads:
        per_use[u].add((member, output))

    interior_per_use = {}
    for u in active:
        interior = {p for p in per_use[u] if p[0] != roots[u]}
        interior_per_use[u] = interior

    by_template = defaultdict(
        lambda: {
            "active_uses": 0,
            "closed_uses": 0,
            "member_counts": Counter(),
            "interior_ports": set(),
            "reads": 0,
        }
    )
    for u in active:
        template = reuse["uses"][u]["template"]
        bucket = by_template[template]
        bucket["active_uses"] += 1
        bucket["member_counts"][len(reuse["uses"][u]["members"])] += 1
        bucket["interior_ports"] |= interior_per_use[u]
        bucket["reads"] += len(interior_per_use[u])
        if not interior_per_use[u]:
            bucket["closed_uses"] += 1

    histogram = Counter(len(interior_per_use[u]) for u in active)
    return interior_per_use, by_template, histogram


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--analysis", type=Path, default=DEFAULT_ANALYSIS)
    parser.add_argument("--json", type=Path, default=None,
                        help="write machine-readable results here")
    args = parser.parse_args()

    doc, layers, reuse = load(args.analysis)
    stats = reuse["stats"]
    active, owner, conflicts = build_owner(reuse)
    reads, roots, skipped, unresolved = collect_reads(reuse, active, owner)
    interior_per_use, by_template, histogram = summarize(reuse, active, reads, roots)

    total_reads = len(reads)
    total_interior = sum(len(v) for v in interior_per_use.values())
    closed = [u for u in active if not interior_per_use[u]]

    print(f"analysis            {args.analysis}")
    print(f"events              {stats['events']}")
    print(f"active Uses         {len(active)}")
    print(f"templates (all)     {stats['templates']}   used {stats['used_templates']}")
    print(f"cover conflicts     {len(conflicts)}  (must be 0)")
    print(f"reads to plain residuals {unresolved}  (producer is not inside any active Use)")
    print(f"internal edges      {skipped}  (excluded: inside the composite)")
    print()
    print("--- cross-check against the runtime counters ---")
    print(f"external reads through a Use   probe {total_reads:>6}   "
          f"stats.continuations_through_use {stats['continuations_through_use']:>6}")
    print(f"  of which interior            probe {total_interior:>6}   "
          f"stats.interior_port_reads      {stats['interior_port_reads']:>6}")
    print("  (probe counts the *final* cover; the runtime counter accumulates as the")
    print("   cover changes, and counts Use inputs/contexts too, so exact equality is")
    print("   not expected -- same order of magnitude is the sanity condition.)")
    print()

    print("--- interior external reads per active Use ---")
    for size in sorted(histogram):
        label = "closed" if size == 0 else str(size)
        print(f"  {label:>7} ports : {histogram[size]:>4} Uses")
    print()

    closed_templates = []
    for template, bucket in sorted(by_template.items()):
        if bucket["active_uses"] >= 2 and bucket["closed_uses"] == bucket["active_uses"]:
            closed_templates.append((template, bucket))

    packable_uses = len(closed)
    print("--- headline ---")
    print(f"closed Uses          {packable_uses} / {len(active)} "
          f"({100.0 * packable_uses / max(len(active), 1):.1f}%)")
    print(f"closed templates     {len(closed_templates)} / {len(by_template)} "
          f"(active Uses >= 2 and every instance closed)")
    print(f"interior ext. reads  {total_interior} / {total_reads} "
          f"({100.0 * total_interior / max(total_reads, 1):.1f}% of reads through a Use)")
    print()

    port_histogram = Counter(len(b["interior_ports"]) for b in by_template.values())
    print("--- Q1 verdict: composition with no projections (dead intermediate) ---")
    if closed_templates:
        print("  candidates: " + ", ".join(f"T{t} ({b['active_uses']} uses)"
                                          for t, b in closed_templates))
    else:
        print("  NONE. No template has all of its active instances interior-closed, so the")
        print("  property never holds at the granularity where rules live. Closed *instances*")
        print("  exist but are scattered across templates that are otherwise open.")
    print()

    print("--- Q2: projection surface per template (distinct interior ports read) ---")
    for size in sorted(port_histogram):
        print(f"  {size:>3} ports : {port_histogram[size]:>3} templates")
    print()

    print("--- ranked candidates: by active uses, then by projection surface ---")
    ranked = sorted(by_template.items(),
                    key=lambda kv: (-kv[1]["active_uses"], len(kv[1]["interior_ports"])))
    header2 = f"{'T':>5} {'uses':>5} {'closed':>6} {'open%':>6} {'members':>8} {'proj':>5}"
    print(header2)
    print("-" * len(header2))
    for template, bucket in ranked[:15]:
        members = bucket["member_counts"].most_common(1)[0][0]
        open_pct = 100.0 * (bucket["active_uses"] - bucket["closed_uses"]) / bucket["active_uses"]
        print(f"{template:>5} {bucket['active_uses']:>5} {bucket['closed_uses']:>6} "
              f"{open_pct:>5.0f}% {members:>8} {len(bucket['interior_ports']):>5}")
    print()

    print("--- top templates by template id ---")
    header = f"{'T':>5} {'uses':>5} {'closed':>6} {'members':>8} {'int.ports':>9} {'reads':>6}"
    print(header)
    print("-" * len(header))
    for template, bucket in sorted(by_template.items()):
        members = bucket["member_counts"].most_common(1)[0][0]
        print(f"{template:>5} {bucket['active_uses']:>5} {bucket['closed_uses']:>6} "
              f"{members:>8} {len(bucket['interior_ports']):>9} {bucket['reads']:>6}")

    if args.json:
        payload = {
            "scope": (
                "Read-only probe over one recorded analysis. 'closed' means no observed "
                "external consumer read a non-root member output. It does NOT prove that no "
                "rule could match the interior: history coverage is not a rule-level "
                "domination property."
            ),
            "analysis": str(args.analysis),
            "counters": {
                "events": stats["events"],
                "active_uses": len(active),
                "templates": stats["templates"],
                "used_templates": stats["used_templates"],
                "probe_external_reads": total_reads,
                "probe_interior_reads": total_interior,
                "runtime_continuations_through_use": stats["continuations_through_use"],
                "runtime_interior_port_reads": stats["interior_port_reads"],
                "cover_conflicts": len(conflicts),
                "unresolved_refs": unresolved,
                "excluded_internal_edges": skipped,
            },
            "histogram_interior_ports_per_use": {str(k): v for k, v in sorted(histogram.items())},
            "histogram_interior_ports_per_template": {str(k): v for k, v in sorted(port_histogram.items())},
            "closed_uses": packable_uses,
            "closed_templates": [t for t, _ in closed_templates],
            "q1_verdict": (
                "no template qualifies" if not closed_templates
                else "candidates: " + ",".join(str(t) for t, _ in closed_templates)
            ),
            "per_template": {
                str(t): {
                    "active_uses": b["active_uses"],
                    "closed_uses": b["closed_uses"],
                    "members": dict(b["member_counts"]),
                    "interior_ports": len(b["interior_ports"]),
                    "interior_reads": b["reads"],
                }
                for t, b in sorted(by_template.items())
            },
        }
        args.json.parent.mkdir(parents=True, exist_ok=True)
        args.json.write_text(json.dumps(payload, indent=2) + "\n")
        print(f"\nwrote {args.json}")


if __name__ == "__main__":
    main()
