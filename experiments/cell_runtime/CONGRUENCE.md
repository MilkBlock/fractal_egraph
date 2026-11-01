# Baked local congruence on the standalone CPU engine

This executes the real **standalone, single-threaded Cell CPU runtime**, not a
model of its final graph and not the native egglog kernel. Native egglog provides
the frontend and independent fixture oracle. The optimization is opt-in:

```sh
CELL_CPU_CONGRUENCE=1 python3 experiments/cell_runtime/check.py \
  --output out/congruence-check
python3 experiments/cell_runtime/congruence_bench.py \
  --output out/congruence-bench \
  --real-controls out/cell-sieve-join/final-controls
```

`eggfuse compile --backend cell-cpu` also publishes
`eggfuse_related_files/local-congruence-program.json`. The complete executable
program embeds the same proof data. Existing artifacts without it still load,
with no synthetic guarantees added. The runtime flag controls only this new
repair shortcut; it does not change the query or multi-stage switches.

## Representation and execution

The CPU engine already derives stable identities from `(instance, slot)` and
keeps immutable constructor wiring in shared recipes. This change retains that
representation; it does not allocate another per-row identity or rewrite IDs.

The analyzer interns constructor shapes, preserving constructor names, ordered
arguments, types and literals. Only repeated shapes within a producer template
are compared. It derives sufficient equalities on parameters, unfolds nested
constructor comparisons, and shares identical conditions across conclusions:

```
a = b AND the relevant constructor occurrences still exist
    => F(a) = F(b)
    => F(F(a)) = F(F(b))
    => ...
```

The independent Rust loader recomputes each implication from the source-checked
typed recipe, including its required constructor occurrences. It rejects forged
guards, missing requirements and invalid endpoints. Hashes do not prove equality.
No terminal saturated sample or rule name is used to authorize the result.

Union subscriptions enqueue only affected instances with compiled programs.
During the ordinary repair boundary, the engine evaluates exact parameter guards
and checks live, non-hidden carriers. Nested proofs share a carrier check. It
installs the proved unions in a batch, notifying affected dependencies once.
It then coalesces the proven **identical constructor keys** within that instance,
retaining the earliest local occurrence. Equal result classes alone never permit
deleting different constructor alternatives. Survivors still go through global
repair, including collisions with other instances and existing facts.

There is no early stage publication, extra source Run or additional rule match.
Hidden, deleted, unready or reused-but-not-owned carriers conservatively retain
ordinary repair. An unexpected interior alias also uses ordinary repair; these
conditions are sufficient, not an exhaustive list of possible equalities.

Only programs whose mutable functions all have pure i64 `min`/`max` merges may
use eager local propagation. Other callbacks may observe changed identities or
merge order, so the current gate disables this shortcut for the **whole program**.
It deliberately does not pretend to have proved arbitrary callback reordering.
This is the principal coverage limitation on eggcc.

## Results, 2026-11-01

One clean off/on run per workload, same release binary and compiled program.
Both use the default multi-stage execution and leave the optional Sieve and Join
intersection optimizations off. There is no averaging or best-run selection.
All final graphs and original Run records match the off control.

| Workload | Off inference | On inference | Off repair | On repair | Repair items off/on |
|---|---:|---:|---:|---:|---:|
| 256 independent pairs, depth 8 | 18.684 ms | 17.395 ms | 8.078 ms | 6.944 ms | 6,912 / 2,816 |
| 256 independent pairs, depth 32 | 78.543 ms | 68.561 ms | 32.643 ms | 25.916 ms | 25,344 / 8,960 |

Inference decreased by **6.9% / 12.7%**. Deep-chain repair decreased by **20.6%**.
These deliberately favorable workloads are not eggcc speedup evidence. Setup
(Graph construction) is separately recorded, about 1.4 ms. JSON parsing, program
verification, source preparation and export are outside inference; the wall
time includes process parsing/verification/export. Peak RSS is process-wide,
not a Cell storage-compression metric.

The deep case has one shared guard and 32 conclusions, used by 256 instances.
It installs 8,192 proved unions and coalesces 8,192 duplicate constructor facts.
The 512 instance visits include a second visit after its own unions; after local
coalescing that visit cannot reuse removed carriers. `unavailable_proofs` includes
these already-coalesced occurrences and must not be interpreted as missed matches.

| Real frozen Pass 3 | Requested off/on inference | Compiled templates / proofs | Actual shortcut |
|---|---:|---:|---|
| fact | 22.767 / 23.620 ms | 67 / 176 | disabled: merge safety gate |
| loopfact | 23.343 / 24.737 ms | 67 / 140 | disabled: merge safety gate |
| sum-bits | 166.844 / 167.807 ms | 67 / 179 | disabled: merge safety gate |

These real-program pairs execute the same repair path and have zero proof credit.
Their timing differences are retained, not reported as improvement or attributed
to local congruence. No new Raytrace timing was run: it has the same unsupported
merge class. Existing standalone/native Pass-3 discrepancies remain unresolved.

Initial versions only installed the proved unions and still sent all duplicates
through general lookup; they improved inference by only about 0–4%. The accepted
version additionally shares carrier checks, batches notifications and coalesces
proved local duplicates. Preliminary results remain under `out/` and are not
mixed into the final measurements.

## Verification and limits

50 small programs pass native full fact/value/equality/visibility comparison,
independent complete-LHS audit and fresh-process replay (48 in the main run,
two additional cross-boundary cases). The 11 congruence cases additionally check
every stage state and binding list against the same executable with the option
off. Tests include preexisting shared nodes, cross-instance collisions, unseen
interior aliases, hidden/deleted carriers, finite visibility, min merges and
ordered callbacks. Checkpoint/resume before the boundary union agrees with fresh
execution; malformed proofs are rejected by Rust without the Python validator.

Deleting an interior constructor while retaining its parents exposed an existing
native graph-comparison limitation: it reports an unmapped/orphan identity. That
trial had exact off/on stages and bindings but was **inconclusive against native**.
The accepted deletion regression removes a terminal occurrence so the native
oracle can anchor the complete result; the inconclusive trial is not counted.

The first scope is repeated constructor structure within one producer recipe,
not all cross-rule Cell chains. The artifact uses per-conclusion prerequisite
sets, so nested prefixes still occupy quadratic proof metadata in a long chain;
the runtime shares the all-ready check. Within a repeated shape group, pair
enumeration is also quadratic. No alias-partition search or heuristic budget is
introduced. Cross-template proofs, a shared prerequisite DAG and a finer proof
of callback independence are the next extensions; none is claimed here.

Reproducible text evidence is in `congruence-results/`; full graphs, programs,
checkpoints and preliminary trials remain untracked under `out/`.
