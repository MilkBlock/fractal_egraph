# Restore general capabilities in the resident GPU engine

This changes the existing ABI7 worker and common compiler, not a new backend.
Source markers and genuine multi-step Cell recipes still use the same tables,
identity domain, source schedule and publication machinery.

## Restored behavior

1. **Merge index continuation.** A callback needing a canonical constructor
   index saves its duplicate cursor and requests parallel refresh. The refresh
   sizes itself from the current row count, including constructors appended by
   earlier callbacks. The worker resumes the first unexecuted callback without
   regrouping/replaying the prefix. A later union can request another refresh.
   The generated resident adapter no longer deletes the old continuation.
2. **Typed entry routing.** Each active seed table selects only its applicable
   rules, with original ranks/delta variants preserved. Grounded queries run
   once even on an empty graph; a rule with both grounded and branching variants
   is not duplicated on lane zero. The old correlated cache route remains intact
   when explicitly requested; it is not confused with physical row coordinates.
3. **Ordinary RHS.** Single-step programs use complete compiled actions. When
   the current scope has no selected multi-step recipe, matching/order/no-op
   preparation feed those actions directly, without per-binding Cell frames,
   completion slots or wave packets. Independent constructor segments still
   use the existing parallel writer. Actual multi-step programs are retained.

Sorting now sizes its merge dispatches from the match arena independently of
Cell frame capacity. Previously context growth happened to guarantee that all
required sort passes were launched. The no-frame path exposed this assumption
at 2,048 bindings; the new large ordinary regression keeps that case covered.

During native differential testing, the shared device merge routine also exposed
an older correctness bug: `old == new` was treated as a universal no-op. Custom
merges need not be idempotent. The shortcut now excludes custom callbacks, and
such callbacks can request index refresh even for equal arguments. The regression
performs 1,537 additions to a function key and requires the native result 1,537.
This semantics repair is explicit and shared by both comparison lanes.

## Controls and accounting

The compiler accepts `resume_merge_index`, `typed_seed_dispatch` and
`direct_ordinary_rhs`, all defaulting to true. These switches are for local
counterfactual checks, not workload names or candidate budgets. Descriptors in
the compiled artifact expose the actual setting. Batch layout v9 adds counters:

- `merge_index_refreshes`: suspended callback index refreshes;
- `merge_index_rows`: row extent visited by these refreshes;
- `ordinary_rhs_bindings`: bindings routed through ordinary publication,
  including proven no-ops (not a committed mutation count);
- `ordinary_rhs_runs`: Runs taking the no-Cell-frame route.

The physical table stride and typed row representation are unchanged. A compact
no-op mask is allocated in the match arena; only Runs using actual Cell programs
need to grow the wider context arena. Noncommuting callbacks retain their order.
Private schedule gates, finite Repeat and ordinary Run visibility are retained.

The original wave experiment explicitly sets `direct_ordinary_rhs=False` so its
single-step wave ablation remains reproducible. Disabling ordinary direct RHS
does not disable matching or shrink the Analyze library.

## Verification

```sh
python3 experiments/resident_restoration/check.py \
  --build out/eggfuse-clean-paths-build --output out/new-restoration-check \
  --bench 2048
```

Six source fixtures cover custom merge/index continuation, finite ordinary
updates, late inputs/multiple delta sources, union-created matches, large
ordinary batches and empty-graph grounded queries. Each configuration is checked
against the independent native source execution; the restored path also runs
again in a fresh worker for complete per-Run/final-state repeatability.

Existing checks used:

- 41 eggfuse unit tests, using the existing solver environment;
- 19 staged Cell cases, including actual return ports, correlated conditions,
  finite control, constructor compaction, large i64, expected overflow, subsume,
  independent unions and set keys;
- resident constructor writer checks, including mixed ordered actions,
  neighboring arenas growing independently and partial effects before errors;
- four physical checkpoint/restart cases and damaged-program/buffer/worker
  rejection. The growing-Cell fixture now has an actual saturated composition;
  ordinary finite Runs no longer allocate Cell frames merely to exercise growth;
- eight existing source-marker fixtures with native projection, resident GPU
  comparison and repeated GPU execution.

Timing and checked runs remain separate. The full Pass-3 comparison deliberately
reuses the previous **unmarked** typed input to isolate general-backend repair;
it is not a claim that the full source-marker pipeline has been benchmarked.
`--pass3-core FILE --reuse-control PREVIOUS_WAVE_DIR --audit-repeat` preserves
that original typed source/schedule, records the old timing as historical and
performs a new clean run followed by checked repeated runs. Final-state/quality
differences must be reported separately; no old CPU endpoint guarantee is
inferred from GPU repeatability.

## Retained full Pass-3 measurement

One clean inference, no averaging; previous 185.707129 s reused as historical.
The restored executor takes **162.650079 s** (12.4% lower), GPU command duration
160.491739 s. Outer command buffers fall from 2,971 to 2,137, Cell effect waves
from 9,971 to 3,304. There are 31 explicit merge-index refreshes scanning a total
16,268,778 row positions; 384,829 bindings across 4,649 ordinary Runs bypass
Cell-frame allocation. Another 2,070,174 direct single-step invocations remain
inside mixed batches; this is not elimination of all scalar publication.

Both complete audited executions agree on all 9,786 logical Runs and final
state. The final row count is 702,038, versus the historical control's 701,998.
The additional rows are exactly 20 `bound-min` and 20 `bound-max` rows, all with
equal operands. The source custom merges for `hi-bound`/`lo-bound` construct
these nodes even when old equals new; the previous blanket idempotence shortcut
omitted them. The new result therefore includes that semantics repair, and the
whole-program timing is not a pure, equal-output three-switch ablation.

This remains much slower than the historical ~12 s assisted / ~40 s device
backends. No 2x full-workload claim or newly measured occupancy is made. Source
marker integration and remaining scalar callbacks/global maintenance are not
silently declared solved. `results.json` preserves the counters, checks and
historical label; large graph/shader/checkpoint files remain untracked in out/.
