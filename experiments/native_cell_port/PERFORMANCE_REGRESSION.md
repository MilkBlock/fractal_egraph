# Native Pass-3 performance regression diagnosis

## Finding

The slowdown is real. It is dominated by generated structural-key maintenance,
not Tiger or the extraction export adapter. The old Cell lane still runs quickly
with the same executable, but its previously observed final-result mismatch is
not repaired by this timing experiment.

The following are fresh, single-thread, single-run measurements of the saved
complete raytrace Pass-3 program in the actual native engine. Every lane uses
the same frozen release executable. Probe atomic counters are explicitly off.
No GPU, graph export, Tiger, new workload profile, or solver run is involved.
These are **not** full three-pass CLI times, and comparisons across selection
policies are diagnostic rather than equivalence-certified speedups.

| Configuration | Execution (s) | Explicit Cell-handled saturation boundaries |
|---|---:|---:|
| Old SK, native | 23.237 | native |
| Old MK, Cell assistance | 10.517 | 12 |
| Same old MK, closure assistance disabled | 20.352 | 0 |
| Current SK, native | 54.543 | native |
| Current MK, Cell assistance | 53.288 | 0 |

Execution includes setup and, where applicable, Cell attachment. Parsing,
post-run diagnostics and destruction are outside this column. The saved
machine-readable data also contains parsing-inclusive and process wall times.
Earlier 48.39/45.98-second observations used the official executable; do not
subtract those absolute times from this independent, consistently built harness.

## 1. Structural-key maintenance accounts for most new native work

Current SK adds 31.306 seconds over old SK. The native engine's existing timers
attribute 29.104 seconds to generated key helper rulesets: about 93% of that
increase. This locates the work; it is not a semantics-preserving removal
experiment and does not prove those costs could all disappear.

| Generated ruleset | Search + RHS (s) | Merge (s) | Rebuild (s) |
|---|---:|---:|---:|
| `cell-stable-keys` | 20.226 | 5.344 | 0.018 |
| `cell-key-request-1` | 1.824 | 0.010 | 0.020 |
| `cell-key-request-2` | 1.639 | 0.006 | 0.000 |

The primitive stores and compares shared Rust DAG keys, but their preparation
still executes generated egglog joins and repeated `set`/`min` merges. A native
key primitive does not by itself eliminate those queries.

`cell-stable-keys` reports 6,692,529 logical matches. Leading constructor-key
rules include:

| Constructor | Logical matches | Search + RHS (s) |
|---|---:|---:|
| `SubTuple` | 2,187,579 | 3.402 |
| `Concat` | 734,644 | 2.303 |
| `Get` | 1,294,651 | 2.203 |
| `AddContext` | 1,372,765 | 2.038 |

These are rule matches, not new keys or committed mutations. Initial inputs,
their evolving equivalents, key requests and recursive dependencies all feed
this maintenance process. The native report's search timer includes RHS work;
this experiment does not separately time structural comparison inside it.

### A concrete misplaced initialization boundary

The generated schedule currently performs:

```text
prepare initial keys -> saturate key helpers -> freeze keys
run initialization
run the optimization schedule
```

Raytrace's input is built by a large empty-LHS rule in `initialization`. At the
freeze point the input Expr graph does not yet exist. The original position
freezes only 42 keys and **zero Expr keys**. This is inconsistent with the
intended policy of freezing the initial input graph.

One extra diagnostic swaps only those first two schedules:

```text
run initialization
prepare initial keys -> saturate key helpers -> freeze keys
run the same optimization schedule
```

| Native SK diagnostic | Current position | After input injection |
|---|---:|---:|
| Execution (s) | 54.543 | 41.824 |
| Frozen keys | 42 | 13,341 |
| Key-helper logical matches | 6,692,529 | 944,267 |
| `cell-stable-keys`, all measured phases (s) | 25.588 | 7.078 |

