# Typed numeric guards and merge in the shared matrix program

Live transport update: the worker handshake is protocol **6**. Rebuild
`matrix.swift`; bulk live heaps now use binary frames. Offline lowered ABI **5**
and replayable JSON files are unchanged. Numeric workloads also support the
[resident fragment-table backend](../table-runtime/README.md), which sends deltas
and does not rebuild per-instance channels or solve online.

The existing Cell matrix IR now carries numerical dataflow as well as identity
features. The same generated allocation and persistent Metal worker execute both.
This is **MPS structural matching plus exact integer vector operations and segmented
reductions**, not a claim that arbitrary rule semantics become one ordinary GEMM.

## The calculation

For candidate binding row `r`, let `s[r]` be structural compatibility and `g[r]`
its numeric guards. The accepted mask is `m[r] = s[r] && g[r]`. Let `v[r]` be the
compiled numeric RHS and `key[r]` its canonical target function key. For a min merge:

```text
new[g] = min(old[g], { v[r] | m[r] and key[r] = g })
```

Max uses the same layout with `max`. Missing old values have an explicit presence
bit; neither `i64::MIN` nor `i64::MAX` is used as a missing-value sentinel. Min/max
are associative, commutative and idempotent, so batching and deduplicating proposals
preserve their value. Custom/order-dependent merges are not silently reduced.
`Smaller(TCPair(...), TCPair(...))`, tie-preserving argmin and equality union need
their own algebra and remain outside this numeric reducer.

Guards read a stage snapshot. All accepted proposals are prepared before any
numeric state is changed. The next stage can read the merged values and use them
in guards and parameter updates. The shared stage/depth scheduler may evaluate a
cheap numerical guard before an expensive structural Gram block. A numeric guard
cannot replace missing structural evidence or combine half-bindings from different
alternatives.

## Representation and exactness

`Program.numeric` is an optional typed component containing:

- `inputs[binding, numeric_slot]`: actual i64 payloads, **not intern IDs**.
- `state_bindings[binding, slot]`: explicitly wired scalar state registers.
- A shared expression DAG: input, state, constant, checked `+`, `-`, `*`, min, max.
- Guards `<`, `<=`, `>`, `>=`, `=`, `!=`, with channel and exposure depth.
- Versioned canonical-key groups and guarded RHS proposals.

`numeric_lower.rs` derives all matrix dimensions, offsets and field constants.
The device stores an i64 as two unsigned 32-bit words. Signed comparison uses the
signed high word and unsigned low word. Arithmetic computes carry/borrow and
checks overflow; the CPU reference uses Rust checked i64 operations. Float and
Tensor Core approximations are not used for numerical values. A 16-bit partial
product implementation avoids an AGX compiler failure encountered with `mulhi`
in the dynamic operation graph.

The numerical matrices are `numeric_inputs`, `numeric_values`, `numeric_defined`,
`numeric_evidence`, `numeric_proposals`, `numeric_state`, plus generated operand
and group-index matrices. The structural feature graph and its MPS matrices are
unchanged. Group incidence is represented by segment indices; no dense
`bindings × keys` all-ones mask is materialized merely to perform min/max.

The current numeric DAG evaluator computes all columns once per needed binding
row. Thus a false guard produces **no proposal/commit**, but may still have computed
an unused RHS column. An overflowed unused column is not a failed mutation.
Computed guard failure/overflow rejects that current binding. An overflow used by
an accepted RHS aborts the numeric batch before any numeric group update. Native
egglog may expose partial effects on errors; this conservative batch behavior is
not presented as reproducing every native failure prefix. Numeric mutations and
Cell-target publication currently require separate IR stages so an arithmetic
failure cannot publish an uncertified partial Cell transition.

Work is `O(lanes × expression_columns + guards + writes + groups)` for a stage,
apart from the structural matrix calculation and plan preparation. Guards and
expression rows run across lanes; merge runs across keys. The first reducer
processes each key's segment serially, with span proportional to its largest
segment. It is not yet a parallel tree reduction for one enormous key group.
GPU work counters count expression nodes/guards/proposals/reductions, not hardware
instruction counts. No throughput or speedup claim follows from them.

## Native event integration

`--mode matrix-numeric` compiles a selected consumer whose RHS is **one set action**
to a custom i64 function with explicit `min old new` or `max old new` merge. Numeric
RHS expression DAGs and table-bound guards come from egglog's own parsed/resolved
rule. Computed constructor keys, extra RHS effects, non-i64 payloads and arbitrary
merges are rejected. The general matrix IR supports more guard expressions than
the current native action-sink frontend, which still requires table-bound i64
comparisons on the LHS.

The frontend reuses the existing analyze Cell projections and native committed
row/union events. Source Cell evidence is checked by CPU keyed reads; external
function/relation reads and candidate-fragment enumeration remain host work.
Structural compatibility, numerical guard decisions, RHS arithmetic and min/max
proposal reduction execute on the GPU. The original consumer LHS is never run on
the production lane. `--audit true` runs it separately as an oracle.

