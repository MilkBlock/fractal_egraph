# Extraction-specific cost markers in production Tiger

This changes the real Tiger extractor from `check_statewalk@ae80f82a`, using an
isolated source overlay. The upstream checkout and `egglog-baseline` are unchanged.
It is available through the existing extraction pipeline, not just a synthetic
matrix program. The default remains original Tiger; the new modes are opt-in.

## What changed

1. **Shared cost bags (`bag`).** Tiger's global greedy cost estimate retains
   the contribution of each already selected e-class. Copies now share immutable,
   sorted bags; combining bags performs an ordered merge, keeping the lower
   contribution for a repeated e-class. The child's remaining overhead is still
   charged exactly as before. This removes per-entry hash-node allocation and
   deep copies while retaining actual DAG sharing information.
2. **Cost recipes (`cpu` / `gpu`).** Effectful candidates with identical cost
   operations and ordered cost-slot bindings share one arithmetic recipe. The
   recipes retain repeated operands, If's max/min weighting and DoWhile's factor
   of 500. CPU or Metal evaluates them once, then scatters results back to the
   original candidate positions. CPU and GPU preserve Tiger's uint64 overflow
   behavior; no floating point or new tie policy is introduced.
3. The recipe program has a checked cost-only update method: only subscribers
   of changed cost slots are recomputed. This method is tested, **not connected
   to the live egglog event loop**. Topology/union changes require a new program.

These are **extraction-specific representations compiled from the actual frozen
extraction graph**. They do not consume existing matching-marker publications or
claim new RipSat closure coverage. Reusing those publications and performing
online global extraction remain separate work. FSE is not used. The Metal
backend is exact integer gather/arithmetic, not a GEMM/Tensor Core speedup.

The greedy traversal, candidate enumeration, priority queues, effectful Statewalk,
final pure extraction and tie-breaking remain unchanged. `--verify-markers`
checks every global greedy pick/cost against the original algorithm and every
candidate cost against the original evaluator. Full selected-DAG comparison is
still required separately.

## Real saved raytrace Pass 3, one run per mode

Input: `out/boundary-loop/graphs/gpu.json`, SHA-256 in `results.json`. Inference
was not rerun. Timings include the Tiger process, parsing, specialization,
extraction and output; build, Python input preparation and verification are
excluded. The GPU time includes its process startup, Metal compilation, binary
transport and result collection. No average or fastest-repeat selection.

| Mode | Global cost estimate | Candidate-cost stage | Whole Tiger process |
|---|---:|---:|---:|
| Original | 407.059 ms | 1.032 ms | 2.271746 s |
| Shared bags | **29.404 ms** | 1.019 ms | **1.735388 s** |
| Shared bags + CPU recipes | 31.069 ms | 2.045 ms | 1.646489 s |
| Shared bags + GPU recipes | 28.202 ms | 53.044 ms | 1.714332 s |

The targeted global estimate is 13.84x faster with shared bags. The observed
whole-process off/bag ratio is 1.31x. Do not attribute fluctuations in parsing or
other unchanged phases to markers. In particular, the CPU-recipe lane has a
lower whole-process time but a *higher measured candidate-stage cost*; it is not
evidence that the extra recipe stage accelerates this workload. Prefer `bag` here.

7,501 candidate costs become 2,709 unique recipes (5,099 cost-port reads).
The GPU kernel takes 28.083 us, but the GPU evaluation path takes 51.072 ms.
Only 271,944 binary request bytes are transmitted, without a JSON graph. This
batch is too small to justify the current process/compilation overhead. The
main demonstrated gain is CPU representation/sharing, **not GPU acceleration**.
Shared bags processed 62,983 merges, 8,831,164 input entries and 7,773,128 output
entries, with a largest bag of 858 entries. These are work counters, not RSS.

## Correctness and determinism

- All five outputs (off, bag, CPU recipes, GPU recipes, separate GPU oracle)
  are byte-identical and match the existing production Tiger output.
- Native reconstruction checks the full selected DAG: 8,007 rows, 29 functions.
- Separate oracle verifies all 18,054 global greedy costs/picks and all 7,501
  effectful candidate costs. Its extra time is not a speedup measurement.
- C++ tests check 4,000 random cost-bag combinations against Tiger's hash-bag
  arithmetic, 6,400 candidate costs against native, aliases/repeated operands,
  If/loop rules, uint64 overflow, cost updates, invalid shape, empty GPU input,
  worker failure and repeated exact GPU results.
- A standard `bench.py extract --extraction-markers bag` run also reproduces
  the previously saved native, CPU and GPU output files byte-for-byte. The
  **pre-existing fused-native versus CPU/GPU mismatch remains a failed gate**;
  same-commit CPU/GPU passes. This change does not claim to fix that mismatch.

Determinism is relative to the same input graph/order, cost model and Tiger
version. No claim is made about arbitrary nonconfluent inference schedules or
changing the native extractor's tie order.

## Use

From this worktree, for a saved run that does not already have `extraction/`:

```sh
python3 skills/cell-assisted-egglog-bench/scripts/bench.py extract out/MY-RUN \
  --extraction-markers bag --no-build
```

The same `--extraction-markers bag` flag works with `bench.py run`. `cpu` adds
recipe sharing; `gpu` evaluates those recipes with Metal. Overlay binaries are
built once into a content-addressed cache; source and backend enter validation
and cache identities. No change to the user's Tiger checkout is needed.

Full extraction-only ablation, including independent oracle and regression tests:

```sh
python3 skills/cell-assisted-egglog-bench/scripts/bench.py extraction-markers \
  --graph out/boundary-loop/graphs/gpu.json \
  --reference-output out/boundary-loop/extraction/gpu.egg \
  --build-dir out/tiger-marker-build-NEW --output out/tiger-marker-check-NEW
```

Unchanged, fully verified lanes are reused by exact input/binary/backend identity;
reused timings retain their original paths. The independent oracle is fresh.
Generated binaries, temporary graphs and raw transport are not tracked.
