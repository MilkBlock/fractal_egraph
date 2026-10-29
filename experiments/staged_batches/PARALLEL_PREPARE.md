# Parallel resident index preparation

The actual Raytrace Pass-3 Run 201 identified the bottleneck: 274.81 ms of
276.66 ms instrumented GPU time was in preparation, after union required new
indexes over 43,156 rows. Matching was 0.059 ms; publication was 1.255 ms.

The resident compiler now adapts the existing index kernels to its shared arena
ABI instead of creating a second indexing algorithm:

- Required full query-index rebuilds use stable radix sorting and parallel links.
  Physical row order within each probed column is preserved.
- Required full constructor/function-key indexes use parallel insertion and the
  minimum physical row for duplicate keys. Touched buckets are recorded so later
  sparse invalidation remains correct. Bucket placement itself is scratch state
  and need not be identical across executions; observable rows/IDs/history are.
- Epoch-tagged query heads are preserved across scratch reuse. Wrap and wide
  histogram overlap explicitly invalidate tags. Small graphs fitting one
  256-row block retain scalar construction; this changes no candidates or facts.
- Ordinary appends and output/timestamp patches retain the incremental path.
- GPU control publishes dispatch sizes. No intermediate match/count readback or
  host decision is inserted between stages. Matching storage doubles as radix
  scratch before matching; it is not a row-by-plan allocation.

The shared-prefix cache maintenance and RHS/block rebuild remain serial. This
change is index preparation, not stronger Cell proofs or a new closure semantics.

## Actual full-source Pass-3 window

Same saved source, input, schedule, capacity, numeric audit and per-Run hashing;
one execution per changed variant. Compilation and checkpoint IO are separate.

| Resident variant | First 200 Runs inference | GPU time |
| --- | ---: | ---: |
| Parallel ordering, serial indexes | 54.166 s | 54.090 s |
| Parallel query indexes | 45.221 s | 45.149 s |
| Parallel query and key indexes | **43.292 s** | **43.207 s** |

The final variant reduces this window by **20.1%**. Every recorded final state
field, counter and all 200 intermediate fingerprints equal the baseline. This
is a saved-window comparison, **not complete Pass-3 execution or its speedup**.
The worker host changed to connect the kernels; this is not labeled a same-binary
comparison. Reusing the unchanged saved baseline follows the benchmark policy.
Allocated device buffers increased by 8 MiB (1,060,246,084 → 1,068,634,692 bytes)
for scratch reuse/dispatch layout. This is not an RSS measurement.

Ordinary Run-201 replay GPU time was **294.70 → 118.65 ms**. Separate-command-buffer
diagnostics preserved complete state but perturbed timing (276.66 → 62.00 ms), so
their ratio is **not** the ordinary speedup. In the latter diagnostic, the shared
context/query finishing stage still used 42.86 ms. Future work should address
that maintenance and serial block rebuild/publication, not assume matching is
the dominant cost.

Artifacts are on the external disk under
`/Volumes/Extreme SSD/eggfuse-profile/`:
`order-pass3-200`, `index-pass3-200`, `prepare-pass3-200`,
`order-pass3-201-times`, `prepare-pass3-201-times`.

## Verification

- A 12,321-row fixture performs 32 unions, each enabling a repeated-variable
  consumer match. Serial, parallel and forced epoch-wrap variants preserve every
  state/history field; the expected 32 consumers are checked independently.
  Checked diagnostic inference: 1.638 → 0.953 s.
- All original 18 runtime contracts pass (17 successes plus the expected native
  and GPU overflow failure), as do the four checkpoint/restart cases.
- A new Set-valued-key fixture merges two Box keys after element union: 129
  surviving Box rows and 129 consumers, in two reproducible GPU executions.
- The 30 compiler/IR/reproducibility and 8 profiling tests pass.
- The new adapter source participates in compiler pass fingerprints; it is not
  an untracked dependency that can leave stale device artifacts in use.

```sh
python3 experiments/staged_batches/check.py --build out/eggfuse-clean-paths-build \
  --output out/index-check --index-comparison
python3 experiments/staged_batches/check.py --build out/eggfuse-clean-paths-build \
  --output out/index-contracts
```

Compile with the canonical `--multi-stage-cell` flow to include this path in new
artifacts. `resident_indexes.enabled` describes the new resident implementation;
the legacy compiler's `parallel_prepare` field still selects the older worker's
different layout and remains false here.
