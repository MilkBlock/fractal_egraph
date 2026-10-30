# Parallel keyed Cell publication

This is the resident ABI7 eggfuse engine, not a detached GPU prototype. The
analyzer's exact bindings and typed recipes remain the input. It changes how
admitted effects execute; it does not shrink Bake or omit residual queries.

## Execution

1. Contiguous admitted contexts validate successor rows/guards against a frozen
   device state. Generated SSA code prepares the next effect with an exact key,
   result slot and continuation PC. No global table mutation occurs here.
2. A GPU hash grouping chooses the smallest request ordinal for each full typed
   key. Distinct keys execute independently. Duplicate keys share construction;
   pure i64 min/max conflicts reduce all proposals and the previous value.
3. Prefix sums reserve actual rows, IDs and union slots. Only key owners publish
   rows. Results scatter back to the original correlated contexts, so a later
   constructor/function key can use the actual hashcons identity.
4. The next wave advances those contexts without rematching their entry. Stable
   constructor witnesses avoid redundant lookups. Already satisfied effects do
   not enter the request queue. Completion certificates remain available to the
   existing exact matcher.
5. Deferred equality requests run in parallel at the block boundary. A separate
   flattening phase gives the next Run a deterministic minimum-root forest;
   congruence/function rebuild then uses the existing engine.

Admission is local to each recipe, not a whole mixed ruleset. Constructors,
constructor equality updates, explicit union and pure min/max/Unit-old functions
are admitted. General merge callbacks, destructive effects, containers and
unsupported numeric types retain ordered execution. A single context stays on
the scalar path because there is no inter-context parallelism. No candidate,
iteration or match budget is added.

Single-step recipes that qualify now use the same wave path. Otherwise about
2.4 million Pass-3 single-step invocations would still bypass the optimization.
Finite Run/Repeat outside an admitted saturation scope remains ordinary source
execution. Within an admitted scope, commutative effects may reorder; native
allocation order and intermediate physical history are not promised. Complete
facts, values, equivalence and visibility are checked where the controls agree.

Temporary packets are not a second graph cache. Live PCs/slots stay in the
existing context buffer; all authoritative facts remain in engine tables. Row
and packet allocation grow rather than dropping work. Scratch packets need not
be saved at a completed-Run checkpoint. Batch layout v8 exports 56 counters and
continues to use transport ABI7.

## Retained measurements

One run per lane, not a best-of or an average. Shader compilation, output and
checking are excluded from inference time. Diagnostic/audited executions are
not performance controls.

| Workload/configuration | Off | On | Observation |
|---|---:|---:|---|
| 32,768 independent contexts, current worker pair | 0.903 s | 0.358 s | About 2.52x; native full-state checks pass |
| Full Raytrace Pass 3, first same-worker wave pair | 198.070 s | 190.806 s | About 3.7% lower; entire 701,998-row graph agrees |
| Full Pass 3, adding admitted single steps and parallel union | 198.070 s historical | 185.707 s | About 6.2% lower; different worker, reused unchanged off path |

The last comparison is explicitly historical, not a new same-worker ablation.
The requested full-Pass-3 6.17-second objective is **not** achieved.

The final run still has 701,998 rows. Before/after raw identity counts differ
because unnecessary temporary IDs are avoided; this is not a Cell compression
ratio. The final equality-partition comparison against the disabled historical
control failed; its cause remains unclassified. Two checked full GPU executions
agreed for all 9,784 Run fingerprints/control and final state. These checks
are distinct; equal row counts do not establish endpoint equivalence.

The first wave version handles 1,534,555 already-satisfied effects without a
write request but only improves the full program modestly. The broader version
has 1,502,316 effect requests, 1,348,702 key groups, 33,950 duplicate-key requests
and 9,971 waves. Large independent batches benefit, while extra phase boundaries
and the remaining ordered callbacks limit end-to-end gain. No new occupancy or
bandwidth measurement is claimed from these timings.

## Reproduction

```sh
python3 experiments/cell_wave_publication/check.py \
  --build out/eggfuse-clean-paths-build --output out/new-cell-wave-check \
  --bench 32768

python3 experiments/cell_wave_publication/check.py \
  --build out/eggfuse-clean-paths-build --output /Volumes/Extreme\ SSD/eggfuse-profile/new-wave-pair \
  --pass3-core out/region-pass3-continuations/eggfuse_related_files/core.json
```

`--reuse-off DIR` validates the original typed input/control before reusing a
control. A changed worker requires explicit `--historical-worker` labeling.
Never relabel that result as a same-worker comparison. Every run asserts both
the compiled feature setting and nonzero actual wave work in admitted fixtures.

Focused cases cover independent contexts, duplicate constructors/function keys,
constructor-set equalities and identity feedback, and noncommuting fallback.
The existing staged-batch regression cases cover numeric guards, large i64,
late inputs and union continuations. The source-template unit tests check that
parallel proposal code contains no global write/row-lookup mutation, and that
noncommuting/custom merges are excluded.

Full artifacts remain under `/Volumes/Extreme SSD/eggfuse-profile/`:
`cell-waves-pass3-pair`, `cell-waves-pass3-final`, `cell-waves-pass3-audit`.
Small correctness/measurement summaries are retained here; no graph dumps,
shader binaries or checkpoints are tracked.
