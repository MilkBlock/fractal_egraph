# Matrix equality closures for additional rulesets

`--matrix-saturate` now also handles the **complete three-rule
`terms-helpers-helpers` ruleset** in real Raytrace Pass 3. It computes min-choice
equality closure in private GPU state, then publishes the resulting union-find
and performs one global rebuild. `terms` and `terms-helpers` still use the
ordinary executor; they are not claimed as compiled matrix closures.

This extends the actual device backend, not an offline analyzer/prototype. The
recognizer uses native typed rules and schema dependencies, never the strings
`TCPair`, `Smaller` or a ruleset name. It accepts the corresponding renamed
rule family. Both matrix paths remain opt-in.

## Why a whole comparison family can share one context

The original three rules respectively select the left cost witness when its
cost is smaller, the right witness when larger, and the declared side on a tie.
After union, a class can contain **multiple** `TCPair` witnesses with different
terms and costs. Replacing that class by just its minimum cost is incorrect.

For a choice `Smaller(A,B)`, let `costs(A)` and `costs(B)` include every visible
cost witness. With left tie preference, the exact conditions are:

```text
output ≡ A  iff there exists a ∈ costs(A), b ∈ costs(B) with a ≤ b
            iff min(costs(A)) ≤ max(costs(B))

output ≡ B  iff there exists a ∈ costs(A), b ∈ costs(B) with b < a
            iff min(costs(B)) < max(costs(A))
```

Empty cost sets disable both outputs. Right tie preference changes which
inequality is strict. For example, `A={1,9}`, `B={5}` must select **both** sides
and unite them. A minimum-only implementation would lose the second effect.
The tests retain this counterexample.

Thus the common context is a two-column **exact min/max witness matrix per
current equality class**. Every choice reuses those columns. There is one marker
row per actual choice, serving all three original rules. It does not store the
Cartesian product of cost witnesses or materialize complete rule bindings.

## Exact i64 coordinates, including the large sentinel

There is no conversion of the numeric cost itself to FP16. A signed i64 is
reinterpreted as unsigned, its sign bit is flipped, then it is split into eight
bytes in most-significant-first order. Lexicographic byte order is exactly
signed integer order, including `i64::MIN`, `i64::MAX` and `10^16`.

For each choice, gather the bytes of:

```text
[ min(A) | max(A) | min(B) | max(B) ]     32 FP16 coordinates
```

A shared `32 × 16` coefficient matrix with entries `-1, 0, +1` computes all
eight byte differences for `max(B)-min(A)` and `max(A)-min(B)`. The first nonzero
byte difference supplies the comparison sign. Each byte and each difference is
exactly representable; no subtraction of enormous rounded costs takes place.
An explicit presence mask distinguishes missing costs from a zero-valued cost.

`cu_mma` uses Metal SIMD-group matrix instructions with FP16 input and FP32
accumulation. `EGGFUSE_MATRIX_AUDIT=1` compares the resulting decisions against
direct exact i64 comparisons for every active choice. The coefficients are
compiled, not trained, and the result controls real union proposals.

## Private fixed point and its certificate

```text
GPU rows, immutable at entry
    -> compact actual cost-row and choice-row indices
    -> private copy of canonical union-find
    -> reduce min/max witnesses by current class
    -> gather byte coordinates -> matrix multiply -> exact order mask
    -> union the enabled choice results in private state
    -> close choice-key congruence in private state
    -> repeat until no new union
    -> publish parents -> one existing global rebuild
```

Congruence is essential: two choices may acquire equal operand classes during
this calculation, requiring their result classes to unite. Hash keys are frozen
for each private iteration before any unions are applied. Later iterations pick
up newly equal keys and additional cost witnesses. Witnesses only accumulate,
so every proposed equality has a surviving original-rule witness. A successful
union reduces the number of classes; this supplies a finite termination bound.

The compiler requires:

- Exactly the exhaustive less/greater/equal cases, with consistent min selection
  and tie side, two unconstrained cost witnesses and one binary choice anchor.
- Each effect only unites the choice result with an **existing** selected cost
  witness. No new structural constructor or arbitrary function update is hidden.
- A schema closure proving that key canonicalization cannot invoke a function
  merge or another constructor feeding equality back into the selected sort.
  Downstream constructor congruence with no feedback is allowed and performed
  by the existing global rebuild.

Runtime hidden cost/choice rows reject this fast path before any global write;
the ordinary executor handles their visibility semantics. Unsupported guards,
changed selection policy, function-key callbacks and extra equality feedback
also retain ordinary execution. No time/candidate cutoff accepts a partial
closure. The only bounds are finite state size and device address/storage limits.

The private buffers are reused by capacity, but their **contents are not reused
as stale proofs**. Every call repacks actual current ports and copies current
canonical identities. Scratch therefore follows the largest live context,
not the number of historical calls. Reading table deltas instead of repacking
at every entry remains future work.

## Schedule semantics

