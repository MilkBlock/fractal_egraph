# eggfuse components

The project entry is `python3 -m eggfuse`. Run it from the repository root.
Existing `egg_layout`, `tools.eggfuse` and backend experiment commands remain
compatible. Native Rust modules currently remain in `src/`; the crate split in
[the architecture plan](../docs/architecture/eggfuse.md) is a later step.

| Component | Current implementation |
|---|---|
| Analyzer | `analyzer/` entry/adapters and native `src/native_*`, `src/cell_recipe*` |
| Shared contract | `ir/recipe.py`, `ir/library.py`, `ir/interface.py` |
| Compiler | `compiler/` passes and `compiler/recipes/`; target lowering in `compiler/targets/metal/` |
| Engine | `engine/` artifact validation/entry; actual Swift and Metal in `../engine/metal/` |
| Legacy host/validation harness | `../experiments/device_backend/`; migration is incremental |

```sh
python3 -m eggfuse analyze --source input.egg --output out/analysis
python3 -m eggfuse compile --source input.egg --output out/compiled \
  --build out/device-build --solver-python /path/to/scip-python
python3 -m eggfuse inspect-recipe out/compiled/recipe-program.json
python3 -m eggfuse run --artifact out/compiled/eggfuse_related_files \
  --build out/device-build --output out/run --check
python3 -m eggfuse run-cells --library out/analysis/recipe-library.json \
  --build out/device-build --output out/isolated-cells
```

`analyze --analyzer-bin PATH` selects a native analyzer; otherwise the compatible
`target/debug/egg_layout` binary is used. `run --artifact` uses its manifest's
execution options and the emitted program; it does not run device lowering.
The artifact runner currently accepts the ordinary `gpu` backend. Isolated
libraries use `run-cells`; the compiler's `--backend local` reference mode
remains supported by its integrated verifier.

The shared IR expresses query bindings, typed slots, effects, original control,
conservative dependencies and event routes. It rejects altered wiring/scope or
contracts inconsistent with the typed source. It does not claim that all those
programs have an efficient specialized lowering. Generic execution still does
local queries; optimized terms regions retain their existing separate typed
admission proof. Event routes are descriptive until a runtime subscription and
residual-query implementation is certified.

Real eggcc context rules have been checked through this IR, including growing
constructors, constructor set, union, finite Run, late union, renamed symbols,
and context-to-terms execution. Actual native Analyze exports are also checked
in fresh GPU processes. See [results](../experiments/recipe_ir/README.md).
