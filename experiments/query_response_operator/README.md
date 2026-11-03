# Shared predicate responses for a real eggcc rule bank

**The automatic factorization works; the Tensor Core objective is not met.**
This experiment compiles a broad, mechanically selected class of real Pass-3
queries. It shares actual predicate evaluation across different bindings and
rules. It does not use a selected recurrence or parameterized Cell family.
Its remaining matrices are small, and a straightforward GPU sparse-condition
kernel still beats both matrix implementations. No engine code was changed.

## The eligible class has a precise semantic condition

An eligible query has no primitive guards/native residuals, and has an anchor
atom containing **every variable shared between two or more atoms**. Anchor
selection is deterministic: greatest column count, then first source position.
No query is selected by its measured performance or number of matches.

For an anchor binding b, every remaining atom is a relation restricted by a
fully known projected key. Its other variables occur in that atom alone.
Consequently, existence of a complete binding is exactly:

```
anchor's own constants/aliases hold
AND there is a row for projected key K1(b)
AND there is a row for projected key K2(b)
AND ...
```

Each projection retains ALL original whole rows. The full answer is an anchor
binding plus a product of these correlated row sets. No arbitrary witness is
chosen. A predicate response is not an e-class identity and does not merge
payload values. Primitive guards can couple local variables, so this prototype
excludes such queries rather than treating them as independent.

The cyclic query `A(x,y), P(x,z), Q(y,z)` is rejected: no atom contains all of
x,y,z. Taking independent marginals could falsely combine different z values.
This counterexample, aliases, multiple witnesses and disconnected empty factors
are regression tested.

## From real rules to a shared numerical function

Two actual Bop rules illustrate the construction:

```
Bop(op,x,y), BinaryOpIsPure(op), ExprIsPure(x), ExprIsPure(y)
  -> ExprIsPure(Bop(op,x,y))

Bop(op,x,y), ExprIsResolved(x), ExprIsResolved(y)
  -> ExprIsResolved(Bop(op,x,y))
```

For each real Bop row, one context vector contains the five condition values:

```
X[row] = [OpPure(op), Pure(x), Pure(y), Resolved(x), Resolved(y)]

             pure  resolved
W =          [1      0
              1      0
              1      0
              0      1
              0      1]

rule_enabled = (X W == [3,2]).
```

The compiler constructs the analogous X/W for ALL eligible queries. It shares
the typed projected index, its use at the same anchor positions, and repeated
projected-key reads. Literal tests, alias tests and joint keys are preserved.
It does not assume functions always have a single result.

Two different bindings can have the same context vector. Compute their rule
responses once and keep a route from each original row to that vector:

```
raw typed facts -> condition vector X -> distinct profiles Z
                                      -> Z W -> rule response
each original row ----------------------^       |
its own original binding/payload ----------------+-> eventual effects
```

This is sharing a decision computation, not merely storing repeated spines
compactly. However, obtaining X still costs real table reads. It is not yet an
event-published runtime marker or a proof supplied by RipSatCell.

## Exact cofactors, with explicit validity conditions

For the observed profiles, factor out constant columns, sum weights of equal
columns, and share equal rule tests. For example, if two feature columns are
equal, their combined coefficient may be 2 rather than 1. The resulting
weights are exact small integers, not learned approximations.

The original rule matrix remains available. Before using a cofactor on new
profiles, `evaluate_folded_or_original` verifies its constant/equal-column
assumptions. A violation falls back to the original matrix. Observed column
equality is NOT a theorem about all future egraph states.

A separate regression adds a second witness without changing a predicate's
truth value. The rule response stays the same, but the full binding set grows.
Therefore, unchanged responses do NOT authorize skipping new RHS effects.

## Frozen Pass-3 results

The input is the saved native query-shape profile (609 admitted queries, another
55 rejected by that profile compiler) and 694,061 visible rows of the saved
canonical graph. Exact hashes are in `results.json`.

| Quantity | Result |
|---|---:|
| Eligible guard-free, anchor-complete queries | 452 |
| Eligible queries whose anchor table is nonempty | 282 |
| Anchor table groups / nonempty groups | 116 / 51 |
| Distinct projected indexes | 160 |
| Anchor facts across groups | 331,289 |
| Distinct observed condition vectors | 295 |
| Root/rule decisions before response sharing | 1,304,139 |
| Decisions on distinct profiles | 4,422 |
| Positive `(query, anchor row)` results | 556,797 |
| Raw GEMM FLOPs / shared profile FLOPs / cofactor FLOPs | 28,083,332 / 238,260 / 82,038 |
| Distinct projected-key reads to construct features | **674,792** |

