# Resident ordering and pipeline cache

This changes the actual resident ABI7 engine. Matching, complete bindings, source
effects and their total ordering are preserved; these are executor improvements,
not extra Cell proof coverage.

Up to 1,024 ordinals fit in a 4 KiB threadgroup array (four per lane). They are
sorted and packed in one kernel. Larger batches sort 256-entry tiles, then merge
them in parallel: each entry binary-searches the opposite sorted run to obtain a
unique output rank. The two order arrays already allocated by the serial sorter
are reused. Bindings themselves never move. The comparator, including its final
ordinal tie-break, is unchanged. All dispatch counts are produced on GPU; the
CPU only queues the fixed dataflow and handles allocation growth.

One arena can request growth while another has pending union continuations.
Growth now relocates descriptors, exact context slots and done bits as well as
certificates. The GPU rehashes certificates for the new capacity. The uneven-arena
regression uses quantum 1 to expose this boundary.

Single checked A/B runs (`out/resident-sort-release`), same source and worker:

| Bindings | Serial inference | Parallel inference |
| ---: | ---: | ---: |
| 4,097 | 162.12 ms | 115.21 ms |
| 32,768 | 1,476.92 ms | 998.44 ms |

The large case improves about 32%. Small cases are not uniformly faster: the
257-binding case increased from 17.58 to 21.84 ms. Earlier implementations with
separate small-batch dispatch were tested and replaced; their outputs remain in
`out/resident-sort-pair`, `out/resident-sort-final`, `out/resident-sort-shared`.
These are diagnostics, **not a full Pass-3 speedup**. Cases 0/1/255/256/257/513/
4097/32768 all preserve complete state and every Run fingerprint. `merge new`
checks observable publication order; the expected output pairs are independently
checked. All 18 runtime contracts and all checkpoint checks also pass.

```sh
python3 experiments/staged_batches/check.py --build out/eggfuse-clean-paths-build \
  --output out/order-check --sort-comparison
```

The worker also persists Metal pipeline binaries, keyed by complete shader,
device, OS build, compile settings and capture mode. An archive lookup must pass
Metal's `failOnBinaryArchiveMiss`; failures compile normally. Corrupt archives
are replaced atomically after successful compilation. `EGGFUSE_METAL_CACHE=off`
disables it; a directory value overrides the build directory's `metal-cache`.
Compilation remains outside inference time.

On the full Raytrace Pass-3 shader, cold setup was 135.24 s (32.92 s library,
78.09 s pack pipeline, 18.59 s publish pipeline); a fresh process with the archive
used **0.115 s**, with nine hits and zero misses. A newly instrumented executable
still paid 33.18 s for the library, but hit all nine pipeline binaries. The cache
does not promise to remove every Metal frontend compilation. Corruption, changed
shader identity, cache-off and reload cases preserve the exact executed state.

```sh
python3 experiments/staged_batches/check.py --build out/eggfuse-clean-paths-build \
  --output out/cache-check --pipeline-cache-program SAVED/program.json
```

Actual full-source Pass-3 execution was paused after 200 Runs: 54.17 s inference,
54.09 s GPU. This is not full completion. Next-Run phase diagnosis preserved all
state against ordinary execution: prepare 274.81 ms, match 0.059 ms, publish
1.255 ms, total instrumented GPU 276.66 ms. Prepare accounts for ~99.3%, following
a union that requires rebuilding indexes over 43,156 rows. Parallel preparation
is the next material target; the sorting microbenchmark must not obscure it.
The checkpoint and diagnostics are under
`/Volumes/Extreme SSD/eggfuse-profile/order-pass3-200` and `order-pass3-201-times`.