Canonicalization and rebuild stay native. Before dispatch, keys are canonicalized
and proposals for the same key are assigned the same group. A changed native row
creates a new immutable fragment and numeric snapshot version. Old matches do
not become perpetual assertions about future function values. Hot installation
preserves old numeric state, evidence and consumed-write receipts; an idle/resume
call cannot apply the same proposal again. Union tests exercise key collision and
new guards after rebuild.

GPU outputs are submitted as **one native set per changed group/key snapshot**;
the original RHS arithmetic is not repeated for each match. Native storage still
performs its final min/max check and journals the mutation. Before committing, the
host verifies that the original physical function value has not changed. Hidden
rows still participate in merge, while LHS reads require visibility. Pending
outputs from an aborted batch are retained for retry when their source versions
remain valid. If a pinned native state changed unexpectedly, commit fails closed.

This mode does not pretend that mutable numbers are saturated Cell facts. A pure
structural Cell can be baked separately; `--consumer-source` adds runtime function,
relation and rule declarations. They do not retroactively become analyze evidence.
The previous constructor-only automatic Bake gate remains in place. General
numeric/guarded Cell specialization, arbitrary merge proofs, and all-rule eggcc
acceleration are not completed by this change.

## Optional native verification before commit

Use `--gpu-check on` (also accepts `true`) with `--mode matrix-numeric`.
The default is `--gpu-check off`. This is independent of the older `--audit`
coverage diagnostic. A normal unchecked run should leave both off.

The checker lives in `egglog/src/cell_assist_check.rs`. It compiles and reuses a
native oracle from the **original resolved rule**, not from the GPU instruction
stream. The oracle runs the full LHS, including numerical guards, and computes
the native RHS primitives without writing the target function. Before any GPU
update from that batch is submitted, it checks:

1. Active accepted bindings equal the native match set. Missing matches also
   fail: this is strict diagnostic mode for the selected consumer, not a claim
   that an incomplete Cell library covers it.
2. Every pending proposal has the right binding, destination key and exact i64
   RHS value, including proposals that lose a min/max reduction.
3. Each key snapshot still has its original physical value. Native RHS/merge
   replay on an isolated clone must produce the GPU's merged value, including
   unchanged or absent outputs. Changed-value/version flags must agree.

The full query is compiled once. Verification uses native replay clones per key
snapshot with proposals; these deliberate debug costs are absent when disabled.
It does not modify the live target table. Any discrepancy rejects **all GPU
writes in that batch**, reports the mismatch, and makes that session unusable for
further rounds. Already committed helper work/earlier batches are not rolled back.
Reports include `stats.verification` with enabled state, verified batches,
bindings/proposals/groups and failures. Turning checking on/off is also supported
through `NumericSession::set_gpu_check`; a failed session cannot bypass rejection
by turning it off.

Tests corrupt the actual Metal response (losing RHS, merged value, accepted
matches, and change version), assert zero native sets and unchanged target
values, and cover union/rekey, late function updates, max merge and resume.

## Reproduce

```sh
python3 cell-assisted-egglog/validate_numeric.py --output out/matrix-numeric-check
```

This builds Rust and the Metal worker, loads the recorded real eggcc `Single/Get`
Cell analysis, and compares one native run with one matrix run. The mutable
consumer is in `consumer.egg`; input is in `input.egg`.

Direct invocation, after that build:

```sh
cargo run --offline --manifest-path cell-assisted-egglog/Cargo.toml \
  --target-dir target/cell-assisted -- \
  --mode matrix-numeric --analysis cell-assisted-egglog/fixtures/analyze \
  --consumer-source cell-assisted-egglog/matrix-numeric/consumer.egg \
  --ruleset '' --consumer cost-improve --native single-get \
  --parts '[{"atoms":[0],"cell":true},{"atoms":[1],"cell":false},{"atoms":[2],"cell":false}]' \
  --input cell-assisted-egglog/matrix-numeric/input.egg --rounds 4 \
  --gpu-worker out/matrix-numeric-check/matrix-metal --gpu-check on
```

The fixture has **3 accepted bindings and 2 native function updates**. Final values
are `9999999999999999`, `2`, `1`, exactly matching native egglog. The check compares
primitive result payloads separately from graph identity: 6 Expr enodes / 5 Expr
eclasses, 4 Offer relation rows, and 3 numeric function rows. Primitive numbers
are not counted as eclasses. See [results.json](results.json).

The reproduction script compares native egglog, GPU with verification on, and
GPU with verification off, including final graph equivalence and primitive values.
It asserts that the unchecked run has zero verification batches and no audit.

Tests include all comparisons on i64 extremes and adjacent large integers,
checked add/sub/mul against native primitives, false guards with overflowing RHS,
missing old state, multi-stage read-after-merge, min/max conflicts, hot state
extension, no replay, union/rekey, late updates, rejected custom merges, and
failure recovery preserving already-completed matching. All 71 package tests
passed with the numerical workers enabled. Older constructor/Bake matrix paths
remain regression controls.

Source IR version remains 1 (the numeric component is optional). Lowered/worker
ABI is now **5**; regenerate old lowered JSON with the new executable. Core files:
`src/numeric.rs`, `src/numeric_lower.rs`, `numeric.metal`, `src/numeric_native.rs`.
