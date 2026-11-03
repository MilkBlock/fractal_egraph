# One Spine continues RHS execution across source stages

`CELL_CPU_STAGE_SPINE=1` enables a persistent context that feeds the actual RHS
recipes of later source Runs. This extends query-prefix sharing with shared
producer context and source-scheduled execution. **It remains OFF by default:**
the implementation is correct within the checked scope, but both the real
Pass-3 input and the synthetic chain regress in elapsed time.

This is the **standalone, single-thread Cell CPU engine**. Its native frontend
supplies the typed program; it does not link the egglog runtime, run on GPU, or
certify terminal RipSat/Cell substitution. `CELL_CPU_MERGE_PROGRAMS=1` remains ON.
The earlier raw-native whole-Pass-3 discrepancy is not resolved by this change.

## What is shared

A Spine owns a context containing exact `(table, canonical functional key)` to
physical fact/function references. Its cursors carry the continuation depth.
After an original RHS finishes, its query ports and resulting table ports are
captured; after that Run's normal repair, they resolve to current visible
witnesses. Multiple produced anchors point to the same context. The next
consumer's original Run can read the context, extend it, and execute its entire
original RHS, including constructor requests, function updates, custom merges
and unions. There is no special rule-name or endpoint catalogue.

For example:

```
a: Seed(x)             -> P(x), Known(x)
b: P(x)               -> Q(x)
c: Q(x), Known(x)     -> R(x)
```

At stage c the context still contains a's `Known(x)` even though b did not read
it. It supplies the ancestor witness without a new key lookup. This behavior
has a native-backed regression. Facts read from another source can be acquired
through current exact ports and added to the context; the implementation does
not require every fact to have been constructed by one immediate producer.
It does not yet combine arbitrary inverse/multivalued joins into one recipe.

The final implementation stores **one cursor per physical anchor**, shared
across consumer programs. It attaches a consumer only at that consumer's legal
Run. An eager draft instead constructed a separate frame for each potential
consumer, preparing 60,328 frames for 2,336 resumed steps. Both attempts and their
binary fingerprints are retained in `stage_spine_results.json`; the eager draft
is superseded and was not separately committed.

## Coverage and semantic boundary

- The compiler emits `multi_stage.stage_entries`. Starting from a bound table
  row, every remaining table read must have its entire functional key bound;
  primitive arguments must also be bound. Every query input must be available
  at completion. Thus one pivot determines at most one full binding. An inverse
  read with multiple possible results is not silently reduced to one witness.
- Rust independently reconstructs this exact typed key flow and rejects a
  modified plan, even with a recomputed checksum. Legacy artifacts without the
  new entry field still run with the feature OFF; enabling it requests a
  recompile instead of claiming coverage.
- A takeover requires coverage for **every matching live pending delta pivot**
  in that rule. Unknown producer context or unsupported key flow falls back to
  the existing complete matcher for the whole query. Failed takeover attempts
  still cost work and are counted. No observed hit percentage is used as proof.
- Witnesses are read in the current graph and checked for table, key and
  visibility; current function outputs bind subsequent ports. Missing, hidden,
  replaced or coalesced witnesses require an exact refresh or ordinary fallback.
  Union canonicalizes retained keys without discarding unrelated context ports.
  Multiple constructor alternatives after union remain exhaustive via fallback.
- All LHS bindings for a Run are gathered before any RHS executes. Context
  publication happens after normal repair. Nothing executes a future ruleset
  early, changes finite Repeat counts, fuses source barriers or skips effects.
  Naive rules retain ordinary replay. This differs from local saturation.
- Semi-naive watermarks, unique binding order, original RHS and merge order,
  original fact/equivalence/visibility and source changed flags are preserved.
- Checkpoints contain the authoritative graph and watermarks. These optional
  contexts rebuild cold after resume; old RHS effects are not replayed.
- Retired anchors lose their cursor. Contexts are reference counted and disappear
  when no anchor/cursor retains them. Live contexts keep acquired distinct port
  keys; this is not a bound on memory for arbitrary ever-growing workloads.

The runtime reports logical binding/step counts, request counts and effective
unions separately. `resumed_steps` is not a count of committed mutations.
`max_chain` is a cursor-path depth; `max_sets_per_spine` counts rulesets served
by a shared context and may include sibling paths, not just one path.

## Real sum-bits Pass 3

Final paired output: `out/stage-spine-lazy`. Same binary, same authenticated
source_sk_mk artifact and schedule, feature OFF/ON. Each lane ran once, without
overlapping builds, tests or audit; diagnostics ran afterward. These are not
measurements of pristine egglog. The 133.520 ms result in
`BAKE_AND_LOCALITY_REPAIR.md` used a different binary and local-saturation mode;
it is not this experiment's timing control.

| Metric | OFF | ON |
| --- | ---: | ---: |
| Inference | **133.740 ms** | 168.258 ms |
| Setup | **1.111 ms** | 1.284 ms |
| Whole process wall | **284.876 ms** | 318.878 ms |
| Peak RSS | **204.00 MiB** | 206.86 MiB |
| Ordinary delta/residual + merge candidate visits | 84,033 | **80,450** |
| Total decoded query rows, new path included | **113,887** | 117,119 |
| Total exact-key lookups, context capture included | **62,212** | 81,809 |
| Existing forward point reads | 29,795 | **25,735** |
| Constructor requests | 19,188 | 19,188 |
| Union requests / effective unions | 3,210 / 1,039 | 3,210 / 1,039 |
| Source Run / repair calls | 1,496 | 1,496 |

