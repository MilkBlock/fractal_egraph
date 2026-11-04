# Reusing compiler-proven constructor results in lazy RHS frames

The existing `known_results.static_aliases` plan identifies constructor calls whose full input key already has a result in the matched LHS or an earlier RHS instruction. The loader rederives the proof from the typed recipe. With `CELL_CPU_KNOWN_RESULTS=static`, the lightweight prefix now consults that plan before building the key or requesting the constructor. An aliased output slot reads its certified source through the same slot resolver as the ordinary RHS. Unproved prefix operations still perform exact graph lookups.

An all-hit RHS reserves its Cell ID and returns without registering the frame. At the first actual miss it stores the original frame, inserts the constructor once and continues with the ordinary RHS at the following instruction. This preserves later eclass IDs, parent-projected inputs and alias chains. The normal eligibility checks remain in force for Spine cursors, witnesses, reference storage, union lineage, local saturation and other effectful paths. The existing static mode is opt-in. The integration changes no proof or artifact schema.

## Two frozen Pass 3 inputs

- **Lazy** uses `6eb75fd`, static aliases off and the lightweight constructor-only frame policy.
- **Static** uses `6eb75fd` with static aliases on. That version stores an eager frame for every RHS in static mode.
- **Combined** uses the new implementation with static aliases on and lightweight frames where eligible.

| Input | Constructor policy | Requests | Physical Cells created | RHS frames avoided | Inference median |
|---|---|---:|---:|---:|---:|
| sum-bits | lazy | 17,627 | 9,492 | 966 | 75.22 ms |
| sum-bits | static | 11,774 | 10,458 | 0 | 73.97 ms |
| sum-bits | combined | 11,774 | 9,492 | 966 | 75.51 ms |
| raytrace | lazy | 3,318,743 | 3,741,283 | 134,085 | 34.01 s |
| raytrace | static | 2,096,703 | 3,875,368 | 0 | 33.46 s |
| raytrace | combined | 2,096,703 | 3,741,283 | 134,085 | 34.65 s |

All runs used the standalone Cell CPU executor, first-resumption Spine with anchor receipts, shared entries, Sieve, column intersection and planned Free Join. Each case used the same frozen program in each lane, one excluded warm-up per lane and rotated measurement order, with seven measured rounds for sum-bits and three for Raytrace. The timer covers schedule inference. Loading, export and audit are outside this window.

On Raytrace the combined policy preserves both the 1,222,040 eliminated constructor requests and the 134,085 avoided physical frames. Across all three lanes the original schedule executes 3,785,196 RHS bindings, creates 820,439 logical facts and performs 226,523 effective unions. Sum-bits eliminates 5,853 requests and avoids 966 frames. Candidate visits remain 9,522,225 and 15,549 respectively. Raytrace resumes 663,403 steps with maximum chain depth 29. Sum-bits resumes 3,034 with depth 12.

On Raytrace the median RHS phase falls from 10.08 s with lazy frames alone to 9.63 s with the combined policy. The end-to-end median goes from 34.01 s to 34.65 s, with the combined lane faster in one of three paired rounds. On sum-bits the two end-to-end medians differ by 0.29 ms. These samples establish the work reductions and do not demonstrate a throughput gain.

On both workloads the skips are in the ordinary RHS loop. The combined lane recovers frame elision for a different set of applications. A focused regression exercises actual alias reuse in the unregistered prefix, including one full hit and first misses at two different positions. It eliminates nine requests, five in the lightweight prefix and four in the suffix, and reaches the same stage graphs, bindings and Cell identities.

A skipped constructor produces a result slot without a fresh action receipt. Raytrace capture fallbacks increase from 29 to 140,110 in both static lanes. Total graph key lookups consequently fall from 5,619,821 to 4,537,862, a net reduction of 1,081,959. On sum-bits the corresponding fallbacks rise from 58 to 1,110, and total key lookups fall from 25,041 to 20,240.

## Validation and pure Cargo reproduction

The worker reports `constructor_requests`, `static_eliminated`, `lazy_prefix_eliminated`, `cells_created` and `rhs_frames_avoided`. Each timed run matches its frozen full graph endpoint and per-Run schedule and binding counts. Separate untimed binaries recorded SHA-256 over each Run's ordered sequence of `(rule ID, slot/value map)` records before RHS execution. All three arrays agree element by element across 1,428 sum-bits Runs and 16,883 Raytrace Runs. Intermediate full Raytrace graph snapshots were outside this audit.

```sh
cargo cell-compile --case sum-bits --emit-free-join-entry --output out/static-sum
cargo cell-compile --case raytrace --emit-free-join-entry --output out/static-ray
cargo cell-bench --program out/static-sum/program.json --lane free-join-planned --modules stage-spine,static-aliases --repeat 7 --verify-stages
cargo cell-bench --program out/static-ray/program.json --lane free-join-planned --modules stage-spine,static-aliases --repeat 3
```

For the control modes, run the same artifact with the runtime CLI in a separate checkout at `6eb75fd`. Set `CELL_CPU_KNOWN_RESULTS=off` for lazy or `static` for static-only. Use a fresh output directory for each sample. The combined run on this branch uses `static` as well.

```sh
CELL_CPU_KNOWN_RESULTS=static CELL_CPU_STAGE_SPINE=1 CELL_CPU_STAGE_SPINE_RECEIPTS=1 \
CELL_CPU_SHARED_NODE_SCHEDULE=1 CELL_CPU_SIEVE=1 CELL_CPU_JOIN_INTERSECTION=1 \
CELL_CPU_FREE_JOIN=planned cargo cell-run --program out/static-ray/program.json --output out/static-trial
```

For the RHS audit, apply [the diagnostic patch](root-rhs-audit.patch) in temporary copies of both revisions. Use the same CLI invocation with `CHECK_RHS_DIGESTS=1` for each policy and compare the three `rhs.json` arrays with `cmp`. [Raw samples, counters and fingerprints](static-rhs-results.json)

The follow-up carries the original matched fact or earlier action receipt into Spine for skipped constructor anchors. [Forwarded receipt comparison](ALIAS_RECEIPTS.md)
