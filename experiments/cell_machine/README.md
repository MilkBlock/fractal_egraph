# RipSatCell state machine and residual packing

This experiment runs **the actual modified egglog runtime**, with the existing frozen
Math Bake library. It is not a replay-only model and does not call a learned sequence
of matches a committed effect.

## State and transition

A state is `RipSatCell type + canonical original parameter tuple`. It is represented
by a native `adaptive_cell_state_N` relation, so union/rebuild canonicalizes its keys.
It certifies completion of the cell's entire positive recipe, rather than just the
presence of its root operator. It does not replace native eclass identity.

During compilation, each source cell is checked for embedded target cells. A target
is registered only if its constructor facts **and all recipe union obligations** hold
under the source slot mapping. Registration also emits the target's outgoing interface
projections. Thus `T → U → V` keeps working when U is already included in T and its
redundant RHS is skipped. External bindings remain parameters in the residual query;
uncovered fragments continue through native matching. No empirical threshold decides
semantic completion.

There are three distinct execution certificates:

1. **Committed**: the persistent native state row exists. Once its entire parameter
   tuple is bound, Join can stop exploring the rest of that query branch.
2. **Staged**: all semantic actions have been staged, followed by the completion marker.
   Other work in the same step may reuse this promise. It is not a committed mutation.
3. **Queued**: a *complete successful match* has claimed a total positive recipe.
   Another binding may share that task only through a proved parameter permutation.
   A partial match cannot claim a task. These claims last for one step only.

The compiler obtains parameter permutations from verified self-embeddings and requires
an inverse certificate. It does not hard-code commutativity. For the supplied Math
library, for example, some three-parameter cells admit all six permutations while
others admit only the identity or one swap. The same proof also compresses persistent
state rows: emission chooses one source-slot representative, while lookup tries the proved parameter mappings. Thus union may
canonicalize keys without requiring an unsafe persistent ordering of eclass IDs.
For per-step queued work, the key is the minimum tuple under the proved mappings;
no group-closure enumeration is required. Missing mappings lose reuse,
not correctness. Fresh per-step keys avoid dependence on value ordering after union.

Only fully positive constructor recipes are supported by this runtime. Decomposed Join
plans or unresolved original parameters fall back to native execution. Failed runs must
be discarded; queued/staged certificates are not suitable for fallible or partially
committed recipes. The runtime does not silently apply this optimization to delete,
subsume, arbitrary merge functions, or unfinished RipSatCells.

## Storage contribution, measured separately

Batch packing is now the default (`EGG_LAYOUT_BATCH_CELLS=0` restores the control).
It drains private instance notifications in one update wave, keeps an existing virtual
owner when several cells overlap, and packs only residual rows. Scratch keys use native
function IDs plus canonical values; payload validation stores just the output value.
Validation finishes before packing starts, and scratch maps are dropped before the
physical encoding pass. Hashing is followed by exact key/value equality.

This changes representation only. It does not create facts, alter timestamps, or certify
an ignored shared proposal's payload. Strict `virtualize_existing_facts_batch` still
validates every proposal. State-machine pruning and storage savings are separate ablations.
Inactive state-machine queues are allocated lazily, so ordinary egglog/ripen creation
pays no allocation for them.

## Reproduce

```sh
cargo build --offline --release --example adaptive_math
python3 experiments/cell_machine/bench.py --output out/cell-machine-check --runs 3
```

The script interleaves controls, checks exact Math graph/equivalence correspondence in
separate untimed runs, and reports whole-run time, phase times, process RSS, library
size, matching, and expanded enode/eclass counts. `summary.json` records this revision's
measurements. Raw logs and graph exports stay under `out/`.

Run the actual state machine on a compatible positive Math program and matching Bake:

```sh
RAYON_NUM_THREADS=1 EGG_LAYOUT_RUNTIME_SUMMARY=1 EGG_LAYOUT_CELL_MACHINE=1 \
  cargo run --offline --release --example adaptive_math -- frozen \
  experiments/math_mini3/source.egg out/my-cell-machine \
  experiments/math_mini3/adaptive_runtime/bake_lib_round2.json
```

Outputs include `cell-machine.json` (checked embeddings and permutations),
`cell-transitions.egg` (the executable state registrations and residual rules), and
`report.json` (separate committed/staged/queued counters). With no explicit residual
specification, existing source queries are retained and completion/parameter-orbit
reuse still operates. Explicit residual interfaces can be supplied using
`EGG_LAYOUT_CELL_TRANSITIONS`; this experiment does not claim to synthesize every
possible optimal interface.

A short native Cargo benchmark isolates the machine from storage improvements:

```sh
RAYON_NUM_THREADS=1 EGG_LAYOUT_RUNTIME_SUMMARY=1 \
  cargo bench --offline --bench cell_machine
```

It compares batch packing in both cases. It checks counts; the Python experiment
additionally checks every constructor fact and the equivalence partition. Offline Bake
and compilation are excluded from runtime timing. The ordinary-native reference may
produce fewer facts at equal rounds and is not an equal-work speedup claim.

## Measured result (three independent runs, median)

All optimized/frozen variants have exactly the same constructor facts and equivalence
partition at each tested final round. Timings below include parsing, installation,
matching, and storage packing; graph verification is outside the timed runs.

| Math rounds | Previous frozen runtime | Batch storage only | Full cell machine | Total speedup | Expanded enodes / eclasses |
|---:|---:|---:|---:|---:|---:|
| 6 | 30.24 ms | 16.51 ms | 14.66 ms | 2.06× | 2,167 / 748 |
| 8 | 108.21 ms | 50.12 ms | 40.45 ms | 2.67× | 15,493 / 5,150 |
| 10 | 6,235.45 ms | 2,306.97 ms | 2,085.56 ms | 2.99× | 853,396 / 198,463 |

The main-rule match outputs fall from **13,213 to 4,173** at 8 rounds and
**563,474 to 222,763** at 10 rounds. These are match outputs, not committed mutations
or an exact count of Join probes. The separate Cargo benchmark compares batch packing
in both modes: **48.90 → 40.55 ms (1.21×)** at 8 rounds.

Merely adding completed-state checks is insufficient: at 10 rounds it takes 3,083.91 ms,
slower than batch storage alone. Staged promises alone take 3,206.43 ms. Sharing queued
symmetric jobs improves this to 2,738.65 ms; compressing the state rows brings it to
2,085.56 ms. State rows decrease from **1,705,267 to 719,435**. This is why the complete
ablation, rather than just counting skipped matches, matters.

Peak process RSS at 10 rounds is **673.7 MiB previous frozen / 644.5 MiB batch-only /
983.6 MiB uncompressed machine / 692.6 MiB compact machine**. The final machine is still
about 2.8% above the previous frozen runtime: this is a speed improvement, **not a net
memory-compression result**. The Bake JSON is 22,017 bytes; its file hash and shared
binding/template payload accounting are in `summary.json`. Payload accounting excludes
native indexes and is not RSS.

Ordinary native execution at 10 rounds is 75.00 ms, but has only 136,446 enodes and
58,464 eclasses; the baked runtime has already produced 853,396 / 198,463. **These results
do not establish that this runtime beats ordinary egglog at equal work.** They establish
an approximately 3× improvement over the previous frozen RipSatCell runtime.

Validation: 56 focused tests passed, including T→U→V chaining, source-union-enabled
transitions, compact-state union rekeying, rejected unproved union certificates,
failed partial matches, 1/4-thread work sharing, exact residual packing, and the existing
DABCABC/branching reentry regressions. A separate four-thread Math8 run also passes exact
graph correspondence (15,493 enodes, 5,150 eclasses).
