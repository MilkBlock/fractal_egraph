# First divergence is a native order-sensitive choice

The earlier message `different equality partition` was too broad. The comparator
also used that message when a **function selected a different existing Expr**.
It did not establish that the GPU performed an invalid union. The diagnostic now
includes the function and its key in result-mapping errors. The strict benchmark
gate is unchanged.

## Isolated boundary

CPU cached-query control and the hybrid GPU execution were advanced through the
same original schedule. After each atomic Run/Saturate, the diagnostic compared
all public facts, primitive values, equivalence and subsumption modulo allocation
order. Sequence/Repeat and their early-stop semantics were preserved.

- First **142 boundaries agree**.
- Boundary 143, path `/0/1/r1/12`, is `(run boundary-analysis)`.
- That ruleset runs **natively on both lanes**; GPU handles only the three terms
  rulesets.
- Immediately before this boundary the complete graphs agree.
- Immediately afterward, all tables/equivalence/visibility still agree **except
  the `to-hoist` function**. Both have 453,198 total rows.

The production source explicitly chooses an arbitrary maximal invariant:

```egg
(function to-hoist (Expr Expr) Expr :merge new)
(function to-hoist-size (Expr Expr) i64 :merge (max old new))

(rule ((is-inv-Expr body expr)
       (DoWhile inputs body)
       (HasType expr inv_type)
       (= inv_type (Base base_inv_ty))
       (= (Expr-size expr) (to-hoist-size inputs body)))
      ((set (to-hoist inputs body) expr)) :ruleset boundary-analysis)
```

Source: `check_statewalk/dag_in_context/src/optimizations/loop_invariant.egg`,
lines 24–47, in the captured revision. Its comments explicitly describe picking
one invariant and call this an "evil hack". The effect is not commutative:

```text
write candidate a; write candidate b -> choose b
write candidate b; write candidate a -> choose a
```

Equal candidate sets do not imply equal final function values. Replacing queries
can alter native storage/index/commit traversal without changing logical facts.
We have isolated where that ordering becomes observable; we have not isolated
the precise lower-level cache or physical traversal responsible for the order.

## Concrete witness

For the original key `(inputs = Expr(1951), body = Expr(2013))`:

| Observation | CPU control | Hybrid GPU |
|---|---|---|
| Eligible candidate set in pre-boundary replay | Same 13 Exprs | Same 13 Exprs |
| Candidate `Expr-size` | All 1 | All 1 |
| Actual function value after the boundary | Expr(1991) | Expr(1972) |

The 13 candidates are 1962, 1964, 1968, 1972, 1974, 1977, 1980, 1982, 1986,
1988, 1991, 1994, and 1996. These are **original input identities shared by both
lanes**, not newly allocated IDs compared blindly. Both chosen values are
eligible, distinct candidates with the same maximum size.

Tracing is diagnostic and can perturb traversal: the traced CPU replay retained
its selection; tracing the cloned GPU-side native boundary selected 1991 rather
than its actual untraced 1972. Consequently the trace is evidence for the
candidate set, not a faithful transcript of the untraced physical write order.
No logical match is reported as a committed mutation.

## Full-run checks and limits

The original-partition lockstep replay reaches the end with strict equality
still failing on `to-hoist`. Excluding only that function, **698,986 rows and
524,631 mapped equality classes** agree, including primitive values and visibility.
This is a diagnostic projection, not a relaxation of benchmark acceptance.

A separate **sequential CPU-then-GPU** replay confirms the same conclusion:
excluding only `to-hoist`, 698,985 rows and 524,631 mapped equality classes agree.
This diagnostic has one fewer materialized `Smaller` row than the lockstep
replay, in both lanes. It is recorded separately and not substituted for the
previous timing run. Full equality still fails on the chosen `to-hoist` value.

Moving `boundary-analysis` to the sorted assisted path does not by itself solve
the problem: it can still select different fresh Expr IDs. Sorting raw allocated
IDs is not an allocation-independent semantic tie-break. This experiment is kept
as a rejected diagnostic, not installed as a fix.

The original sequential benchmark also used a fused native lane whose execution
and materialized `Smaller` history differed from the sorted CPU control. That is
a separate comparison. Do not use the lockstep projection to certify the older
three-lane timing or claim pristine graph equality. The final sequential
CPU/GPU projection is recorded separately in `results/first-divergence.json`.

A deterministic comparison policy would need a stable tie-break for permitted
choices, or a separately justified observational-equivalence contract. Neither
silently dropping `to-hoist` from the strict gate nor merely comparing graph sizes
provides that contract. These diagnostics do not enable Cell sat skip.

## Reproduction

```sh
cargo build --offline --release --example raytrace_pass3_diff --target-dir target/analyzer

# Stop at the first differing atomic boundary; writes diff.json even on failure.
RAYON_NUM_THREADS=1 target/analyzer/release/examples/raytrace_pass3_diff \
  /Users/mineralsteins/Repos/egg_related/egg_layout/out/raytrace-interactive-20261010/pass-3-flat.egg \
  out/cell-locality-worker/matrix out/pass3-diff-new
```

Use a fresh output directory for each variant:

- `PASS3_DIFF_INSPECT=/0/1/r1/12`: verify/inspect this already located boundary
  without repeating all prefix graph exports. Writes `boundary.json` with
  replayed native matches and the actual function outputs.
- `PASS3_DIFF_INSPECT=final`: execute in lockstep but compare only final graphs.
- `--sequential`: CPU completes before GPU, as in the benchmark, then compare.
- `--sort-hoist`: rejected diagnostic assigning `boundary-analysis` to the sorted
  assisted partition as well; this is not the original partition.
- `PASS3_DIFF_OPEN=PATH`: descend into a located Saturate boundary to compare its
  individual Run steps. Never used in the timed path.

All modes retain strict graph failure as a nonzero exit. Projections are labeled
diagnostics. The complete original input/schedule and matrix worker are unchanged.

Validation: the new function-choice regression plus the existing graph/cost
checks, six sat-skip checks, and twelve shared-backend checks pass (21 total).
