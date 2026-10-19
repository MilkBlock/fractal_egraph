# Parameterized terms Cell update program

This experiment tests whether one Bop spine and a fixed update program can
survive changes to Term/cost ports, without registering a concrete saturated Cell
for every value combination. It uses the actual egglog commit/merge/rebuild APIs
on CPU. It is **not** GPU acceleration, a whole-Cell saturation shortcut, or a
proof that all of raytrace's terms family has a finite closure.

The input is [terms.egg](terms.egg): the real Bop rule plus six helper rules
from the saved raytrace Pass-3 program, lines 1072–1139 in
`egg_layout/out/raytrace-interactive-20261010/pass-3-flat.egg`. Op/Expr/Term are
reduced to the necessary constructors; V and TermLeaf provide external ports.
Rule names are shortened. `PotentialExtractedExpr` remains a constructor,
`ExtractedExpr` keeps `:merge (Smaller old new) :unextractable`, and the native
NoTerm sentinel, cost expression, negative-cost panic and all three Smaller
rules are retained. No substitute min function or pruning of losing candidates.

## What is shared

For all instances and updates, compile this wiring once:

```text
Bop(o, left, right)
    ├─ ExtractedExpr[left]  → TCPair(t1, c1)
    └─ ExtractedExpr[right] → TCPair(t2, c2)
              ↓ original RHS
PotentialExtractedExpr(Bop(...), TCPair(TermBop(o,t1,t2), 1+c1+c2))
              ↓ original helper schedules
ExtractedExpr / Smaller / union / rebuild
```

Term IDs and cost values are runtime payload. Changing them does not create a
new template. A second Bop can use the first Bop's ExtractedExpr as an input, so
feedback between two instances is exercised. An equality class can contain
multiple TCPair constructors: the inverse index retains all of them, and all
compatible bindings remain distinct.

The seven rule kernels are inferred from native compiled atoms by
[`parameterized_cell.rs`](../../cell-assisted-egglog/src/parameterized_cell.rs).
The compiler admits a unique seed from which each remaining atom can be reached
by a complete input key or a bound equality-sort output. It refuses disconnected
Cartesian queries and ambiguous seeds rather than choosing a heuristic seed or
silently dropping a rule. This is a deliberately restricted compiler; the normal
matcher remains the alternative. No automatic Analyze-to-parameterized-family
discovery is claimed here.

## Incremental correctness contract

1. A committed-row feed mirrors five queried native tables, including visibility
   and timestamps. It stores forward keys and inverse equality-output keys.
2. Each evaluated seed subscribes to the keys it actually read, **including
   absent keys**. Updates wake affected seeds; new seed rows wake themselves.
   Native union/rebuild removals and canonical replacements use the same path.
3. Re-evaluation retains constants, repeated-variable constraints, every inverse
   alternative and exact native guards. A binding is eligible only when at least
   one participating row reaches the original rule's semi-naive watermark.
4. All rules in a Run read a frozen pre-action state. Their complete binding
   batches go through the original native RHS and original cursor completion.
   The nested Saturate/Sequence/Run boundaries are preserved; no helper result
   is published early. Rulesets not installed here, and Run with `until`, run
   natively and return their committed deltas to the same mirror.

For this admitted query shape, an insertion enabling a new binding either adds
a seed or changes a subscribed read along a previously explored prefix. This
is why failed reads matter: otherwise a later external port would never wake
the old seed. Removals invalidate that prefix; canonical replacements also wake
it. Pure guards cannot change without a changed input. These are the conditions
under which the program supplies a complete frontier to native cursor completion.

The independent audit compares the mirror against actual native scans, including
timestamps and reverse-index contents. Those scans are **validation only**, not
the incremental update algorithm. Errors stop the runtime permanently; the tests
require both paths to report the same error, not an unproved equivalence of
partial error states.

## Validation

[`examples/parameterized_cell.rs`](../../examples/parameterized_cell.rs) runs 17
phases against both fused native egglog and the cached native-query/same-submit
CPU runtime. Every phase requires:

- all original facts, literal values, equality partitions and subsumed visibility
  equal modulo fresh allocation IDs; no Smaller table excluded;
- per-rule emitted binding counts equal to the native-query control;
- independently scanned native table rows and the incremental mirror equal.

Scenarios include better/equal/worse cost candidates, different Terms, late input,
input union, multiple TCPair aliases, a new instance, eight further updates, and
a quiescent run. The last run must perform **zero additional keyed reads**.

[`tests/parameterized_cell.rs`](../../tests/parameterized_cell.rs) adds finite Run
boundaries, native handoff/handback, non-idempotent function updates after
delete/reinsert, subsumption, negative-cost and overflow errors, and refusal of
an unbound Cartesian query. These tests need no GPU. The included comparator's
negative tests ensure graph differences are actually rejected.

Use the existing skill entry, which builds the release example and saves input,
result and provenance without compiling a GPU worker:

```sh
/opt/homebrew/bin/python3 skills/cell-assisted-egglog-bench/scripts/bench.py \
  parameterized --output out/parameterized-cell-new
```

The normal `bench.py test` includes this regression target alongside existing
Cell/GPU tests. [results/summary.json](results/summary.json) records this run's
checks and counters; [results/program.egg](results/program.egg) is its complete
native-replay input.

The release run passed all 17 phases, including both full-graph controls. The
final graph has 210 original rows, including 4 Bop, 33 PotentialExtractedExpr,
60 Smaller and 50 TCPair rows. It retains 154 watched rows and 502 dependency
edges; these are real auxiliary-storage costs, not compressed logical counts.

| Measured counter | Result |
|---|---:|
| Fixed update-program compilations | 1 |
| Rule kernels in that program | 7 |
| Concrete saturated-closure catalog entries | 0 |
| Original ruleset runs | 244 |
| Emitted logical bindings, matching native per-rule counts | 222 |
| Local keyed/output reads | 1,553 |
| Initial mirror rows | 9 |
| Additional keyed reads in final quiescent phase | 0 |

The focused suite passed 59 Rust tests (including the six new semantic tests
and the graph comparator tests), plus 9 skill-script tests. The negative-cost
and checked-integer-overflow cases report the identical native error and reject
resumption. These are correctness results; there is no speedup measurement.

## Limits and cost

No concrete closure catalog is created, but **real facts are not erased**.
Potential/TCPair/Smaller rows, aliases, inverse alternatives and dependency
subscriptions can grow. This experiment removes value-specific template
enumeration; it does not prove termination or bounded space for arbitrary rules.

For fixed arity/program length, with F mirrored rows, D subscribed read/seed
edges and K compiled kernels,
retained auxiliary state is O(F + D + K). Ordered index operations are logarithmic.
For a batch of changed rows, work also depends on subscribers woken and actual
inverse-lookup fanout; the implementation dispatches each change to K kernels.
The temporary binding batch costs O(Mw) for M matches of width w; parameterization
does not remove a genuine product of compatible alternatives. An unchanged
public Run still has fixed schedule/API overhead, even when it does
no keyed reads. Logical binding counts are not committed mutation counts.

This is an independent correctness prototype in the native runtime, not the
default raytrace path. It still executes local indexed lookups and iterative
helpers. A reusable **joint saturated-closure certificate**, automatic family
discovery, GPU execution and a net speed/memory benefit remain unproved.
