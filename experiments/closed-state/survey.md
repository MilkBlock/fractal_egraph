# Math6 ClosedState sharing survey

This survey attempts **all 449 historical Use instances** in the saved default
full-policy Math6 analysis, not just the 243 currently active Uses. The dictionary
contains 168 templates; 82 have a historical Use witness, and 86 have no instantiated
Use to ripen. Template IDs and layer IDs are local to this source history.

Each attempt executes native `ripen-use` independently with a four-sweep budget,
an eight-second timeout, and a sampled 768 MiB RSS limit. The watchdog samples every
0.2 seconds; this is not a hard instantaneous memory bound or a peak-memory
measurement. No timeout, memory stop or disk stop occurred. Failed resource-limited
attempts would have their own incomplete cell directory removed, with logs and the
ledger retained; source histories are never deleted. The survey used about 226 MiB
of output for this run, including all per-round diagnostics.

| Outcome | Use instances | Templates |
|---|---:|---:|
| Closed, exported and compared | 303 | 39 |
| Suspended at four sweeps | 29 | 8 |
| Entry extraction/replay rejected | 117 | 35 |

No template had mixed outcomes among its instances in this dataset. That is an
observation, not a guarantee for future instances. Rejections comprise 55 late
external producers, 50 external reads that would pre-create internal results,
5 unversioned later inputs, and 7 failed staged-replay checks. Neither rejection
nor suspension proves that the combination cannot eventually mature.

## Shared states

All 303 exported states were compared with the exact supported constructor-state
checker. They form **9 states**, with no budget-unknown comparisons. Eight states
are shared by multiple instances; **six states are shared across different
combination templates**. The 39 template memberships below are disjoint in this
sample. These are conditional symbolic-interface results, not a concrete global
closure theorem or a byte-compression ratio.

| State | Uses | Distinct templates | Representative family |
|---|---:|---:|---|
| C0 | 37 | 1 | Add commutation |
| C1 | 27 | 4 | Sub elimination + Add/Mul commutation |
| C2 | 77 | 1 | Mul commutation |
| C3 | 100 | 12 | Add association/commutation, three parameters |
| C4 | 12 | 6 | Product differentiation, three distinct parameters |
| C5 | 3 | 2 | Add association/commutation, b=c |
| C6 | 29 | 6 | Mul association/commutation |
| C7 | 17 | 6 | Distribution/factoring + commutation |
| C8 | 1 | 1 | Product differentiation, x=b |

C3 includes T12, T11, T13, T17, T18, T51, T77, T53, T95, T93, T101 and T81.
For example, T12 has two members and 29 successful Use instances; T77 has six
members and six instances. Both mature to the same C3. C4 and C8 deliberately
remain different despite similar rule names: their boundary aliases differ.

## Coarse/Smooth correspondence in the debugger

Open `http://127.0.0.1:8080/?closed_catalog=out/closed-survey/catalog`.
The shared-state table lists instance and distinct-template counts. Choose a
ClosedState to list its combinations, then choose one combination to inspect:

- each member's CoarseComb/SmoothComb classification;
- actual internal output-to-input port edges and context edges;
- the source rule, observed alias numbers, and instance-specific original
  coarse/smooth layer references.

The colors use **direct recorded dependency ownership relative to that Use**:
external parents/facts/bindings or an entry with no parents make a coarse member.
This is not a search for minimal alternative proof requirements. `source_kind`
retains the original global layer classification separately. Layer IDs identify
associated source interfaces; they do not prove complete layer domination.
Independent members are drawn as a DAG, not falsely serialized into a chain.
Unordered parent lists are normalized when grouping display patterns.

Legacy catalogs remain viewable. New catalogs carry `state_groups`, `comb_groups`
and optional scan statistics; detailed member metadata is read-only and does not
change the equivalence result. Unknown equivalence comparisons remain explicit.

## Reproduce

```sh
cargo build --release
python3 tools/scan_closed_states.py \
  --history out/use-reuse-final/math/history.json \
  --analysis out/adaptive-reuse-final/full/analysis.json \
  --output out/closed-survey \
  --rounds 4 --timeout 8 --rss-mb 768
```

Use a new output directory. `--limit N` is an explicitly partial survey. The driver
verifies each returned Use's members/template against the input analysis; it does
not silently assume that IDs from another builder version still identify the same
combination. The driver orchestrates independent native jobs; it does not add a
raw trace pipeline to each run.

`scan.json` retains every outcome, `logs/` retains native diagnostics, and
`catalog/` is loaded by the existing debugger. `survey-results.json` is the compact
checked-in result with source digests and per-state template patterns. Rust/native
tests cover member metadata; Python tests cover timeout isolation and annotation
identity; the browser test checks all nine groups, the twelve C3 templates, member
colors, bindings and source layer references.
