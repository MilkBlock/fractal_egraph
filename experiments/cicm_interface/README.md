# An incremental interface for the real eggcc CICM Uop rule

2026-10-09. Run offline:

```sh
python3 experiments/cicm_interface/verify.py
```

The default native oracle is target/release/examples/native_math_baseline; use
--binary to select another compatible .egg runner. The script reads the current
CICM source in ../check_statewalk, copies its first Uop LHS verbatim, and parses
it using the native engine. This is a standalone, specialized Python interface
prototype against a minimal typed fixture, NOT a modified production eggcc
pipeline, automatic abstraction search, or performance benchmark.

## What works

The prototype keeps factor buckets indexed by:

    (branch, extracted Term, predicate, original inputs, Uop, base type)

Each bucket retains Expr identity, operand slot and extraction cost. Frame
metadata supplies the If, then/else branch tuple types, outer context and length.
Only matching true/false buckets produce the final Cartesian product; unrelated
buckets are not joined. All 18 named source variables are preserved, including
both costs (which need not agree). No extra e1!=e2 predicate is added.
The verifier exports all predicted tuples into native checks and checks the
fresh native Hit relation's cardinality, so this is full set equality rather
than testing a positive sample. There is also an independent Python all-pairs
reference. All caches are updated from fixture events, not rebuilt from each
snapshot. Native execution replays the same event sequence in one engine.

The finite size abstraction is cap(s)=min(s,10), with missing stored separately.
For nonnegative, non-overflowing integers:

    cap(min(a,b)) = min(cap(a),cap(b))
    cap(a+b) = min(10,cap(a)+cap(b))
    s<10 iff cap(s)<10.

The executable checks 35^2 = 1225 input pairs (0..32,1000,1000000).
Stored min updates never increase size. A cap change entirely below 10 updates
the analysis register but need not rebuild this consumer's factor bucket, since
this LHS does not expose the exact size as a named binding. Other consumers may
need a different interface or the exact value. Native size updates in this
fixture are supplied explicitly; the complete Expr-size analysis is not compiled.

Term and predicate/input unions use an inverted dependency index and rekey only
rows referencing the losing canonical ID. DelayedSubstUnion does nothing until
the actual union-commit rule runs. Its records are not silently deleted.
Subsume removes an existing Uop's eligibility but retains its historical size,
purity and context facts.

## Binding results

| Stage | Event | Full matches | Reindexed rows |
|---:|---|---:|---:|
| 0 | initial facts; rejection cases mixed in | 0 | 0 |
| 1 | right size 12 -> 10 | 0 | 0 |
| 2 | right size 10 -> 9 | 2 | 1 |
| 3 | propose size25, retained min9 | 2 | 0 |
| 4 | missing size becomes1 | 4 | 1 |
| 5 | another expression becomes pure | 6 | 1 |
| 6 | queue predicate union | 6 | 0 |
| 7 | commit predicate union, enabling a predicate-only mismatch | 8 | 2 |
| 8 | merge extracted Term classes | 10 | 1 |
| 9 | merge input classes, completing context joins | 24 | 1 |
| 10 | subsume one original Uop | 12 | 1 |
| 11 | another left size200 -> 8 | 24 | 1 |
| 12 | its size8 -> 6, guard remains true | 24 | 0 |

All thirteen original-LHS binding sets agree. Pending and committed union are
observably different (6 vs 8 bindings), so moving the union across the barrier
would fail this test. Negative cases include wrong Uop,
base type, Term, branch context, mismatched branch tuple types, missing size,
non-purity and the strict threshold. Insertion-order reversal also agrees.
The script rejects Expr-key/operand unions outside its supported interface.
Reported brute_force_pair_attempts is an intentionally naive Python reference,
NOT native Free Join probes or a production speedup comparison. Bucket/frame
scanning and output enumeration still have costs.

## Original RHS: attempted, currently NOT passed

The full source rule is additionally run unchanged except for its ruleset name.
Its TermSubst/Subst/Get/Tuple constructors are available, but the helper rules
that interpret these auxiliary nodes are not installed. The experiment attempts
to check its emitted equalities and resulting visibility.

With the recorded cached native binary this fails: constructing a new row and
immediately subsuming it in the same action lane leaves the row visible and skips
a subsequent action. The minimal reproduction is fresh-subsume.egg:

    (rule ((= x (A))) ((B x) (subsume (B x)) (Done)) :ruleset make)

Observed after this rule: Seen(B)=1 and Done=0. An explicit subsequent subsume
of the existing B row gives SeenAfter=0. The full CICM RHS consequently fails its
final If equality check in this fixture. This is a concrete local-oracle blocker,
not a claim that all versions of production eggcc have this behavior.

results.json explicitly records original_rhs_construction_and_union_checked=false
and the diagnostic. The verifier succeeds only on the binding obligations and
the reproduced, explicitly classified RHS failure; it does not silently count
that failure as full-rule correctness. If a binary passes the immediate-subsumption
probe, the full RHS visibility assertions are enabled and must also pass.
The minimal probe reproduces on both the cached baseline binary and the local
egglog CLI inspected during this task; only the selected binary hash is recorded.

Outputs are in out/cicm-interface: original-LHS collector .egg, full-RHS attempt,
logs, minimal probe and report. Source/LHS/binary hashes are retained.

## Remaining scope

Only one Uop, type, extraction witness and context per fixture expression; no
merging of Expr keys carrying conflicting ExtractedExpr/Expr-size function rows,
no congruence collisions of hidden/visible Uop rows, no arbitrary primitive guards,
no general TypeList/container handling. Those need explicit semantics before this
interface can be installed in the production runtime. A future LHS source change
also requires reviewing this specialized factorization, even though the oracle
uses the current source verbatim. No exact footprint or speedup of a full eggcc
workload follows from this fixture.
