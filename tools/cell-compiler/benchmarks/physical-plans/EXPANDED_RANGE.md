# Range-only validation on additional eggcc Pass 3 inputs

The input selection used the completed 96-workload production capture at eggcc `ae80f82`. Before Cell timings, we chose the two largest initialization fragments after Raytrace and a larger floating-point Bril program with a different algorithm. Deriche is a Rust PolyBench filter (6,562 initialization lets, 135,965 production endpoint nodes), 3mm is a Bril matrix-chain kernel (6,508 lets, 104,995 nodes), and conjugate-gradient has 3,400 lets and 39,849 nodes. The original runs used Tiger extraction to feed successive passes. These endpoint sizes are capture metadata. The timings below are new measurements of the standalone Cell CPU engine.

Each case uses the same frozen Bake input as the existing suite. Its full flat-source checksum was checked against the capture manifest and its original source against the pinned Git blob. The [suite manifest](../../../../experiments/profile_suite/suite.json) records the full and pre-flattening Pass 3 digests. Splitting out the common prefix and suffix preserves all input bytes.

## Admission at 32 rows

| Case | Historical candidates, off → on | Index build / update rows | Index maintenance | Inference off → on | CLI wall off → on | Faster pairs | Median paired inference change |
|---|---:|---:|---:|---:|---:|---:|---:|
| deriche | 1,437,068 → 1,432,911 | 6,023 / 1,680 | 3.14 ms | 5.287 → 5.058 s | 5.974 → 5.757 s | 2/5 | +116.0 ms |
| 3mm | 675,556 → 675,556 | 0 / 0 | 0.00 ms | 2.773 → 2.783 s | 3.322 → 3.317 s | 3/5 | -8.4 ms |
| conjugate-gradient | 303,741 → 303,741 | 0 / 0 | 0.00 ms | 1.184 → 1.197 s | 1.465 → 1.480 s | 2/5 | +0.6 ms |

One release executable per matrix, Apple M3 Max, one thread, one excluded warm-up per lane and five alternating fresh-process pairs. Both sides use planned Free Join, shared nodes, Sieve, column intersection, static aliases and first-resumption Spine with receipts. FD keys and scalar merge are off. The timed engine delta is the range switch. Inference covers the full schedule; CLI wall also includes artifact loading, setup and output of the full graph and report. It excludes compilation. Maintenance is inside inference and covers index construction and updates. The paired change is the median of the five on-minus-off inference times.

Only Deriche reached the admission threshold. It refuted 101 whole domains (4,157 input rows) with one index build and 100 index hits. Its off and on medians suggest a 229 ms difference, while three of the five paired calls took longer with range enabled. This sample does not establish a repeatable speedup. The two other inputs built no index. All three had zero partial reductions at the production threshold.

## Real mixed domains

An untimed observer inspected the first eligible single-column i64 guard and exact bound equality prefix on the original residual domain, also examining sizes 2 through 31. It counted a mixed call only when at least one original reference survives and at least one is excluded. It kept the production threshold and did not change matching or admission. Rows in this table count visits and may recur across calls.

| Case | Mixed calls with 2–7 rows | Mixed calls with 8–31 | Mixed calls with 32+ | Rows in mixed calls → exact survivors |
|---|---:|---:|---:|---:|
| deriche | 97 | 267 | 0 | 3,978 → 3,612 |
| 3mm | 76 | 242 | 0 | 3,491 → 3,173 |
| conjugate-gradient | 50 | 19 | 0 | 437 → 368 |

On 3mm, all 318 mixed calls came from `TypeList-ith` with `i > 0`. At Run 55 one input domain held indices 0 and 1: exactly the second survives. On conjugate-gradient, Run 42 included a domain with indices 0, 1 and 2 with two survivors. Deriche also supplied five mixed calls at `Add-All-Gets` with `pos < tuple-length`. Most of these small domains lose just one row.

A separate five-pair 3mm matrix changed only the temporary build's `MIN_DOMAIN` from 32 to 2 and tested range off/on. All 318 mixed prefixes now reached the index. Four actual intersections retained 4 of 8 input rows. The other 314 exceeded the existing half-domain collection budget and used the original domain. Another 915 domains were rejected whole. Overall candidate visits moved from 675,556 to 673,383, so only four of the 2,173 avoided visits came from the partial intersections. The two indexes read 3,144 build rows and 3,638 update rows. Median maintenance was 5.23 ms. Median inference went from 2.796 to 2.956 s, with one faster pair out of five. This supplementary matrix has its own off control and makes no change to the production cutoff.

## Correctness

Every sample matched its case's full graph export, schedule digest, bindings, committed-effect counts, resumptions and longest chain. In separate untimed builds, the per-Run SHA-256 digest of the complete ordered pre-apply `(rule ID, slot/value map)` sequence was compared element by element between off and on. The 5,687 Deriche, 4,260 3mm and 2,896 conjugate-gradient Run digests all matched. 3mm at cutoff 2 also matched the same 4,260 digests and graph endpoint. Intermediate full graphs and native-to-Cell graph equivalence are outside this audit.

## Reproduction

```sh
for name in deriche 3mm conjugate-gradient; do
  cargo cell-compile --case "$name" --emit-free-join-entry --output "out/range-$name"
  for modules in stage-spine,static-aliases stage-spine,static-aliases,range-guards; do
    cargo cell-bench --program "out/range-$name/program.json" --lane free-join-planned \
      --modules "$modules" --repeat 5
  done
done
```

Use fresh processes in alternating order to reproduce the paired protocol. The measured process-wall samples use the release `eggfuse-cell-runtime --program FILE --output NEW_DIR` binary directly with the same flags, excluding Cargo launch overhead. A Cargo recompile of conjugate-gradient reproduced the marked source and executable recipe/plans. The analysis provenance digest and enclosing artifact checksums varied between Bake invocations.

For ordered RHS validation follow [the runtime audit instructions](../runtime-modules/ALIAS_RECEIPTS.md), changing only `CELL_CPU_RANGE_GUARDS` between 0 and 1 and keeping FD keys and scalar merge at 0. Apply [the diagnostic shape patch](range-shape-audit.patch) with `git apply --unidiff-zero` after the RHS patch in a temporary checkout to reproduce the shadow counts. In `shape.json` the key contains rule, scan atom, guard atom, range column and size bin (0 = 2–7, 1 = 8–31, 2 = 32+). The eight counters are calls, input rows, exact survivors, mixed calls, mixed input rows, mixed survivors, empty calls and full calls. To reproduce the supplementary 3mm matrix change `MIN_DOMAIN` to 2 in an isolated checkout. Keep instrumentation out of timing builds. [Per-run measurements, counter definitions, examples and fingerprints](expanded-range-results.json)