This reduces execution by 23.3% and key-helper matches by 85.9%. It also changes
the structural anchor policy and subsequent work: all-table tuple totals change
from 890,267 to 980,526. Those totals include auxiliary and intermediate rows;
they are not equivalent output-node counts. **This diagnostic has not passed
final winner/Tiger checks and is not installed as a production repair.** It
establishes sensitivity to the boundary, not preservation of the current ordering.

## 2. The old saturation shortcut is no longer applicable

With identical old MK source and executable, disabling closure assistance costs
9.836 seconds: 10.517 -> 20.352. The old lane handles 12 explicit saturation
boundaries; the current lane handles zero. Those boundaries include existing
parameterized dataflow/comparison handling, not twelve whole-graph constant-time
replays.

The new stable selection changes equal-cost comparison and late-improvement
rules. The old left-biased selection closure therefore fails its source contract
and is correctly rejected. Re-enabling it without proving the new contract would
restore a fast but unverified path, not solve the regression.

## 3. The replacement query layout is not the old optimized layout

The current artifact uses the first complete source-derived plan for every
admitted query. It has 493 query declarations, no shared marker tables, and was
deliberately generated for broad correctness testing without a new priced solve.
It is not the old profile-selected plan.

Cell-query time rises from 1.862 seconds in the old query-only lane to 6.273
seconds in current MK. Cell-served Runs increase from 6,399 to 11,124; total Runs
increase from 10,122 to 28,188, including generated key maintenance. The source,
workload and plans change together, so this is not an isolated layout penalty.
Within each policy, query assistance saves 2.885 seconds before the change and
1.255 seconds afterward. Do not add these contrasts as independent speedup credits.

## Repair order

1. **Correct the input-anchor boundary generically.** Identify the leading input
   construction schedule from the preceding artifact, rather than testing the
   name `initialization`. Freeze after those facts exist, document the resulting
   ordering policy, and rerun winner and production Tiger checks on both lanes.
2. **Avoid unnecessary key work.** Separate immutable anchors from derived keys;
   maintain only demanded candidates and affected dependencies. Retain explicit
   union/update invalidation and exact comparisons. Do not exchange structural
   order for allocation-ID order or hash-only equality.
3. **Recertify the stable-selection closure.** Cover cost ordering, structural
   ties, late improvements, candidate aliases and native effects before granting
   saturation-skip authority. Keep ordinary Run boundaries intact.
4. **Reprice marker/query layouts only after the key policy is fixed.** Reuse the
   exact-source complete profile cache. Broad correctness layouts should not be
   presented as solver-selected performance configurations.

The measurements identify the priorities; they do not predict a final runtime.
No candidate-visit reduction, per-Cell credit, or pristine-upstream speedup is
claimed from this experiment.

## Reproduction and provenance

The benchmark skill now supports `native-port`. It freezes the example runner,
records source/binary/configuration fingerprints, reuses intact completed lanes,
and preserves failed/incomplete results. Native per-ruleset timers omit supplied
mixed Cell batches; Cell runtime totals are therefore reported separately.

```sh
python3 skills/cell-assisted-egglog-bench/scripts/bench.py native-port \
  --fork /path/to/cell-native-egglog \
  --runner /path/to/release/examples/run \
  --before-sk /path/to/old/source_sk.egg \
  --before-mk /path/to/old/source_sk_mk.egg \
  --after-sk /path/to/current/source_sk.egg \
  --after-mk /path/to/current/source_sk_mk.egg \
  --output out/native-port-regression
```

Use `report out/native-port-regression` to inspect existing results. The report
does not rerun inference or certify extraction. See
[performance-regression.json](performance-regression.json) for exact values,
artifact locations, source/binary hashes, hot rules and the anchor diagnostic.

The runner's report is an opt-in dump of existing native timing accumulators;
no row-level profiler was added. The unrelated extraction-adapter working-tree
edit was preserved, was not called in these runs, and is outside this change.
