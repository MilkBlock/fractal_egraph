# Adaptive local Cell runtime

`runtime.py` closes several gaps of the fixed two-stage prototype. It remains a
CPU/GPU local-closure runtime, not a replacement for global egglog execution.

## What now runs automatically

**Alias repair.** A GPU status-2 target retains its actual input coordinates.
The host groups equal inputs, compacts the slots, and Bakes the resulting symbolic
seed. The cache key is `(template, equality partition)`, not concrete input values.
After checking the specialized RHS, the repaired target is published immediately
and its consumers resume. A confirmed match is not rerun just to reconstruct it.
The same normalization also applies to aliased initial inputs.

**Exact reuse.** Both template and concrete-instance interning compare a complete
rooted closure. Classes are named by their grounded minimum expressions, including
cyclic closures. The key includes every exported constructor row and the selected
root. Hash lookup uses exact structural equality; it is not a fingerprint-only
proof. Ground keys are recomputed with actual input identities, so swapping
symmetric parameters can reuse an existing Cell. The same root expression with
different retained facts does not merge.

**Fact-driven routing.** The input supplies real binary constructor facts and
their child references. A worklist wakes a constructor when its children are
available, indexes applicable boundary rules by its root operator, and publishes
the resulting Cell. Publishing wakes its actual consumers. No hand-written stage
count or arbitrary Cell-pair enumeration is required. Duplicate constructor facts
and previously computed transitions are reused.

**Native fallback.** A constructor fact still exists when no compiled rule matches
it. The runtime retains its complete parent context and Bakes it natively instead
of dropping the fact. Unsupported GPU interface codecs also use this path. This
fallback currently assumes the same finite, grounded Math-constructor domain as
the experiment; it is not general set/function/primitive support.

**Input union.** An external input-identity union uses reverse input/descriptor
indexes to find affected Cells. Those Cells are re-canonicalized and specialized
if needed; stable old handles redirect to the new states. Reverse boundary/cache
indexes wake and invalidate affected work. Independent chains are left alone.
The external input-union registry is retained separately and included when
reifying for the native oracle.

**Batch layout.** Each frontier packs only its live source Cells and immediate
potential outputs. Bit width follows input IDs and slot width follows the current
batch; neither is fixed at the old 4 bits / 5 slots. Inactive output rows skip
feature arithmetic (their temporary tape is still zeroed). The backend uses exact
FP16 inputs with FP32 matrix results when the coordinate bounds permit it.

**Rule selection.** The adaptive mode discovers unconditional constructor rewrites
with a binary outer pattern and a variable shared across its two children. It is
not restricted to four hard-coded rule names. Other positive constructor rewrites
remain in native Bake and are reported with their lowering limitations. The
common-interface numerical codec is still restricted to Input/Add. Guards,
literal/primitive-bearing rules, set/relation effects, named rulesets and input
actions are rejected: native fallback alone cannot make erasing their data sound.
This is not an arbitrary `.egg` compiler.

## Execution boundaries

The fixed-program demo can execute two known stages in one GPU submission. The
adaptive path instead returns to the CPU between frontier batches to repair new
aliases, intern states, and route newly available constructor facts. It does not
pretend that these decisions are already a pure GPU program.

The current Metal executable is launched per batch, although compiled once. A
persistent backend, cached device programs, and incremental feature buffers remain
performance work. Native Bake and exact closure keys also cost CPU time. No
end-to-end speedup is claimed.

Input unions are not arbitrary unions of hidden internal e-classes. The runtime
does not maintain the original global fact tables, global congruence closure,
subsume/update visibility, or arbitrary cross-Cell value identity. Cell interning
only merges exactly equal **local rooted closure states** in one rule environment.

The input constructor DAG is supplied through the API. The worklist discovers
dependencies from that DAG; it does not infer all possible grafts or perform
automatic Cell ingress on every expression of an arbitrary `.egg` program.

## Validation workload

`adaptive_demo.py` supplies two overlapping factoring chains, a six-step
Integral/Cos–Diff/Sin chain, a power-combination chain, and a constructor with no
shared factor. It then unions two input identities and adds another fact after
the worklist has drained.

- 15 initial constructor facts and one late fact all resolve.
- 16 observed alias-target events are repaired and published.
- Alias specializations are reused across different actual input values.
- The union updates six Cells and five related constructor facts; the independent
  factoring, calculus, and power branches are not reprocessed.
- The calculus chain reuses the two repeated saturated states, while keeping the
  earlier Sin state distinct because it has fewer retained facts.
- A quiet second `drain()` does no new matching or Bake work.
- The CPU and real Metal/MPS paths produce exactly the same final rooted states.

There are 15 whole-state native checks before union, 15 after union, and 16 after
the late fact, per backend. The oracle starts from the original input expression
DAG and the external union log, not from the runtime's chosen target seed. It
compares all exported e-nodes and e-class partitions against reified Cell states.
These are local-context checks; they are not a proof of global egglog equivalence
for arbitrary interacting roots.

Additional tests cover new rule discovery, 33-bit IDs, slot-width growth beyond
five, repeated unions, symmetry reuse, the RHS-only negative example, invalid DAG
references, and interrupted-batch retry.

## Reproduce

```sh
python3 experiments/cell_spine/transition_program/adaptive_demo.py \
  --output out/adaptive-cell-next

python3 -m unittest experiments.cell_spine.transition_program.test_runtime -v
```

Use `--skip-build` after building the native examples, or `--cpu-only` without
Metal. The output directory must be new. `index.html` presents results;
`report.json` includes batch, fallback, alias, and native-audit evidence. Detailed
native exports and GPU programs are left untracked in that output directory.

## Remaining research questions

Bake plus markers makes a *certified local transition* executable. It does not
establish that useful template families stay small, that specialization is rare,
or that global boundary maintenance is cheaper than native joins. Exact interning
is also not free: the host currently constructs grounded whole-closure keys.
Those representation and amortization questions remain open even after the
correctness and continuation mechanisms below work.
