# Slotted e-graph baseline

Pinned slotted-egraphs 0.0.36 versus the local egglog engine (trace disabled). Direct graph construction and rewrite execution. No SharedStore, no snapshot replay, no dictionary-compression ratios.

| case | roots | engine | nodes | live classes | root classes | retained KiB | peak RSS MiB | build ms | rewrite ms | rounds |
|---|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|
| ac | 1 | egglog | 54 | 15 | 1 | 806.18 | 6.42 | 0.917 | 0.378 | 5 |
| ac | 1 | slotted | 7 | 4 | 1 | 43.51 | 1.86 | 0.069 | 0.441 | 3 |
| ac | 16 | egglog | 864 | 240 | 16 | 1103.13 | 6.86 | 1.583 | 2.082 | 5 |
| ac | 16 | slotted | 7 | 4 | 1 | 44.73 | 1.86 | 0.171 | 0.468 | 3 |
| ac | 128 | egglog | 6912 | 1920 | 128 | 3108.00 | 10.06 | 6.686 | 13.152 | 5 |
| ac | 128 | slotted | 7 | 4 | 1 | 56.10 | 1.88 | 0.867 | 0.459 | 3 |
| constants | 1 | egglog | 5 | 5 | 1 | 649.48 | 6.11 | 0.910 | 0.000 | 0 |
| constants | 1 | slotted | 5 | 5 | 1 | 33.73 | 1.81 | 0.058 | 0.000 | 0 |
| constants | 64 | egglog | 320 | 320 | 64 | 854.53 | 6.31 | 3.373 | 0.000 | 0 |
| constants | 64 | slotted | 320 | 320 | 64 | 1189.82 | 3.30 | 1.007 | 0.000 | 0 |
| constants | 1024 | egglog | 5120 | 5120 | 1024 | 1412.35 | 7.42 | 39.637 | 0.000 | 0 |
| constants | 1024 | slotted | 5120 | 5120 | 1024 | 18811.07 | 25.42 | 15.187 | 0.000 | 0 |
| renamed | 1 | egglog | 5 | 5 | 1 | 649.48 | 6.11 | 1.247 | 0.000 | 0 |
| renamed | 1 | slotted | 4 | 4 | 1 | 31.21 | 1.81 | 0.117 | 0.000 | 0 |
| renamed | 64 | egglog | 320 | 320 | 64 | 854.53 | 6.30 | 3.719 | 0.000 | 0 |
| renamed | 64 | slotted | 4 | 4 | 1 | 37.30 | 1.81 | 0.450 | 0.000 | 0 |
| renamed | 1024 | egglog | 5120 | 5120 | 1024 | 1412.35 | 7.39 | 39.552 | 0.000 | 0 |
| renamed | 1024 | slotted | 4 | 4 | 1 | 134.80 | 1.97 | 5.567 | 0.000 | 0 |

## Interpretation and limits

- renamed: Add(Mul(a,b),Neg(a)); distinct free-variable names per input, with sharing of a across branches. constants uses the same shape with distinct numeric constants. ac starts from (a+b)+(c+d), applies add commutativity and both associativity directions to saturation.
- All external roots remain live. Slotted root handles contain their actual SlotMaps, and these allocations are included. Equal class IDs alone are never accepted as semantic equality; AppliedId mappings are checked.
- Constructor-node counts exclude egglog primitive i64 records but include Var/Num constructors. All engine allocations, indexes, union-find, caches, rules and root handles remain included in requested-byte measurements. Different implementations have different metadata costs.
- Retained/peak requested bytes are tracked with a System allocator wrapper, relative to the process argument baseline. They exclude allocator metadata and memory outside Rust System allocations. RSS is the whole process maximum, including subsequent correctness/statistics queries.
- Build time includes initialization, input generation, parsing and insertion via each public API. egglog also resolves/types expressions. It is an end-to-end frontend measurement, not an isolated data-structure operation comparison. No internal let function is created per root.
- Rewrite time includes each engine's actual matching, application and rebuilding until its own change/progress detector reports saturation (cap 32 rounds, failure rather than partial success). These engines need not use the same number of rounds.
- Correctness is checked after memory/time snapshots using read-only term lookup. AC verifies all 120 four-leaf binary trees/permutations for up to three input roots per measured run, plus a negative cross-root check. This is a scoped AC equivalence check, not general equivalence of all engine behavior.
- Unit tests with slotted internal checks enabled also verify cross-layer repeated-variable wiring, constant separation and binder alpha-equivalence without variable capture.
- These are three controlled workloads, not production compiler workloads. Small absolute RSS differences may be process noise; use requested bytes, node counts and scaling together. Slotted is not assumed to win for constants or every workload.

## Reproduce

```sh
cargo test --bin slotted_baseline --features slotted-checks
cargo build --release --bin slotted_baseline
python3 experiments/slotted_baseline/run.py
```

Source: https://github.com/memoryleak47/slotted-egraphs/ ; version and transitive checksums are pinned in Cargo.lock.
