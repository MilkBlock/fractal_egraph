#!/usr/bin/env python3
"""Compare preserved dense/sparse runs without hiding bootstrap or audit costs."""
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent


def read(name):
    return json.loads((ROOT / "results" / name).read_text())


def main():
    dense, sparse = read("dense-time.json"), read("sparse-time.json")
    profile, audit = read("sparse-profile.json"), read("sparse-check.json")
    old_profile = json.loads((ROOT.parent / "bake_quality/results/profile.json").read_text())
    assert dense["instances_per_family"] == sparse["instances_per_family"] == 512
    assert profile["instances_per_family"] == old_profile["instances_per_family"] == 128
    assert audit["mode"] == "check" and audit["instances_per_family"] == 32
    rows, startup = [], []
    assert all(len(v["variants"]) == 5 for v in (dense, sparse, profile, old_profile, audit))
    for before, after, prof, old_prof, checked in zip(
        dense["variants"], sparse["variants"], profile["variants"],
        old_profile["variants"], audit["variants"]
    ):
        assert len({v["library"] for v in (before, after, prof, old_prof, checked)}) == 1
        for field in ("training_contexts", "distinct_states", "body_rows"):
            assert before["bake"][field] == after["bake"][field]
        bd, ad = before["report"]["device"], after["report"]["device"]
        for field in ("cell_plans", "row_slots", "row_capacity", "bindings", "events"):
            assert bd[field] == ad[field], (before["library"], field)
        for phase in ("intact", "disrupted"):
            assert before[phase]["verification"] == after[phase]["verification"]
            assert checked[phase]["verification"]["exact_rows_values_equivalence_visibility"]
            for field in (
                "routed_ready", "routed_suspected", "routed_unmapped", "routing_valid_seeds",
                "ready_covered_atom_sum", "ready_query_atom_sum", "ready_matches",
                "suspected_matches", "unmapped_matches", "ready_reused_atoms",
                "skipped_join_probes", "join_probes", "routing_locator_probes",
            ):
                assert prof[phase]["work"][field] == old_prof[phase]["work"][field], (phase, field)
        def ms(v, phase):
            return v[phase]["runtime"]["run_seconds"] * 1000
        def total(v):
            return v["report"]["runtime"]["run_seconds"] * 1000
        def setup(v):
            return v["report"]["runtime"]["setup_seconds"] * 1000
        rows.append(f'| {before["library"]} | {bd["cell_plans"]} | '
                    f'{bd["device_bytes"]/2**20:.2f} → {ad["device_bytes"]/2**20:.2f} | '
                    f'{ms(before,"intact"):.3f} → {ms(after,"intact"):.3f} | '
                    f'{ms(before,"disrupted"):.3f} → {ms(after,"disrupted"):.3f} |')
        startup.append(f'| {before["library"]} | {setup(before):.3f} → {setup(after):.3f} | '
                       f'{total(before)-ms(before,"intact")-ms(before,"disrupted"):.3f} → '
                       f'{total(after)-ms(after,"intact")-ms(after,"disrupted"):.3f} | '
                       f'{total(before):.3f} → {total(after):.3f} |')
    report = """# Sparse Cell allocation: unchanged Bake/workload

Apple M3 Max; each timing variant runs once, RAYON_NUM_THREADS=1, no profiler
or frontier audit during timing. Actual GPU matching + native RHS/merge/rebuild,
not a standalone storage prototype or a full eggcc benchmark. Dense control is
the pre-change f120fba executable/worker, frozen before editing; sparse uses the
same four training contexts, five library variants, eight consumers, 512
instances per structural family, facts, schedule and union/subsume perturbations.

## Resident memory and the two previously reported inference phases

All arrows are dense → sparse. Memory is the sum of live persistent Metal buffer
allocations after the disrupted phase: **not RSS or transient allocation peak**.

| Library | Interface plans (unchanged) | GPU MiB | Intact inference ms | Disrupted inference ms |
|---|---:|---:|---:|---:|
""" + "\n".join(rows) + """

The rich library retains **4 native-certified states / 24 interface plans**.
Persistent allocations fall 94.46% (18.03× smaller), with no smaller library,
sampling, candidate budget or truncated output. Intact and disrupted phase
times improve by 2.34× and 5.15× in this run. Small-library disrupted timings do
not all improve; sizing/compaction introduces synchronization and dispatches.
These single-run timings are not statistical speedup estimates.

## Bootstrap and total inference must also be reported

Initialization includes worker startup/compiler/cache effects; it is excluded
from the original two inference phases. Bootstrap is the first consume pass.
Total inference = bootstrap + intact + disrupted, excludes initialization,
Bake, input parsing/insertion and final comparison.

| Library | Initialization ms | Bootstrap ms | Total inference ms |
|---|---:|---:|---:|
""" + "\n".join(startup) + """

**Total rich-library inference does not improve in this single measurement.**
Sparse bootstrap contains 56.724 ms in Rust binding decoding/sorting versus
0.042 ms dense, with the same binding counts and bytes. Base-library decoding
also spikes. That CPU path was not changed, and the cause has not been isolated;
do not discard the measurement or claim an end-to-end speedup. Rich bootstrap
worker round trip itself drops from 27.740 to 15.639 ms. Shader initialization
times vary strongly too, so they are not evidence of the allocator's speed.

## Rich-library storage accounting

| Item | Dense | Sparse |
|---|---:|---:|
| Native row slots / capacity | 20,096 / 32,768 | 20,096 / 32,768 |
| Interface frame capacity | 786,432 (rows × 24) | 65,536 |
| Active interface instances | not recorded | 54,784 |
| Historical slot high water / currently free | not recorded | 55,296 / 512 |
| Slots reused across events | not recorded | 1,920 |
| Interface frame bytes | 88,080,384 | 7,340,032 |
| Recorded watch entries / capacity | 88,960 / 16,777,216 | 82,560 / 131,072 |
| Recorded ready-port entries / capacity | 17,920 / 4,194,304 | 16,256 / 16,384 |
| Directory + metadata + free-list bytes | no directory | 1,179,688 |
| Watch log + hash-head bytes | 637,534,208 | 5,767,168 |
| Ready-port bytes | 33,554,432 | 131,072 |

An interface instance is an actual anchor-row/projection binding, **not** a
distinct baked semantic Cell. Ready, waiting-for-a-key and uncertified shapes
all retain state; missing-key subscriptions are necessary for completeness.
Several interfaces/anchors can refer to the same native structure. Logs can
contain invalid old versions until compaction; differing log counts do not
mean bindings were discarded. Capacity rounding and a reusable high-water pool
remain, but are determined by observed instances/dependencies, never rows ×
all plans. This is an allocation fix, not a claim of compressing native e-nodes.

## Semantics and work

- All five libraries: exact complete native facts, values, equality and visibility
  agree after both phases. Timing runs check final graphs outside timed regions.
- Separate 32/family check run audits every incremental binding frontier before
  native RHS. All pass, including unknown/ambiguous contexts falling back.
- 128/family profile exactly preserves previous Ready/Suspected/Unmapped counts,
  successful bindings, reused reads and query/locator probes. Rich intact Ready
  stays 1,024/1,024; disrupted stays 832/1,600. Nothing is removed from Bake.
- 31 GPU/native regressions pass. New tests keep Cell/watch/port capacities
  constant after 4,096 unrelated rows, reject wrong literal anchor indices,
  cover cache-only mode, and recycle slots through late facts and same-batch
  retire/reallocate with exact non-idempotent RHS counts.

Raw JSON includes all timing phases and counters. Run `python3 compare.py` to
recheck comparability and regenerate this table; no benchmark rerun is needed.
"""
    (ROOT / "RESULTS.md").write_text(report)
    print("All library, work, frontier and final-state comparison assertions passed.")


if __name__ == "__main__":
    main()
