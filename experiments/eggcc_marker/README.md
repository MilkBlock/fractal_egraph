# RipSat-selected rule-level marker experiment

This experiment generates ordinary `.egg` programs. **It does not change the
kernel, Free Join planner, table representation, or production defaults.**

The frozen `motifs.json` comes from the finite local states in
`experiments/eggcc_cell_probe` and source consumers around their operators. RipSat
constrains the candidates; the actual transformation is an exact positive
subquery materialization. It does not infer that predicted closure facts already
exist. The boundary candidates use operator/connection hints, not a complete
certificate of a reusable cell interface.

## What is generated

For example, replace repeated `(HasType e (Base t))` matches with a two-column
relation, schematically:

```egg
(relation HasBaseType (Expr BaseType))
(rule ((HasType e (Base t)))
      ((HasBaseType e t)) :ruleset maintain-base-types)
```

Consumer LHS occurrences become `(HasBaseType e t)`. Their RHS stays unchanged.
The unused constructor-result column is projected away. The analogous tuple
view stores `(Expr TypeList)` for `(HasType e (TupleT ts))`.

A native maintenance run is inserted immediately before each affected consumer
ruleset run. It sees initial facts, late facts, and union-enabled combinations.
The builder depends only on original tables, so one maintenance step suffices;
it never saturates user rules or invents predicted interior facts. This is a
whole-program transformation for the supplied schedule. Extra maintenance steps
and changed matching order can still perturb order-sensitive merges, so a
complete execution check remains mandatory in this prototype.

Candidate selection is finite and reproducible:

- Nested constructor pairs or two positive predicates sharing a binding.
- Observed RipSat interior motifs, plus source consumers connecting known cell
  operators to boundary facts.
- At least two LHS occurrences, with no fixed top-k cutoff.
- Constructors only; operators deleted/subsumed in the active schedule or by
  top-level actions are excluded. Mutable custom functions are not cached.
- Flat explicit-rule exports are required; include/push/pop, late declarations,
  and `until` schedules are outside the current contract.

Relations provide narrow typed tables without allocating another e-class for
every marker. No original facts are removed, so this is a time/space tradeoff,
not a storage compression result.

## Native measurements

Both sources are real exports from `check_statewalk`/eggcc revision
`ae80f82a403c1209fb084b115ee89470c7e66d0e`, using one optimizer pass. The same local
modified engine executes baseline and transformed source. This is not a comparison
to the pristine egglog tag.

| Input / view | LHS sites | scan + refine rows | extra marker rows | execution improvement* |
|---|---:|---:|---:|---:|
| fact / `HasType(Base …)` | 23 | 195,665 → 177,476 (−9.30%) | 39 | 2.30% |
| loopfact / `HasType(TupleT …)` | 14 | 340,173 → 325,835 (−4.21%) | 76 | 3.40% |

\*Median of nine paired execution-time improvements, with rotating run order and
profiling disabled during timing. The individual execution medians are
62.34 → 61.40 ms and 78.59 → 75.68 ms; a ratio of medians is a different statistic.
Total setup + execution medians are 145.96 → 144.24 ms and 162.58 → 158.87 ms.
Other batches varied; these are small measured gains, not an order-of-magnitude
speedup. Source parsing, rewrite generation, and graph validation are not timed.

All original facts, primitive values, equality structure, and subsumed visibility
match in the profiled check and every timed repetition of these two accepted
variants. Only the new auxiliary relations are excluded from comparison. Marker
maintenance work is included: key lookups actually rise from 5,579 to 5,600 and
8,906 to 8,986. Scan/refine counters count visited row positions, including index
work and repeated visits; they do not count distinct facts or logical matches.

## Negative results and limits

The initial interior-only screen found four eligible views. Extending to boundary
consumers yielded 21. Many fail full-state equality; they are retained as rejected
experiments, not recommended rewrites. The same Base view that passes on `fact`
fails on `loopfact`. Combining individually passing views also failed. A separate
shadow-ruleset placement preserving original registration did not resolve the
mismatches.

One concrete mismatch in zero-hit variants is the final value of `to-hoist`,
declared with `:merge new`. Such selection depends on arrival order. This explains
some differences but is not assumed to explain all: the failed Base view on
`loopfact` still differs even when this one table is excluded diagnostically.
That diagnostic projection is never used to accept a candidate.

These are workload-specific empirical validations, not a proof of correctness
for every eggcc input or future rule schedule. A frozen motif dictionary proposes
views; it does not authorize them automatically. Running the differential
validation on every production invocation would cost more than the small gain.
A broader deployment needs an explicit semantic contract for order-sensitive
analysis functions or a stronger preservation proof.

## Reproduce

Build the source transformer and existing native read counters:

```sh
cargo build --release --offline --features native-read-profile --example eggcc_marker
cargo test --offline --example eggcc_marker --example eggcc_cell_probe
```

Export a real source with the existing eggcc binary:

```sh
/path/to/check_statewalk/target/release/eggcc \
  /path/to/check_statewalk/benchmarks/passing/bril/core/fact.bril \
  --run-mode egglog --stop-after-n-passes 1 > /tmp/fact.egg
```

Generate, validate, and measure against the unchanged source:

```sh
python3 experiments/eggcc_marker/run.py \
  --source /tmp/fact.egg --view base --repeats 9 --output out/marker-fact-new
```

For `loopfact.bril`, use `--view tuple`. A new output directory is required.
`validated.egg` is emitted only when **all** full-state checks pass. It runs as an
ordinary egglog program, without analysis or tracing, for example:

```sh
cargo run --release --offline --example native_math_baseline -- \
  out/marker-fact-new/validated.egg
```

`run.py` leaves the full profiles, candidates, and timings in the output directory.
The checked-in results are compact summaries. The optional low-level `bench` mode
screens every eligible motif; `bundle` evaluates combinations and `--shadow`
reproduces the registration-placement ablation. None changes production defaults.

Tests cover same-step results, late input, union-enabled shared and nested
matches, alias constraints, output preservation, Set membership, primitive
values, and boundary-slot aliasing.
