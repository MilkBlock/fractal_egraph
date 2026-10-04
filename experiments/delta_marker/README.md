# Producer delta markers: rule-level experiment

This experiment runs real egglog rules from eggcc. It is not enabled in the
production analyzer or eggcc compiler. It tests a small stable-tag boundary
fragment, not a complete certified RipSat state-machine optimizer.

## Transformation

For a consumer fragment `(HasType e (Base t))`, introduce a relation
`DeltaView_known(e, t)`. When a producer RHS constructs that fragment, emit the
relation directly from its already evaluated bindings. Consumers query this
relation instead of joining HasType with Base. Original facts remain available.
For example, the observed Get producer constructs
`HasType(Get(e,i), Base(TypeList-ith(tylist,i)))`; its RHS can emit the marker
without searching for that output again. The manifest links producer rules to
committed HasType records in the training trace.

Producers with unknown Type shape emit `DeltaView_unknown(e, type)`. A small
fallback ruleset joins only those rows with Base before consumer schedules.
Thus the direct path has no marker-materialization Join; the fallback still has
one. TupleT is the second supported fragment. Only supported root-fact consumer
occurrences are replaced.

The generator checks the resolved supplied program: explicit unions/functions
merging the decomposition sort, unsupported writers and destructive mutations
are rejected. Child unions are handled by native canonicalization. This is a
closed-program transformation: adding rules or writers requires regeneration.
It does not require tracing during optimized execution.

## Correctness criterion

Internal tables may differ: auxiliary relations are intentional, and `to-hoist`
can select another admissible expression. Full table equality is reported but
is not an acceptance gate. The measured-instance gate requires:

* Copied-row oracle finds no invented marker after steps, and no missing marker
  immediately before consumer schedules.
* The auditor's schedule walk agrees with ordinary execution of the same program.
* Each candidate Function extraction passes a read-only membership query in the
  baseline's original function-root eclass (no insertion into that graph).
* Stored to-hoist choices satisfy the original selection predicate at the final
  state. This is not a proof of every historical write.
* Actual eggcc conversion, type checking and DAG interpretation agree on inputs
  0, 1, 2, 3, 5 and 10, including return values and print logs.

Both tested programs passed, with unchanged argument annotations and identical
baseline/candidate extracted function terms. These finite checks do not prove
universal equivalence. The production Tiger/DAG extractor has not been tested.

## Measurements (2026-10-04)

Three rotating unprofiled timing samples; row counters from separate profiled
runs. Execution time excludes parsing/installing rules (about 82–83 ms). Same
local kernel for all variants; this is not a pristine-upstream comparison.

| Workload / fragment | Variant | Scan + refine rows | Execution median ms |
|---|---|---:|---:|
| fact / Base | original | 195,665 | 62.105 |
| | producer instrumentation only | 192,092 | 60.290 |
| | rewritten consumers | 182,306 | 60.364 |
| loopfact / TupleT | original | 340,173 | 72.288 |
| | producer instrumentation only | 340,145 | 72.340 |
| | rewritten consumers | 328,630 | 72.193 |

Scan/refine row work falls 6.83% and 3.39%; these are **not Join-count reductions**.
Key lookups increase 5,579→5,707 and 8,906→9,051. Auxiliary marker rows are 53
and 93. Paired execution improvements have medians 2.80% and 0.19%; with three
samples these are not evidence of a substantial speedup. In fact, producer-only
fact is slightly faster than the fully rewritten candidate. Rule instrumentation
also changes planning/scheduling choices, so not all row-work differences can be
attributed to eliminating the target Join. No memory advantage is claimed.

Compact source-hashed reports: `fact-result.json`, `loopfact-result.json`.

## Reproduce

From the repository root, with local sibling `../check_statewalk` available:

```sh
cargo build --offline --release --example delta_marker --example marker_validate --example eggcc_marker
LIBRARY_PATH=/opt/homebrew/opt/cbc/lib CARGO_TARGET_DIR="$PWD/../check_statewalk/target" cargo build --offline --release --manifest-path tools/eggcc_term_validate/Cargo.toml
python3 experiments/delta_marker/run.py --source /path/to/flattened-fact.egg --history /path/to/history.json --inner Base --output out/marker-fact
python3 experiments/delta_marker/run.py --source /path/to/flattened-loopfact.egg --history /path/to/history.json --inner TupleT --output out/marker-loopfact
cargo test --offline --release --example delta_marker --example marker_validate
```

Sources must be flattened explicit-rule eggcc programs with initialization and
one optimizer pass; trace evidence used here comes from fact. The checker is
factorial-specific. Generated files include baseline, producer-only, candidate,
manifest, oracle report, extracted programs and timing measurements. On success
`instance-validated.egg` is emitted. This name only certifies the measured case.
The separate checker uses eggcc's own dependency versions and existing CBC
installation; its lockfile permits offline builds with the cached dependencies.
