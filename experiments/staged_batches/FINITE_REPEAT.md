# Private finite and nested schedule completion in the resident backend

The new ABI7 backend now imports the existing typed fixed-carrier proof and
matrix executor. It gathers one context, executes the original compiled
Sequence/Repeat/Saturate on private GPU predicate coordinates, forwards results
to subsequent stages, then publishes fresh rows at the admitted region exit.
Repeat remains finite. Run inputs are gathered before its outputs are applied.
The CPU allocates buffers and dispatches a compiled region; it neither enumerates
bindings nor chooses its internal stages or supplies inferred facts.

Admission is deliberately explicit: structural carriers stay fixed; outputs are
positive unary predicates with opaque result identities. Escaping result IDs,
structural constructors, mutable function updates and union inside a region are
not admitted by this proof. They retain the new resident engine's general
recipes and exact residual queries. No source rule names are recognized.

A union **between** admitted regions is supported: the next region reads the
new canonical rows. The union-boundary regression caught and fixed initial
read-table version setup when the first operation is a private region.

## Verification and reproduction

```sh
python3 experiments/staged_batches/check_repeat.py \
  --build out/eggfuse-clean-paths-build --output out/private-repeat-v3
python3 experiments/staged_batches/check_repeat.py \
  --build out/eggfuse-clean-paths-build --output out/private-repeat-bench --bench
```

Seven cases pass repeated GPU execution and native complete fact/equivalence/
visibility comparison: Repeat 0/1/2; nested Repeat+Saturate; tree propagation
stopped before its root; escaping predicate identity; union between regions.
The ordinary 19 batch fixtures and four checkpoint cases also pass.

A single paired diagnostic with 256 inputs and **finite Repeat 28**:

| | Ordinary resident path | Private schedule |
|---|---:|---:|
| Inference seconds | 0.056101 | 0.020750 |
| Final rows | 7,424 | 7,424 |
| Joint region calls | 0 | 1 |
| Logical Runs inside region | 0 | 28 |
| Rows published at exit | 0 | 7,168 |
| Reads forwarded privately | 0 | 6,912 |

This is **2.70x on this microbenchmark**, not the Raytrace Pass 3 target.
The real current Pass 3 source admits `is-resolved` (17 rules); broad mixed
regions are still unfinished work. Full Pass 3 is measured separately.

## Diagnostics and storage

Private Run history carries cumulative fingerprints of newly derived fact
coordinates; it is not labeled as a hash of already-published global rows.
The ordinary path still fingerprints its resident rows. Both are checked for
repeatability in fresh processes, and native comparison independently checks
final semantic facts.

Exact-Run checkpoint/profile mode explicitly pins **ordinary schedule execution**
in its semantic program identity and checkpoint manifest. Resuming it retains
that choice; it never silently switches to a private region after restart.
The private executor currently publishes an entire admitted region atomically,
so its internal live matrices are not checkpointed. Such diagnostic timing is
not a benchmark of private completion. Multiple independent arenas likewise
retain the ordinary path for now.

The context contains predicate-by-live-ID coordinates and only actual marker
instances/dependencies. It is rebuilt on region entry, not on every private
stage. It is not yet an event-updated persistent context spanning distinct region
entries; reporting it as such would overstate the implementation.
