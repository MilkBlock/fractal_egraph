# Cell CPU toolchain

`cargo cell-compile` accepts a named workload or a flat egglog source. It rewrites structural keys, analyzes a training schedule, selects structural markers, lowers the marked source and validates the CPU artifact. Training and target declarations and rules are checked for compatibility. Compilation publishes into a fresh directory after validation.

`--bake-source FILE` supplies a separate training input for a custom source. `--profile FILE` and `--marker-locality identity|all` control offline marker selection. An optional `--catalog FILE` attaches recipe-bound Spine blocks, iterative transitions and admitted query contexts after lowering. The catalog's recipe hash and each executable component are checked. Changes to marker selection can produce a different recipe; supply a matching catalog in that case.

`cargo cell-run --program FILE --output DIR` executes the complete source schedule and writes `state.json` and `report.json`. The report separates inference from setup and export. For small inputs, `--audit-stages` saves every logical graph and binding snapshot. `--inspect-program` validates an artifact and reports its admitted context plans.

`cargo cell-bench --program FILE --repeat N` starts a fresh graph for each repetition, checks the final graph digest and reports inference samples and their median. It can also compile `--source FILE` or `--case NAME` before the timed runs. `--reference-program FILE` adds per-Run comparison before timing. `--lane` and `--modules` belong to the research CLI.

For ordered RHS digests and the full winning Spine prefix tree, use a separate audit process.

```sh
cargo run --release --locked -p eggfuse-cell-runtime \
  --features slot-transfer-audit,spine-shapes -- --program out/sum-bits/program.json --output out/audit
```

To learn blocks for another workload, run a training artifact with `spine-shapes` to collect observed prefixes. Create a seed using `cargo run --release --locked -p eggfuse-cell-runtime --example bake_spine_blocks -- TRAIN_PROGRAM TRAIN_SHAPES TARGET_PROGRAM NEW_PROGRAM 32`. Use `cargo cell-iterate --program NEW_PROGRAM --output out/learned --rounds 4 --objective rhs-lookups` to grow bounded adjacent units. The iterative command checks ordered RHS inputs across rounds. The separate `bake_contexts` example can transfer admission evidence from paired discovery profiles collected with the research executor.

The frozen suite uses marked recipes produced by the compiler. Per-Run equality on this branch is measured against the prior standalone Cell CPU executor. Comparisons against native egglog require its original unmarked eggcc input.