The optimization only replaces an explicit Saturate boundary. Ordinary Run,
finite Repeat and source assertions retain their execution path. Numeric guard
calculations and local unions remain on GPU; CPU schedules the operation and
receives small status blocks, never binding arrays for RHS execution.

A private equality iteration is **not an original logical match or Run**.
Reports record one `comparison_saturate` operation with `matched: null`, and
separate `local_iterations` / `local_unions`. Per-rule watermarks advance to the
completed public boundary. External rules still see the resulting rows as new
relative to their own watermarks. Checkpoint restoration preserves this rule.

The native checker independently runs the original ruleset to saturation and
compares the entire boundary state. It does not imitate the private iteration
count. `compare_runs.py --saturation-macros --semantic-device-states` requires
the same original core/schedule, complete execution from the initial state and
full typed graph equality; it explicitly records unequal internal Run histories.
No native import/rebuild is used to repair the GPU result before comparison.

## Real Raytrace results, 2026-10-27

Reuse baseline `out/device-matrix-sat-pass3-final` from the preceding change;
candidate `out/device-comparison-pass3`. Same saved source, native frontend,
schedule and capacities; old control was not rerun. One candidate timing, without
validation layers or native execution. The old predicate matrix path is enabled
on **both** sides, and independent batched RHS publication is off on both.

| Metric | Previous predicate-only path | Plus comparison closure |
|---|---:|---:|
| Full inference | 44.161 s (historical) | 39.307 s |
| Comparison ruleset GPU time | 7.502 s | 1.566 s |
| Its preparation | 4.668 s | 0.231 s |
| Its matching/propagation | 0.423 s | 0.352 s |
| Its commit/rebuild | 2.411 s | 0.983 s |

The comparison ruleset uses **4.79x less GPU time**; full inference is about
**11.0% lower** against this saved control. This is not a new pristine CPU
speedup, not an average, and not an isolated GEMM-versus-integer benchmark.
Most measured savings are from avoiding repeated index preparation and global
publication/rebuild, not from making an individual integer comparison faster.

All **1,186** comparison Saturate calls were admitted. They performed **2,566**
private union iterations and **90,045** successful local unions. Host control
plus wait for this ruleset totals 3.409 s, including its 1.566 s GPU time; these
are overlapping measurements, not additive costs. Retained comparison scratch
is **21,266,192 bytes (~20.3 MiB)**. It coexists with the earlier predicate
workspace and original device tables; it is not an RSS measurement.

The full source schedule completes. Final **699,784 rows / 524,742 eclasses**,
all primitive/function values, visibility and equality partitions match the
previous device backend. Internal operation count changes from 10,097 to 9,257,
because equality Saturates are now reported as macros. The original source
schedule is unchanged.

Independent native check uses Raytrace's real initial graph and **all three
nested terms rulesets**, isolated from the rest of Pass 3. Both device paths
match native: **27,793 rows / 21,655 eclasses**. Native executes 1,060 Runs;
the candidate reports 928 public/retained operations. This does not repair or
hide the pre-existing full-Pass device/native mismatch.

Validation also includes 19 comparison cases, 16 predicate-matrix cases and all
60 existing backend checks, with Metal validation and exact matrix auditing.
Cases cover multi-valued classes, both tie policies, signed i64 boundaries,
congruence, cycles, a 96-link chain, external unions, ordinary Run, renamed
schemas, hidden-row fallback, rejected callbacks/policies, and restoration.
Evidence: [comparison-saturate-results.json](comparison-saturate-results.json).
No fresh hardware occupancy claim is made.

## Reproduce without rerunning unchanged controls

```sh
python3 experiments/device_backend/build.py --worker-only \
  --reuse-native-build out/device-build-context-final-v2 \
  --output out/comparison-build --target target/device-backend

EGGFUSE_MATRIX_AUDIT=1 MTL_DEBUG_LAYER=1 \
python3 experiments/device_backend/test_matrix_comparison.py \
  --build out/comparison-build --output out/comparison-tests

EGGFUSE_MATRIX_AUDIT=1 MTL_DEBUG_LAYER=1 \
python3 experiments/device_backend/verify_matrix_raytrace.py --scope terms \
  --source /path/to/pass-3-flat.egg --build out/comparison-build \
  --output out/comparison-native

python3 experiments/device_backend/run.py --matrix-saturate \
  --source /path/to/pass-3-flat.egg --build out/comparison-build \
  --output out/comparison-pass3 --check-every 0 --save-state \
  --capacity 1048576 --match-capacity 1048576

python3 experiments/device_backend/summarize_matrix_sat.py \
  out/device-matrix-sat-pass3-final out/comparison-pass3 --saturation-macros \
  --tests out/comparison-tests --native out/comparison-native \
  --output out/comparison-results.json
```

Remaining high-value extensions include the structural construction in
`context` and the numeric/candidate updates in `terms` / `terms-helpers`. They
change the local carrier or invoke merge callbacks, so this fixed-carrier
equality certificate does not authorize skipping them.
