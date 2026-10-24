# Stable selection and production extraction repair

The final-result gate now passes. The source pipeline emits explicit structural
tie-breaking and late equal-cost improvement. Candidate keys are stored in native
Rust shared DAG values and read through engine-managed candidate views, so union
and key updates retain normal incremental dependencies. The old left-biased
selection closure is rejected rather than silently reused.

The second source of divergence was Tiger's input-order sensitivity. The new
Rust adapter is now called from official eggcc's serialization boundary. It uses
constructor-schema reachability, required typing/debug metadata, and deterministic
structural ordering. Missing historical keys do not fall back to allocation IDs.
Tiger's C++ algorithm, the matcher, and the ordinary visualization serializer are
unchanged. Earlier unmodified passes retain their original export by default.

## Checks

- 477,053 Cell query frontiers pass independent native checks.
- 5,068 keyed Expr selector roots and 48 keyed hoist roots agree. Another 2,354
  Expr roots lack ranking keys and are explicitly outside that ranking metric.
- The non-Term public projection agrees exactly: 412,072 rows and 319,063 mapped
  classes. This is a diagnostic projection, not a claim that all intermediate
  tables are identical.
- The production Rust export generates identical 53,624-node / 40,331-class
  inputs. Unchanged production Tiger selects identical 29-function / 8,032-row
  DAGs, checked by reconstructing all selected operators and connections.
- The actual official three-pass runs both load the expected Pass-3 artifact.
  Native SK and Cell MK produce byte-identical final printed IR:

```
ca52f060fbf1d95632c807b07f59d12da735ae42fa660877ec88e73a3ecb6960
```

The Cell lane performs 11,124 partially Cell-served Runs and supplies 2,967,639
complete bindings. These are logical query results, not counts of committed
mutations. It is not an all-native fallback.

The layout is the broad source-derived correctness configuration, not a freshly
profiled solver optimum. Run durations are retained in the JSON as diagnostic
observations; no optimized speedup is claimed. No full workload profile was
repeated for the export change. Completed profiles are now cached independently
of output directory and solver selection; interrupted profiles are never priced
as complete.

Implementation and reproduction entrypoints live in the local
`cell-native-egglog/cell-extension/`: `pipeline.py`, `STABLE_EXTRACTION.md`,
`examples/stable_export.rs`, and `diagnostics/selection_layout.py`.
See [stable-results.json](stable-results.json) for exact records and local paths.

The separately authorized export adapter adds a wrapper/call and a read-only
function metadata accessor beyond the original source-dispatch entry. This is
not described as a one-line total implementation.
