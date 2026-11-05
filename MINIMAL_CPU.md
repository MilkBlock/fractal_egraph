# CPU branch acceptance

The control is the research branch at `f985eb7` with the complete #34 configuration, ordinary storage, Raytrace-trained admission and the fixed 64-block iterative program. The minimal executable takes that same artifact with no execution switches. Source compilation with the two packaged recipe-bound catalogs was validated separately.

| Pass 3 workload | Research inference median, ms | Minimal inference median, ms | Candidate visits in each arm | Faster minimal pairs |
|---|---:|---:|---:|---:|
| sum-bits | 35.403 | 32.674 | 19,154 | 7/7 |
| raytrace | 15,074.794 | 14,586.906 | 7,263,030 | 3/4 |

Fresh release processes alternate order by round. One warm-up per arm is excluded. The time covers source-schedule execution in the independent Cell CPU engine. The graph export and report are outside the inference timer. Raytrace timings varied across rounds. These measurements check for a throughput regression in the trim. [Raw paired timings and state hashes](docs/minimal-cpu-results.json)

Both the old artifact and a clean recompilation of each named case reproduce the frozen logical graph, schedule hash, facts, function changes, unions and logical bindings. The audit compares the ordered RHS digest bytes for every Run and the entire winning Spine prefix tree. Sum-bits has 1,428 Runs, 3,034 resumed steps and maximum depth 12. Raytrace has 16,883 Runs, 663,403 resumed steps and maximum depth 29.

The runtime source now has 14,134 physical Rust lines in 37 files, compared with 22,083 lines in 48 files. Across the compiler library, engine directories and compiler command sources, the counts are 50,650 → 32,844. The vendored native egglog checkout is excluded. Formatting changes affect physical line counts.

The Rust workspace has one first-party lockfile. The vendored egglog source retains its upstream lockfile. The source suite and small optimization catalogs are sufficient for a fresh checkout to reproduce the fast configurations. Earlier benchmark results, diagnostic tools and experimental source remain available at the parent commit. [Research tree](https://github.com/MilkBlock/eggfuse/tree/f985eb7)
