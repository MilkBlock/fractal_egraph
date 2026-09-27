# Executable RipSat runtime, 2026-09-27

This is an opt-in **actual native egglog execution path**, not a simulation or
post-hoc whole-body matcher. It currently supports the positive `Math` constructor
rewrite fragment used by math-microbenchmark: unconditional rewrites, positive
initial actions, union, and finite default `(run N)` schedules. Unsupported guards,
primitive-valued pattern variables, subsume/set/delete, named schedules and other
command forms are rejected, not silently treated as saturated.

## Runtime loop

1. Native match counts identify source rewrites that actually executed. For each
   previously unseen trigger shape, run the complete rule environment on its LHS
   with fresh typed opaque inputs. Constants and repeated variable identities are
   preserved. A native unchanged step, without an observation stop, certifies a
   finite symbolic closure. The original RHS equality is also checked explicitly.
2. Compile that RipSat result into a parameterized macro. The native LHS query
   supplies actual bindings; macro actions construct/union the proven body. The
   selected combined ruleset replaces that source rule with the macro; it does
   not execute both. An unfinished proof retains the native source rule. Existing
   50 ms / 200K observation limits are reused, with no new candidate ranking/cap.
3. Macro actions emit a binding packet into a private notification relation.
   Consume and delete those notifications after the native step, then validate
   and pack existing committed rows into shared native templates/binding matrices.
   Packing does not query the full body again. Native hashcons, canonicalization,
   displaced-table rebuild and cross-boundary matching remain active.
4. Newly matched source triggers can add more finite closures at later boundaries.
   Persist each revision as `bake_lib_roundN.json` and readable executable
   `bake_lib_roundN.egg`. Reloading the typed execution library compiles macros,
   without replaying their local saturation searches. `adaptive` with an input
   library continues learning; `frozen` isolates reuse without new learning.

The admission unit here is an **observed source rewrite's symbolic LHS closure**,
not arbitrary CSCS fusion or every possible context specialization. The 13 entries
are 13 executable triggers, not a claim of 13 distinct bodies modulo graph
isomorphism. The old `rip-sat-library/v2` research catalog and this
`adaptive-ripsat/v2` execution format are different formats. No automatic legacy
catalog conversion is claimed.

## Correctness boundary

For positive constructors/equalities, substituting actual values into a derivation
from opaque parameters preserves its conclusions, including when parameters alias.
Every macro consequence was derived by native rules, and every replaced original
consequence is retained. Thus fair saturation has the same consequences; **a fixed
number of rounds need not produce the same graph**, because a macro can advance
several original inference steps at once. External facts or union can enable new
matches and are not blocked by a permanent closed flag.

This avoids repeating local RipSat search at every instance. It does **not** prove
that the global matcher never revisits internal trigger patterns. Query planning,
per-fact native indexes, instance bindings, macro output construction and cross-body
joins still cost memory/time. Virtualization currently follows committed construction;
it does not remove peak memory spent staging macro outputs.

## Measured results

`measurements.json` contains the binary hash and all raw process measurements.
Three independent processes per mini3 mode; medians below. Times include sampling,
serialization and library setup, exclude compilation, and are not the earlier
uninstrumented baseline. The internal driver timer excludes final report construction, final serialization and engine destruction; process-wall times are in the raw report. Process peak RSS uses macOS `time -l`; plotted per-round
RSS uses `ps` and is not peak RSS.

| mini3 mode | Internal driver time | Peak RSS | Final e-nodes | Final e-classes | Virtual rows |
|---|---:|---:|---:|---:|---:|
| native controller | 10.66 ms | 7.58 MiB | 208 | 116 | 0 |
| adaptive, empty library | 122.24 ms | 52.34 MiB | 259 | 118 | 223 |
| frozen round-2 library | 14.74 ms | 10.14 MiB | 347 | 137 | 311 |
| round-2 library + continued learning | 15.03 ms | 9.97 MiB | 347 | 137 | 311 |

Native rule execution medians (excluding setup, learning, packing and diagnostics):
0.672 / 0.851 / 1.383 / 1.296 ms respectively. Online local learning costs 108.43 ms;
loaded modes do no local saturation in mini3. Packing costs about 1.35 ms for loaded
modes. Third-round loaded execution has 126 macro matches and three source-rule
matches. These are native query matches, not counts of committed mutations or an
estimated number of eliminated matches.

The round-2 library contains 13 executable triggers: **22,017 bytes** typed JSON,
**6,692 bytes** readable `.egg`. It is learned using only rounds 1–2, not later
samples. Primitive parameters stay opaque only where valid; constants are retained.
The unbounded symbolic integration case is recorded unfinished and stays native.
Library retained heap is not separately measured; RSS includes it.

**No speedup or net memory reduction is established.** The loaded runtime computes
more consequences in three rounds, so these are not equal-work speed ratios. High
virtual-row coverage (311/347) is not equivalent to low total RSS.

The original 11-round math workload was also run once per mode under an external
30-second/4-GiB process-group guard:

- Native completed: 0.57 s wall, 1,047,896 e-nodes and 443,832 e-classes.
- Adaptive stopped at observed 4,346,626,048 B after 10.740 s.
- Frozen stopped at observed 4,689,215,488 B after 9.165 s.
- Loaded + continued learning stopped at observed 4,364,402,688 B after 9.402 s.

The latter three completed ten rounds, then exceeded the memory guard during round
11. Observed guard RSS is a sample, not a completed-process peak. Partial rounds
are not reported as completed or saturated. Faster propagation via macros can
increase the graph/query/output workload dramatically. The mode remains opt-in.

## Reproduce

```sh
cargo build --release --offline --bin egg_layout --example adaptive_math

# Learn and install closures during the same tier0 execution.
cargo run --release --offline -- reuse-adaptive adaptive \
  experiments/math_mini3/source.egg out/my-adaptive

# Load the round-2 library and continue learning on new triggers.
cargo run --release --offline -- reuse-adaptive adaptive \
  experiments/math_mini3/source.egg out/my-adaptive-warm \
  out/my-adaptive/bake_lib_round2.json

# Protected benchmark, including the full 11-round source.
python3 tools/bench/adaptive_math.py --output out/my-adaptive-bench --full
```

Output directories must be new. The full workload should use the protected script;
the direct CLI does not impose that external whole-process memory guard.
Open `out/my-adaptive-bench/index.html` for match-vs-RSS/e-node/e-class/library-size/
virtual-row curves. Source and macro matches are listed separately. The generated
HTML passed JavaScript syntax checking; browser visual inspection was not performed.

## Checks

22 relevant tests passed: native virtual facts (12), adaptive runtime (7), native
RipSat snapshots (2), existing single-pass provenance (1). New checks cover changed
actual bindings after reload, aliased parameters, external facts + union after
learning, environment mismatch, readable `.egg`, unfinished-proof native fallback,
and notification consumption without deleting constructor facts. This is not an
exhaustive equivalence proof for arbitrary egglog programs.
