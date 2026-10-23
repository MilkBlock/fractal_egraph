# Runtime-priced spine markers

Use the versioned skill entry point, not a new collector or benchmark script.

```sh
python "$SKILL/scripts/bench.py" planner-corpus \
  --profile-binary /absolute/path/to/eggcc-schedule-profile \
  --output out/planner-corpus
python "$SKILL/scripts/bench.py" planner-prices \
  --input out/planner-corpus/manifest.json --output out/planner-prices.json --jobs 8
python "$SKILL/scripts/bench.py" spine-program \
  --runtime-workload out/planner-prices.json \
  --query-selection experiments/pass3_parameterized_reuse/combined.json \
  --compile-only --output out/runtime-spines
```

`planner-corpus` defaults to every `.bril`/`.rs` in the production repository's
`benchmarks/passing`. `--source FILE` (repeatable) explicitly selects a smaller
development corpus; never describe that as all-sample evidence. Production
extraction creates each subsequent pass's input. The target native engine
replays each flat pass, measuring every Run, including Runs inside Saturate.
It tries every admitted constructor-root fixed query plan, compares complete
bindings against the native query, and only then executes the original Run.
It does not execute hypothetical marker RHS actions during profiling.

Profiles contain table sizes/updates, root evaluations, per-stage lookups,
candidate row visits and correlated prefix counts. They contain no binding
trace. Unsupported queries remain in `rejected`, not as zero-cost plans.
Unsupported `until` boundaries are recorded and prevent a complete pricing
dataset. This is diagnostic target-engine replay of production inputs, not
production timing, GPU timing, or original Free Join stage attribution.

`--resume` verifies tool/input fingerprints and reuses completed work. Do not
rebuild the active `planner_profile` binary or change inputs mid-capture. A
partial gzip remains excluded until successful process exit and a complete
footer. Inspect failed cases before `--resume --retry-failed`; previous partial
files/attempt records are retained and completed passes are not rerun. On macOS
the resource log includes exit/signal and peak RSS, separate from performance
timing. Never delete failures to make the corpus look complete. A log line
`[N/N]` means the final case started; only the
manifest `complete: true` confirms success. No automatic timeout or candidate
sampling substitutes for missing work.

`planner-prices` streams all completed profiles, preserving query identity,
types, slot wiring, constants and guards. Private plan work is measured; shared
prefix prices use unfiltered prefix census work and peak live cardinalities.
These shared prices are work estimates, not elapsed-time fits or proven bounds.
Same-prefix maintenance is coalesced across consumers at the same table version
using their maximum observed work. The exact union of their dirty-root sets is
not recorded, so this can differ from actual shared-cache maintenance.
The solver selects among measured roots, fixed orders, prefix lengths and
payloads; it does not claim a globally optimal join algorithm. Unmeasured or
empty-work queries retain the native path. The output records corpus and input
hashes, pass/round counts and per-table stage work. It cannot be combined with
old single-snapshot/static pricing options.
`--jobs` parallelizes independent pass readers. It does not sample or omit any
workload; serial and parallel aggregation use identical integer sums and maxima.

Use `boundary-credit --layouts` after the usual same-binary `run` pair, with
separate diagnostics for depth distributions. Keep CPU provider maintenance,
GPU candidates and whole-graph/Tiger checks separate. Use fixed-query plans as
the control; a smaller objective value is not a measured speedup. Do not time
while collection, compilation or another run is active.

## Lower selected parameter programs to source markers

```sh
python -m tools.cell_spine.compiler --source SOURCE.egg \
  --selected-layouts out/runtime-spines/program/boundary-programs.json \
  --source-markers --output out/source-markers --skip-build
```

This native-checks the existing layout and shares its structural prefix in
`source_sk_mk.egg`. Current function/global values and guards remain consumer
conditions. RHS values can publish successor markers directly, with one-time
primitive evaluation and unchanged original Run boundaries. Mutable selected
roots retain their original consumers. View-completion helpers remain necessary
for new input and union-enabled combinations; do not claim all maintenance
joins disappeared. `markers.json` lists converted and retained consumers.
`runtime-layout.json` excludes rewritten consumers to avoid running them twice,
but retains private or mutable-root plans whose source consumers were unchanged.
When the solver selects no materialized prefix, the source is preserved byte for
byte; the emitter must not manufacture one-atom markers or drop the runtime plans.
Source correctness examples are in `experiments/parameterized_source_markers`;
they are not a full Pass-3 source-backend speed claim.