Candidate work falls **4.26%**, but inference regresses **25.81%**. The removed
candidate visits are replaced partly by direct current-witness reads, so their
count alone is not total saved work. The new context capture performs 20,129
key lookups. Combined with remaining refreshes and displaced ordinary reads,
total key lookups increase by 19,597. Search rises from 71.872 to 75.644 ms;
RHS including capture rises from 26.223 to 36.287 ms; query publication rises
from 5.171 to 25.461 ms. Capture and publication instrumented subtotals are
7.460 and 19.576 ms, already inside inference, not additional elapsed costs.

Final execution coverage:

- 492 typed keyed entry programs; 526 producer rules have eligible outputs.
- 2,637 complete bindings resume the existing Spine, out of 12,500 total logical
  RHS applications (**21.10%**). There are 1,039 successful whole-query takeovers
  and 1,604 attempted takeovers that fall back. Takeovers include empty results.
- 750 resumed steps enter a ruleset not yet served by that context. Longest
  cursor path is 9 steps; the widest context serves 7 rulesets over 12 Runs.
- Resumed applications include always-run **1,273**, cell-stable-keys **752**,
  terms-helpers **193**, is-resolved **101**, type-analysis **91**, and
  apply-drop-unions **90**. Per-rule and per-set counts are in the JSON evidence.
- 540 context-witness reads and 3,528 current-port refreshes occur during
  successful takeover processing. The 10,850 inspected pending rows include
  unsuccessful attempts. None of these reads is free.
- 9,117 contexts are captured across the execution, with 17,221 anchor
  publications and 1,141 anchor retirements. At the endpoint, 2,328 anchor
  references retain 1,551 contexts containing 9,662 port references. These are
  reference counts, not physical RSS or the number of closed Cells.

The earlier eager draft was 127.310 -> 182.108 ms (**43.04% slower**). Lazy
attachment removes repeated consumer-frame preparation, but does not remove
capture lookups. Different revisions' OFF times are retained; their absolute
changes are not attributed to this optimization or combined into a speedup.

All **1,497 snapshots** match OFF/ON exactly: every table including auxiliaries,
function values, equivalence, visibility, binding lists, Run paths, iterations
and changed flags. Final state is **4,314 rows / 2,324 eclasses**. A fresh ON
process reproduces the exact final identities. These checks compare standalone
Cell executions; they do not establish whole-Pass-3 equivalence to raw native.

## Eight-stage mechanism fixture

`out/stage-spine-eight-stages` runs 128 inputs through 8 distinct source
rulesets. Exactly 128 root contexts perform 1,024 RHS applications, including
**896 resumed steps**. All 7 later queries are covered, with 1,792 current
context-witness reads, **zero port refreshes**, and no unknown-context fallback.
There are 9 matching full-stage snapshots including initialization and an exact
fresh-process replay. The native-backed three-stage fixture separately checks
this mechanism against egglog.

Inference still regresses **4.520 -> 6.393 ms**. This demonstrates persistence
and full RHS execution across stages, not a performance win. It also shows why
merely making a baked chain longer is insufficient when the runtime first
rediscovers and republishes every known port through global lookup.

## Validation and reproduction

- 15 stage-Spine tests pass, including native-backed finite Run isolation,
  ancestor facts, hidden/reinserted evidence, union/function replacement,
  inverse multiple alternatives, custom merge effects, guard changes,
  unknown-context fallback, tampered/legacy artifacts, and checkpoint resume.
  The semantic corpus test additionally checks the existing CASES inputs.
- 13 compiler/IR tests and 7 Rust unit tests pass.
- Whole runtime discovery (before adding the separately passed legacy artifact
  regression) has 162 passes / 1 failure. The failure is
  `ReferenceTests.test_marker_multiple_owners_union_late_consumer`, whose
  strict event-count assertion expects `16 < 16`. The complete graph/binding
  comparisons preceding it pass. An isolated build of unmodified parent
  **00e07c3** reproduces the same assertion. Its source is left unchanged.

```sh
cargo build --offline --release --manifest-path engine/cell-runtime/Cargo.toml \
  --bin eggfuse-cell-runtime --example project_stages --example project_semantics

EGGFUSE_TEST_FRONTEND=/tmp/eggfuse-ripen-build/debug/examples/eggfuse_frontend \
  /opt/homebrew/bin/python3 -B -m unittest experiments.cell_runtime.stage_spine_test

# Existing authenticated compile includes stage_entries. Older outputs need
# re-emission through the public compiler, preserving source and Bake inputs.
/opt/homebrew/bin/python3 -B -m experiments.cell_runtime.local_sat_bench \
  --compiled out/stage-spine-compile --feature stage-spine \
  --output out/NEW --audit

/opt/homebrew/bin/python3 -B -m experiments.cell_runtime.local_sat_bench \
  --demo-stages 8 --instances 128 \
  --frontend /tmp/eggfuse-ripen-build/debug/examples/eggfuse_frontend \
  --feature stage-spine --output out/NEW-DEMO --audit
```

The next performance boundary is **capture from commit receipts instead of
re-looking up known facts**, followed by retaining successful coverage when
only part of a rule's delta needs ordinary completion. Those changes need their
own tests and paired measurements; neither is claimed by this implementation.
