# Executable whole RipSatCell A → B → C

This mode executes full closure-state transitions. It does **not** use the GPU
interface cache as a substitute for the acceptance test. State IDs are interned
by the existing `RipSatEgraph` / `ReuseGraph` whole-state equality machinery;
the runtime holds a complete typed value frame for that state.

The plan specifies only new coarse context, not target facts or a smooth rule
sequence. Original native saturation derives all three target bodies at Bake:

```text
A = Sat(Add(x, y))
B = Sat(A ∪ Mul(k, root(A)))
C = Sat(B ∪ Add(root(B), Mul(k, z)))
```

The four original rules are Add/Mul commutativity, distribution, and factoring.
The target closures contain 2, 10 and 18 Math e-nodes. No state is a hand-written
summary node. Complete bodies, port labels, value types and equality partitions
participate in state identity. An equivalent successor reuses its whole state ID;
a regression checks this with a cyclic `F(x) = x` body.

## Execution

Bake forks the actual parent saturation, adds new opaque typed parameters and
coarse context, and runs the original rules to a real no-change step. It obtains
the map of **all parent values** into the completed target, including values that
become equal. After rebasing the parent's rows through that map, it compiles only
the target's missing rows, constructor calls and required unions.

It verifies that delta independently by replaying it on a **parent** snapshot,
not by copying the already completed target engine. Whole-state comparison must
agree with native saturation before the library can be used.

At runtime:

```text
source StateId + stored full frame + new external bindings
  -> source/type/alias checks
  -> compiled constructor/union delta
  -> original native hashcons, union and rebuild
  -> typed native receipt of every target slot
  -> target StateId + new full frame
```

No target-body lookup or subgraph search reconstructs the frame. A write-only
native primitive receives the values computed by the macro RHS; after commit,
the runtime canonicalizes them and verifies the target and inherited equality
maps. It publishes the target state only after those checks succeed.

The original smooth rules stay installed. Their native query watermarks remain
unchanged across the hot path. The action-only API does one private worklist
match per macro; that is reported separately, not called zero total matching.
Constructor hashcons and union/rebuild costs also remain real runtime work.

## Reproduce

From the repository root, with no GPU worker or solver required:

```sh
cargo run --offline --release --bin egg_layout -- \
  whole-cell-check \
  experiments/whole_cell_chain/math.egg \
  experiments/whole_cell_chain/plan.json \
  experiments/whole_cell_chain/heldout.egg \
  out/whole-cell-abc

cargo test --offline --test whole_cell
```

Use a new output directory. The CLI performs native Bake, actual Cell execution,
an independent original-rule reference, and separate saturation/extraction
audits. Its artifacts include:

- `library.json`: complete Cell bodies, source/target IDs, alias guards, slot
  transport, emitted effect programs and native Bake completion evidence.
- `compiled-deltas.egg`: actual generated macro definitions. `wc_receipt_*` is
  supplied by Rust; the `.egg` is an inspection artifact, not a standalone file
  for an egglog binary lacking that primitive. The action API bypasses `wc_gate`
  matching and submits its known bindings directly.
- `original.egg`: the unmodified arithmetic rules, held-out input facts, and
  coarse grafts with ordinary native saturation after each graft.
- `A/B/C-native.json` and `A/B/C-cell.json`: **entire original-program graphs**,
  including input `Var` rows, not just the local Cell or selected projections.
- `report.json`: exact comparisons, original smooth match counts, macro match
  counts, original query watermarks, receipt widths and extraction costs.

## What establishes correctness

1. The macro execution starts in a fresh engine with held-out input identities;
   it is not the saturated Bake engine with a timer reset.
2. At each boundary, a separate native engine inserts that same coarse context
   and saturates the original rules. Exact typed graph comparison checks every
   constructor row, literal, visibility and equality class, not only counts.
3. A clone of the macro result runs the original matcher. It finds applicable
   matches, but its graph does not change. These **audit** matches are excluded
   from runtime counts and never feed the runtime.
4. Original-rule query watermarks in the real runtime do not advance. Actual
   native execution reports identify only the single-row macro actions.
5. Tests reuse one Bake with different concrete bindings and cover non-root
   grafts, old-slot merging by union, cyclic bodies, equivalent successors,
   wrong sources, unknown aliases, unsupported effects and extra-but-saturated
   graphs that must fail comparison.

## Scope

This is an executable CPU/native full-Cell backend, **not** a new GPU performance
result or a replacement for arbitrary eggcc schedules. No egglog kernel changes
are required for this implementation; it reuses the existing action sink.

It currently requires one positive constructor/union ruleset and complete
saturation boundaries. Runtime input forests must use constructors absent from
the rules' LHS, so active hidden contexts cannot invalidate the opaque-port
assumption. Unknown aliases fail explicitly; unexpected aliases after commit
poison execution rather than publish an uncertified state. Numeric guards,
custom functions/merge, subsume and finite `run N` substitution are rejected.
Native Bake can only certify a state after saturation finishes; this is not a
termination proof for generative/infinite rules.

All original facts are materialized. The demonstrated saving is **internal
smooth matching**, not RSS or virtual-fact storage compression. Coarse graft
selection is explicit in the plan; automatic discovery of arbitrary runtime
grafts and connection to the GPU transition selector remain separate work.
