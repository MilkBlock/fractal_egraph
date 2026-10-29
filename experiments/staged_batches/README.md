# Resident multi-stage context batches

This is an execution path in the actual eggfuse Metal backend, tested on small
programs. It is not a whole-Raytrace speedup result or a proof of CPU endpoint
completeness. The acceptance contract is sound rule applications and reproducible
GPU rounds, as requested.

## Execution path

1. The analyzer composes typed rule invocations with unique symbolic wiring
   inside eligible Saturate scopes. A program may fork to several consumers;
   ambiguous wiring and recurrence backedges remain in the residual matcher.
   Finite Runs/Repeat do not authorize arbitrary lookahead.
2. A seed match supplies one correlated context. Numeric/boolean inputs,
   constructor slot recipes and future parameter updates share this context.
   Instances with the same recipe are packed into tiles of at most 128 rows.
3. A GPU control stage publishes tile descriptors and an indirect dispatch size.
   Many threadgroups evaluate these tiles. CPU code does not enumerate bindings
   or fetch intermediate results between their local stages.
4. Affine outputs use real Metal `simdgroup_float8x8` multiplication when the
   input/intermediate bounds prove exact FP32 integer evaluation. The current
   conservative domain is |numeric seed input| <= 256, with all affine
   intermediate bounds <= 2^22. Other values, including 10^16, use checked i64
   arithmetic in the same context. Min/max and guards remain exact operations.
   These bounds select a numeric implementation; they do not prune matches.
5. The block publisher resolves constructor slots through shared hashcons and
   applies all effects in seed-binding/step/action order. Pure min/max function
   updates combine by their actual keys. This publication is currently ordered,
   not a claimed parallel hashcons or segmented-reduction implementation.
6. Each proposed successor checks its complete concrete rows and guards against
   actual state before publishing effects. A proposed set value which loses its
   merge is not evidence. Pending unions defer successor execution. Ordinary
   union/custom-merge/subsume rules remain on the exact residual action path.
7. Successfully completed, immutable single-atom consumers receive completion
   certificates. A later query can skip the certified binding before evaluating
   its remaining conditions. Union and visibility generations invalidate these
   certificates. The restriction is deliberate: no blanket 'Cell is done' flag
   is attached to mutable inputs or branching queries.

Three device stages share the same resident storage; they are not a per-rule
interpreter split into extra launches. The CPU queues a fixed dispatch quantum,
then only checks completion/allocation status. Context/certificate arenas grow
from actual demand and retry before RHS; already committed logical rounds are
not replayed. Original table storage still receives block effects on GPU.

## Soundness and reproducibility checks

- Source Recipe IR, scope membership, types, SSA slots and producer dependencies
  are independently validated before loading. Arbitrary reassignment of a live
  slot is rejected even after re-signing the JSON.
- Seed matching uses the existing exact GPU matcher. Every inlined successor
  rechecks actual keyed rows and guards. Full correlated bindings are retained.
- `--check` enables a source-i64 numeric audit before effect publication. A
  mismatch fails the run; it is not silently repaired from a CPU endpoint.
- `--check` also executes a fresh GPU process twice and compares every logical
  round's control/state fingerprints and the complete final rows, values,
  visibility, IDs and runtime counters. Fingerprints are finite checksums,
  not a mathematical proof against collisions. Fixed publication order provides
  the deterministic execution discipline.
- Full table hashing is diagnostic only; normal execution does not pay that
  per-round scan. Numeric proposal/witness validation still applies as described
  above. CPU whole-graph equivalence is not required by the region pipeline.

## Results

Nine fixtures each pass two fresh-process GPU executions: wide signed affine
inputs; contended min keys; exact min columns; 10^16; finite Run; losing min
proposal; an already-bound arithmetic guard; masked overflow; union/subsume.
The wide fixture has 513 independent seeds and three source stages:

| Work | Executed |
|---|---:|
| Context evaluation tiles | 5 |
| Seed invocations | 513 |
| Witness-checked local successor invocations | 1,026 |
| Completed-binding matcher shortcuts | 1,026 |
| Complete batch effect requests | 2,565 |
| Context rows using the matrix path | 513 |
| Exact numeric audit checks | 1,539 |

The per-Run history match column counts seed/residual matcher outputs; accepted
inline invocations are reported separately. Their sum counts executed rule
invocations, not committed mutations. Effect requests are also not
committed-mutation counts. A shortcut count records a
successful certificate check, not a distinct Cell count or a timing saving.
The reported workload has no remaining ordinary action invocations, but seed
matching and successor witness validation still perform reads.

```sh
python3 experiments/staged_batches/check.py \
  --build out/eggfuse-clean-paths-build --output out/new-staged-batches
python3 -m eggfuse compile \
  --source experiments/staged_batches/chain.egg --output out/new-batch-pipeline \
  --build out/eggfuse-clean-paths-build --solver-python /path/to/scip-python \
  --multi-stage-cell --capacity 256 --match-capacity 1024
python3 -m eggfuse run --artifact out/new-batch-pipeline/eggfuse_related_files \
  --build out/eggfuse-clean-paths-build --output out/new-batch-run --check
```

Seed discovery, cache refresh and stable publication are still serial within a
shared arena. Evaluation now scales across threadgroups. General branching
composition, broader merge-program lowering and fully parallel keyed publication
remain unfinished. No end-to-end 2x or full GPU occupancy claim is made.