The 170 queries with empty anchor tables do not earn any reuse credit. Both
GPU controls exclude empty groups. These query counts are not time-weighted
coverage: the excluded queries include difficult multi-atom joins and guards.
Running all query predicates on a frozen final graph is also not the real
sequence of per-ruleset Run frontiers.

The Bop group makes the reduction concrete:

* 3,486 real Bop rows and 31 source queries.
* 37 original feature columns, 76 distinct observed vectors.
* After checked cofactors: **`76 x 21` times `21 x 20`**.
* Routing reconstructs all 108,066 original root/rule decisions, of which
  31,340 are positive. Their 76 condition profiles produce 30 distinct response
  masks, but the original binding/payload of every row remains separate.

## Independent native verification

`run.py --native` emits `oracle.egg`: frozen typed table values are bijectively
mapped to opaque i64 IDs, and each original query shape is translated into a
native natural join publishing `(query ID, anchor row ID)`. Guards are absent
by the eligibility condition. All 452 queries are executed once by the existing
native egglog binary, including their original literals and variable wiring.

The complete projected result set agrees with the shared matrices and a second
Python full-binding join implementation: **556,797 entries, no missing or extra
entries**. The source-program and binary hashes are recorded. This verifies
frozen LHS existence; it does not run original RHSs, function merges, native
watermarks, live union/rebuild, or extraction.

The unchanged native result was reused for the cofactor implementation by
checking both source hashes, the complete query set, and the full projected
answer digest reconstructed from the earlier oracle target packet. Counts alone
are insufficient. `--reuse-native-report` does these checks automatically.

## GPU experiment: controls defeat the dense-matrix hypothesis

All four modes consume the SAME prepared feature bank and recover all 1,304,139
original Boolean outputs. All output bytes agree. One warmup per mode, then
one retained execution, on Apple M3 Max:

| Implementation | Device time | Host encode/submit/wait |
|---|---:|---:|
| Raw feature GEMMs | 1.968 ms | 4.669 ms |
| Raw sparse predicate checks | **0.547 ms** | **1.266 ms** |
| Shared cofactor GEMMs + routes | 0.887 ms | 2.855 ms |
| Shared cofactor integer evaluation + routes | 0.640 ms | 1.295 ms |

These times INCLUDE expansion to original output rows, but EXCLUDE Python
projection-index preparation, feature construction, profile grouping, device
input packing/transfer and native effects. Python spent roughly 0.36 s building
the projections and 0.94 s materializing/grouping/scoring features; this is an
untuned preparation implementation, not a proposed production baseline. Native
oracle time includes parse, compile and printing and must not be divided by
these device-only numbers to claim acceleration.

The sparse control prevents misleading credit: fewer logical decisions do not
automatically mean lower time. Decoding routes, memory traffic and 32 tiny GEMM
launches dominate the shared version. Padding them into one block-diagonal
matrix would add unused arithmetic. There is no NVIDIA Tensor Core measurement
and no neural-network-like utilization claim.

## What remains useful, and what is still missing

This is a general automatic construction of a finite response function over
many source queries. It exposes a useful boundary between original binding
payloads and shared decisions. It is still a shallow, anchor-complete class;
it does not solve arbitrary joins or certify Cell closure reuse.

The unremoved cost is **producing and maintaining the predicate coordinates**.
A production continuation would need native commits to update those coordinates
and payload witnesses, including canonicalization and visibility. Only then
could response reuse avoid repeat search. To justify large matrix computation,
one must fuse that generation and downstream work while preserving sharing;
the tiny final predicate classifier alone is not a sufficient target.

## Reproduce

Set `SNAPSHOT`, `PROFILE`, `NATIVE` to the recorded inputs/binary; use Python
with NumPy. Native replay is optional after an identical result is available.

```sh
python -m unittest discover -s experiments/query_response_operator -p 'test_*.py' -v
python experiments/query_response_operator/run.py --snapshot "$SNAPSHOT" --profile "$PROFILE" --output out/query-response --native "$NATIVE"
/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/swiftc -O -sdk /Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk experiments/query_response_operator/gpu.swift -o out/query-response/gpu-worker
out/query-response/gpu-worker out/query-response/input.bin out/query-response/gpu.json
```

`model.py` contains the compiler/cofactor algebra and an independent reference.
`run.py` prepares real inputs and emits the native oracle. `gpu.swift` checks
both matrix and non-matrix controls. Generated `.bin`, native oracle programs,
compiled workers and caches remain outside Git. Only sources and text results
are committed.
