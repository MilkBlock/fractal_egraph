# sum-bits Pass 3: original-source native baseline

The baseline is the original `source.egg`, not `source_sk.egg` and not an older
Cell configuration. Its SHA-256 is
`29b8396e56c0a08bc11ca9e5d52afdc25c138e5e4731d2780484bb004b3686d1`, identical
to the frozen profile suite. The official compiler's source differs only by one
`cell-choice/v1` comment; that comment is absent from the measured original.

All four new measurements use one release executable, one CPU thread and the
same local egglog native runtime. It registers the source-declared structural
key primitives when required, then executes the unchanged source. No Cell
observer, recipe, or GPU is enabled. **This is the local fork's native execution,
not an independently rebuilt pristine `egglog-baseline` tag.**

| Native input | Inference | Relative time to original | Process peak RSS |
|---|---:|---:|---:|
| Original source | **52.144 ms** | 1.00x | 49.53 MiB |
| Structural keys (`source_sk`) | 61.740 ms | 1.18x | 61.80 MiB |
| Current markers (`source_sk_mk`) | 93.179 ms | 1.79x | 118.36 MiB |
| Markers with the Cartesian Get pair removed | 78.220 ms | 1.50x | 83.41 MiB |

Each lane ran once. Inference is the existing frontend's `--native-full`
`native_run_seconds`: all commands after declaration/fact bootstrap, which in
this source are schedules and checks. Parsing, bootstrap, compile/Bake, final
graph export and process startup/shutdown are outside that interval. RSS is
whole-process peak, including export, not the table-storage byte size.

Adding current markers takes 50.9% more time than the same native runtime on
`source_sk`. Removing the bad pair reduces that cost, but remains 26.7% slower
than `source_sk`. These are measured time differences, **not accepted equivalent
execution speedups**, because the cross-source graph check below has differences.

Historical same-machine Cell CPU times are retained, not rerun:

| Cell CPU input | Historical inference | Relative time to original native |
|---|---:|---:|
| Current markers | 520.016 ms | 9.97x |
| Cartesian pair removed | 217.336 ms | 4.17x |
| Context-filtered If pair | 224.555 ms | 4.31x |

The current Cell executable fingerprint still matches those historical records.
No no-marker Cell lane was run here, so this does not isolate all marker costs
inside that backend. The input/code fingerprints and measured-vs-historical
distinction are saved in `marker-shape-results/source-native-sum-bits.json`.

## Correctness and interpretation

The existing exact typed graph comparator checks original table rows, values,
equalities, and subsumption after excluding only newly introduced auxiliary
tables. It does not validate by row count alone.

- All native variants have 3,206 original-table records after projection.
- SK versus original: exact comparison fails (`different equality partition`).
- Marker versus SK: exact comparison fails (a `TermBop` row differs).
- Marker with bad pair removed versus current marker: exact comparison passes,
  including 3,206 rows and 2,193 mapped equality classes.
- Historical Cell CPU with the pair removed has 3,257 original-table records:
  154 `Smaller` rows versus native original's 103. Full comparison fails.
- Native original through the legacy driver and the shared-key-aware driver
  has exactly the same endpoint.

The selection rewrite and order-sensitive merges may explain some differences;
this measurement does not establish their cause, soundness, or extraction
equivalence. Tiger extraction was not run. The data establishes **no net speed
benefit over original native execution on this input**, and no new semantic
equivalence claim. Repairing one expensive marker is not evidence that markers
as a whole are beneficial.

## Reproduction and provenance

`native_source.rs` is a small adapter around the existing native frontend and
the shared structural-key registrar. The first legacy `eggfuse-cpu --mode native`
run measured original inference at 59.737 ms but could not register
`CellStructuralKey` for SK inputs. Those failures and the original measurement
remain in `out/source-native-marker-bench/measurements.json`. They are excluded
from the matched table above; the replacement runner was run once per lane,
including original source. This is a change of harness, not averaging or choosing
the fastest sample.

To avoid rebuilding the full analysis crate, the adapter was compiled with
`rustc --edition=2024 -C opt-level=3` against the existing release `egglog`,
`serde_json` and `eggfuse_structural_key` rlibs. The complete command and dependency
hashes are recorded in the compact JSON. The benchmark invocation is:

```sh
/usr/bin/time -l NATIVE_SOURCE SOURCE.egg --native-full --export-to NEW_DIRECTORY
```

The linked engine artifacts were read from the main workspace; no main-workspace
source or build artifact was modified. Raw graphs, logs, commands, two-driver
checks and comparisons are in `out/source-native-marker-bench/`. Only adapter
source and compact text results are tracked. No raytrace workload was run.
