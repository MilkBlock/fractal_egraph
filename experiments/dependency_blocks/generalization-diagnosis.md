# Native .egg generalization: initial diagnosis (874e9a2)

Historical record: the later repairs and current boundaries are documented in
`../native_programs/README.md`. In particular CYK witness retention and native
schedule execution have since been repaired.

These are actual egglog runtime checks on the master kernel, not a standalone
simulation or a claim of memory savings. The dpsk working tree is left intact.

## Verified cases

- Fibonacci's original seven steps pass with and without dependency tracing.
- Changing that schedule to 100 steps fails in both modes at the same step with
  `call of primitive + failed`. The i64 primitive uses `checked_add`; this is
  overflow from extending the workload, not evidence of a trace-only panic.
- A relation key `P (+ x 1) b` has complete physical witnesses in the small
  adjacent-pair fixture. Its seeded reads have no producer, but completeness
  remains true. Action masks still filter candidates that fail the primitive.
- CYK's first scope executes in command order, including its original post-run
  `let`, positive checks, negative checks, and extraction. Traced and ordinary
  execution pass those checks. This test explicitly stops at the first pop;
  it does not claim to exercise the other grammar scopes.
- CYK has 52 captured candidates and only eight complete witnesses. Temporary
  instrumentation located missing keys in `NonTerm`: body variables for the
  second and third nonterminals were absent from the retained bindings.
  This is not a general failure to evaluate arithmetic relation keys.
- Invalidated CYK writes observed here belong to tree constructors T/B/NT,
  not P. B and T have result sort tree, whereas P's third key has sort nonterm.

## Implemented kernel repair

Previously, any queued removal discarded all origins in that table, even when
its key did not exist. Serial and parallel deletion now invalidate only an
actually removed key. Table clearing still invalidates everything. Updated or
removed/reinserted rows do not inherit an unsupported producer.

In this first CYK scope the diagnostic run retained 74 producer-bearing reads
instead of 56, and emitted 16 invalidations instead of 23. These are read events,
not unique dependency edges, usable blocks, or performance improvements. The
incomplete witness problem remains. No full union lineage is claimed.

Regression: deleting an absent key invalidates nothing; deleting and untraced
reinserting one key loses that key's origin while another key retains its origin.

## Required next changes, in order

1. Preserve program semantics in the runner. Execute native commands in order;
   instrument schedules without replacing run counts, named rulesets, until,
   repeat, or saturation semantics. Preserve push/pop, checks, include/input,
   and post-run actions. Explicitly reject unsupported instrumentation rather
   than silently dropping commands. Reuse egglog's parser and runtime IDs.
2. Capture physical witnesses before planner elimination, or explicitly retain
   required key variables in diagnostic planning. Prefer atom/row witnesses
   from the join over reconstructing keys from a reduced substitution. Measure
   the overhead and keep uninstrumented planning unchanged.
3. Add union and rebuild lineage separately. Record committed unions and their
   causes; track old row versions to canonical row versions plus equality
   evidence. Coalescence can have multiple origins. Distinguish genuine
   deletion from canonicalization; never blindly preserve every old origin.
   Equality-enabled reads need equality evidence as well as row provenance.
4. Extend composition with typed literals, constructor applications, primitive
   expressions, and guards. Supporting a literal parser alone does not justify
   treating arithmetic as an e-node. Preserve primitive failures/overflow and
   side conditions. Keep opaque rules available for observation meanwhile.
5. Let the viewer show partial observed edges and exact rejection reasons, but
   distinguish those from certified prefixes eligible for shortcut execution.
   Do not relax `physical_witness_complete` into a proof of equivalence.

After union, finding only a block's entry fact again is insufficient to mark all
prefixes usable: revalidate the whole instantiated chain, guards, boundaries,
outputs, row versions, and dependency evidence. A shared canonical root alone
also does not justify merging block instances with different bindings.

## Reproduce

```sh
CARGO_INCREMENTAL=0 cargo test --manifest-path egglog/Cargo.toml -p egglog \
  --no-default-features --test primitive_dependency_trace --test dependency_trace
```

17 tests passed. No block compression or memory reduction is measured here.
