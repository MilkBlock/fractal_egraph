# eggfuse

The Cell CPU compiler turns a flat egglog source program into a checked execution plan. The standalone CPU engine executes that plan and exports the resulting graph. Run these commands from the repository root with a stable Rust toolchain.

```sh
cargo cell-compile --case sum-bits --output out/cell-sum
cargo cell-run --program out/cell-sum/program.json --output out/cell-run
cargo cell-bench --program out/cell-sum/program.json --lane filtered --repeat 5 --verify-stages
```

Use fresh output directories. To compile your own program, replace `--case sum-bits` with `--source my.egg`. The compiler runs structural-key rewriting, native Analyze/Bake, marker selection and Recipe lowering. The runtime writes the final graph to `state.json` and execution counters to `report.json`.

For performance comparisons, run the same artifact with `--lane baseline`, `--lane nodes` and `--lane filtered`. The optional stage audit compares the selected CPU lane against the baseline on the initial graph and each Run's graph, bindings and schedule. See the [compiler and benchmark guide](tools/cell-compiler/README.md) for supported inputs and the known native endpoint discrepancy.

The [research snapshot](https://github.com/MilkBlock/fractal_egraph/tree/bf5b4539d09ad96473e40875868a8f4f02d6add5) contains the Metal engine, Python compiler and experimental benchmarks.
