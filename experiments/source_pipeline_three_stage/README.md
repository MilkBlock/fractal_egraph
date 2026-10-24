# Raytrace Pass 3: source → SK → SK+marker

Measured on 2026-10-24 at `69ee968`, using the actual cell-assisted egglog GPU
runtime and native effects, not a standalone matching prototype. This is the
saved complete Raytrace Pass-3 program, not all three production eggcc passes.

**The marker version completes, but fails the full-original-graph check. Its
lower inference time is not a validated speedup.** No extraction-quality claim
is made. The comparator first stops at a different `Smaller` row count; this
does not establish that all remaining values/equivalences are equal.

| Program | Inference (s) | Native setup (s) | GPU/Cell setup (s) | Timing origin |
|---|---:|---:|---:|---|
| `source.egg` | 18.177226 | 0.169183 | 2.837738 | Reused matching binary/input/worker lane |
| `source_sk.egg` | 18.739206 | 0.174731 | 2.852873 | Reused matching binary/input/worker lane |
| `source_sk_mk.egg` | 17.267876 | 0.296671 | 30.611955 | One new execution |

Inference includes runtime Cell maintenance, CPU/GPU handoff, effects,
insert/merge and rebuild. Offline generation, setup, shader compilation, graph
export and extraction are excluded. Setup values are the recorded single-run
values, not a claim about repeated warm startup. No averaging or fastest-run
selection was performed. These are GPU-backend lanes; `source.egg` is not a
pristine CPU timing.

## Semantics and correctness

This comparison preserves the original arrival-order selection policy. It does
not use the separate opt-in structural `to-hoist` policy. With no declared key
consumer, the pruned SK stage adds **zero key tables, helper Runs or primitive
registrations** and preserves the expanded input body byte for byte.

* Source vs SK: equal across 699,068 original rows and 524,631 mapped classes,
  including values, equivalence and visibility. The single-run timing difference
  is +3.09%; there is no additional query/helper work to attribute it to.
* SK vs marker, and source vs marker: failed. `Smaller` has 73,371 rows before
  transformation and 73,370 afterwards. The marked graph has 699,121 total rows,
  including 54 marker rows. No speedup ratio is accepted for either comparison.
* The source of the graph difference is not localized by this benchmark. It
  must not be dismissed as a harmless auxiliary-table difference: `Smaller`
  belongs to the original program. Source rewriting, native ordering and the
  runtime query provider have not yet been isolated from one another.

The next correctness diagnostic should compare the same generated source with
inline Cell assistance on/off, then audit the selected query bindings. Do not
change the policy or drop original tables to make this comparison pass.

## What was actually generated

The marker stage reads only the preceding SK file. It profiles and checks
alternative queries at all **10,123 original native Run boundaries**, then
selects plans from those measurements. This offline plan check precedes source
rewriting and is not a substitute for the final graph check above.

* 350 selected query plans.
* 29 source consumers lowered to **14 physical marker tables**.
* 10 RHS marker publication actions.
* 321 retained query contexts declared inside the resulting `.egg`.
* 54 marker rows at the end of this runtime execution.
* No joint saturation certificate or sat-skip enabled by this pipeline.

The whole marker generation took **1,952.37 s (32 min 32 s)**. SCIP solved once
in **9.298484 s**, with 36,611 variables and 86,293 constraints; the total is
dominated by other offline work, chiefly the verified profiling pass. Maximum
resident set size reported by `/usr/bin/time -l` was 23,968,088,064 bytes. The
complete compressed profile is preserved locally and was not used as a timing
lane. Build time is not included; existing verified tools were reused.

GPU roundtrip time was 11.073413 → 4.444649 s, while event processing was
0.847158 → 2.728726 s and Cell query work was 0.000197 → 3.939900 s. These are
recorded stage observations on **different final graphs**, not independent Cell
credit. The large new setup cost also prevents claiming a cold-start win.

## Reproduction and retained evidence

Run from the `codex/explicit-keys-cell-integration` checkout. The benchmark skill
reuses unchanged lanes by fingerprint; it does not replay controls merely to
refresh their dates. Use new output directories when rerunning generation.

```sh
DEVELOPER_DIR=/Library/Developer/CommandLineTools \
/Users/mineralsteins/Repos/egg_related/egg_layout/out/cell-spine-venv/bin/python \
  -m tools.cell_spine.source_pipeline \
  --source out/key-demand-pass3/plain-exact/source_sk.egg \
  --output out/source-triple-pass3/mk --solver scip

DEVELOPER_DIR=/Library/Developer/CommandLineTools /opt/homebrew/bin/python3 \
  skills/cell-assisted-egglog-bench/scripts/bench.py program-modes \
  --backend gpu \
  --source /Users/mineralsteins/Repos/egg_related/egg_layout/out/raytrace-interactive-20261010/pass-3-flat.egg \
  --keys out/key-demand-pass3/plain-exact/source_sk.egg \
  --marked out/source-triple-pass3/mk/source_sk_mk.egg \
  --worker /Users/mineralsteins/.codex/worktrees/automatic-refutation/egg_layout/out/commit-marker-worker/matrix \
  --output out/source-triple-pass3/bench --no-build
```

The second command intentionally exits **1** for the preserved graph mismatch.
`results.json` records the full benchmark summary, generation selection,
fingerprints and comparison failure. Local full graphs and runtime Cell records
are under `out/source-triple-pass3/bench/{original,keys,marked}-gpu`; the complete
profile, solver artifacts and standalone final source are under
`out/source-triple-pass3/mk`. They are not committed as build/trace artifacts.
