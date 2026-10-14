# Native fact insertion: direct commands versus an empty-LHS rule

This is a standalone experiment on the **actual local native egglog runtime**.
It does not modify the engine, production replay, analyze, or the spine compiler.
The checkout contains other local kernel changes; this is not a pristine-upstream
comparison. Each workload/variant was run once, on one engine thread. No averages.

```sh
cargo run --offline --release --example replay_fact_batch -- out/replay-fact-batching
```

The command generates the compared `.egg` fragments and `results.json`. Rust
compilation is outside all measurements. File output and full-state verification
are also excluded. Both variants clone the same compiled environment; its setup
cost is reported separately. A small unrelated operation initializes runtime
infrastructure before either timed variant. This is not a statistical benchmark.

## What is compared

Direct top-level assertions:

```egg
(Add (Num 0) (Num 1))
(Add (Num 2) (Num 3))
```

The same assertions in one RHS:

```egg
(ruleset ReplayBatch0)
(rule () ((Add (Num 0) (Num 1))
          (Add (Num 2) (Num 3)))
      :ruleset ReplayBatch0)
(run ReplayBatch0 1)
```

`run_program(Vec<Command>)` still processes each top-level action separately.
In this engine, `eval_actions` creates a temporary backend rule, runs it and
frees it. A single empty-LHS rule groups these writes into one execution.
Declaring a ruleset alone does not insert any facts.

The experiment groups only consecutive expression assertions and stops at every
check, fail, let, set, union, declaration or schedule. All grouped expressions in
these fixtures are constructors. This is not a general-purpose transformation
of arbitrary expressions or mutable function lookups.

`mini_replay.egg` is the actual generated independent right-graft Cell 5 replay
from the graft-diamond test. It has six expression assertions but only its first
two RipenInput assertions form a contiguous group; the others are separated by
stage checks. The other workload has 1,024 independent Add assertions, generating
2,048 Num rows and 1,024 Add rows.

## One local run, 2026-10-15

Times in milliseconds. Total includes cloning, code generation, parsing, rule
installation and execution. Direct execution includes per-action compilation;
for the batch variant that compilation appears in the installation column.

| Workload | Direct total | Batched total | Batch installation | Batch execution | Direct execution |
|---|---:|---:|---:|---:|---:|
| Actual 6-fact replay | 1.036 | 0.814 | 0.026 | 0.683 | 0.877 |
| 1,024 Add assertions | 23.583 | 16.750 | 14.140 | 1.012 | 21.391 |

The complete constructor tables and equality partitions agree in both cases.
The actual replay has 6 constructor rows / 6 eclasses, including its two opaque
inputs. The bulk example has 3,072 rows / 3,072 eclasses. Counts reported as rule
matches count logical applications, not committed row mutations; direct action
execution is not accumulated in the scheduled-rule match report.

The observed bulk end-to-end improvement is about 1.41x, not the roughly 21x
ratio of execution slices. The latter excludes batch installation. Reusing an
already compiled batch may amortize that cost, but this experiment does not
measure such a cache or an end-to-end analyze speedup. The sub-millisecond replay
difference is especially sensitive to single-run noise.

## Ordering and eligibility checks

This valid stream must not be flattened into one initial insertion:

```egg
(N 0)
(fail (check (N 1)))
(N 1)
```

Grouping while preserving barriers passes. Inserting N(1) before its absence
check fails, as expected. The experiment also attempts to move a top-level set
and subsequent mutable Cost lookup into one rule; native typechecking rejects
the RHS function lookup. Thus arbitrary top-level commands cannot just be copied
into an empty-LHS rule.

## Implication for replay

There is a use for batching independent fact assertions **within a stage**.
The current preparation validator already groups checks, but its action slices
still use the per-command native path. Tiny replay stages offer little room.
Large staged injections or repeated reuse of a compiled action plan are the
promising cases. No stages, equality checks or external injection obligations
should be removed to obtain a larger batch. Production code is unchanged.
