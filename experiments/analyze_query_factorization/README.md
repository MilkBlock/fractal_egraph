# Factored probes in native analyze

This changes the actual `egg_layout analyze` pipeline, not a standalone matcher
or a rewritten `.egg` benchmark. Tier0 rules, composition admission, Cell
certification, and the default ripen limits (50 ms / 200,000 facts) stay intact.

## What was expensive

The three-round `experiments/math_mini3/source.egg` finished tier0 quickly but
stalled in weak-dominate probes. A late stack sample found all four workers in
`run_fact_query_difference`. One repeatedly expensive target had 25 atoms,
27 projected variables and 125 initial bindings. Its repeated Sub fragments
shared a `Const(-1)` result with a larger integration fragment.

Two sources of wasted work were identified:

* The read-only success callback waited for a normal RHS batch. It could find a
  valid binding but keep searching before the callback could request a stop.
  Both the normal and specialized last-stage paths now flush collector actions
  immediately. Normal mutation rules retain their batching. A native kernel
  regression checks **one** callback rather than the old **128**, and then checks
  that an exhaustive query still returns all 1,728 tuples.
* Functionally determined values were making independent fragments appear joined.
  `Const(-1, k)` determines `k`; it does not require trying all pairings of
  independent choices on either side. `native_factored_query.rs` computes that
  dependency closure, separates the remaining variable components and includes
  the deterministic anchor atoms in every native subquery. There is no assumption
  that different variables have different values or that constructors are injective.

The existing decomposed Generic Join planner was also tried. It still hit the
15-second process guard on this workload, and that experiment was removed. The
retained implementation uses native flat indexed queries on smaller factors.

## Exact existence and freshness

For independent factors, the projected answer set is a Cartesian product. Let E
be the initial bindings, canonicalized under the current union state.

1. If one factor is empty, the complete query is empty.
2. If a factor produces a tuple outside its projection of E, combine it with one
   valid tuple from every other factor to obtain a fresh complete binding. Once
   this is known, the remaining factors only require existence queries.
3. If every factor stays inside its old projection, **new combinations are still
   possible**. For example, E={(0,0),(1,1)} permits fresh combinations (0,1) and
   (1,0). The implementation retains the complete matching old projections when
   no fresh factor tuple exists, then checks complete combinations against E.
   It visits at most |E|+1 distinct complete combinations before finding a fresh
   one, or exhausts a product of size at most |E| when none exists.

That bound follows from finite-set membership; it is not a candidate/time/depth
budget. Projection tuples use set semantics. Unprojected components remain
existential obligations, including missing ground gates. Full binding mode still
enumerates the full answer set. Initial full-binding materialization is also
retained; neither operation has an unconditional cheapness guarantee.

The extra exclusion/witness storage is bounded by the projected initial sets.
Factoring does not make arbitrary connected joins cheap. Union, subsume, and
function updates are still read through the native engine; the existing query
epoch invalidates result reuse after a mutation. No result is reused across
unrelated engines. Reentry-condition coverage now requests one valid mapping
instead of enumerating every automorphism.

## Measurement and validation

See `results.json`. Each clean timing is one run, with DOT/SVG and profiling off;
compilation is excluded. The old executable was copied before this change from
the working tree at HEAD `8e66e2d` (which already had other local changes). This is
**not a pristine-egglog comparison**. The old run was stopped by an external
15-second guard, so its complete time and exact peak RSS are unknown.

| Version | Process time | Memory |
|---|---:|---:|
| Before | Did not finish within the 15 s guard | At least 2.35 GiB sampled group RSS |
| After | **6.98 s**, completed | **1.86 GiB** peak process RSS |

The completed run retains 129 tier0 applications and processes 221 ripen jobs:
167 saturated, 31 unfinished and 23 rejected. All 165 completed state exports
available from the interrupted control match the new exports after removing
run-local IDs. These figures do not claim that timed unfinished prefixes match.

Timed unfinished ripen prefixes can differ. The separate `verify.py` comparison
uses 80-fact prefixes and a 60-second safety limit, compares exported graph states
and existence/freshness decisions, and allows different valid witness choices.
It does not use timing differences as correctness evidence. The recorded result
compares 194 state files and 1,871 positive weak-dominate decisions; see
`fixed-work.json` for the full comparison scope.

Reproduce (preserve an old executable before rebuilding):

```sh
cargo build --offline --release --bin egg_layout
python3 experiments/math_mini3/bench.py --default-only --no-graphs --seconds 15 \
  --binary /absolute/path/to/before-egg_layout --output out/query-before
python3 experiments/math_mini3/bench.py --default-only --no-graphs --seconds 15 \
  --output out/query-after
python3 experiments/analyze_query_factorization/verify.py \
  --before /absolute/path/to/before-egg_layout --output out/query-fixed-work
```

`EGG_LAYOUT_PROFILE_WEAK=1` prints individual probe starts/ends for diagnosis;
it is off in the clean timing. Existing `EGG_LAYOUT_PROFILE=1` prints phase totals.

Tests cover complete queries, empty/partial/repeated projections, missing anchors,
shared free variables that must not be separated, all 16 exclusion subsets of a
2x2 product, new combinations of old factor values, union, function replacement,
subsume, and preserved user tables. Existing DABCABC, branching reentry, Cell
discovery, saved-history replay and source/target evidence regressions are run.

```sh
cargo test --offline --release --lib weak_dominate
cargo test --offline --release --test native_fact_query --test reentry_bucket \
  --test growth_reentry --test reentry_history_regression \
  --test analyze_cell_ports --test analyze_cell_families --test cell_egraph_pipeline
cargo test --offline --release --manifest-path egglog/Cargo.toml \
  --no-default-features --features graphviz --lib fact_query::tests --target-dir target
```
