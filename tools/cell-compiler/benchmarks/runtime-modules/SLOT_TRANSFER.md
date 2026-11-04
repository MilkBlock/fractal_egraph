# Passing producer slots to scheduled RHS frames

The Rust compiler emits a destination slot and a producer operand for each new binding supplied by an unambiguous sibling effect in a multi-stage route. It normalizes SSA copies and unifies the consumer pivot and subsequent atoms against producer columns, including repeats and literals. Pivot slots are already bound. The loader checks the emitted assignments against the typed recipe and derives them for older artifacts. The same code handles every program.

The standalone CPU engine consumes these proofs in the consumer's scheduled Run. On ordinary storage, a completed producer can supply the sibling's epoch and witness plus only the required operands. An inverse read still needs its original singleton check. Bound slots must agree, and the ordinary matcher still checks semi-naive pivot ownership. Missing, hidden or relocated sources and unresolved candidates follow the existing paths. Private input values can move into the sealed RHS binding. Shared entry packets remain immutable.

For a one-table consumer reached by Spine, the executor can pass the physical producer fact as the RHS frame's parent. This requires the live owner to match the producer template and operation in the compiled route. The existing frame layout projects eligible slots by column, and the graph retains the parent for stored children. The scheduler still seals a complete logical binding for ordering, deduplication and auditing before applying any RHS. A parent-backed frame avoids owning projected fields but reads them through the parent as needed.

In the regression, producer instruction 1 constructs `F(x)` in slot 3 and instruction 2 constructs `G(x)` in slot 4. The next consumer Run matches `g = G(x), f = F(x)` and executes `S(g, f)`. Entering on `G` binds the consumer's `x` and `g`. The compiler emits `consumer[2] ← producer[3]` for `f`. If the sibling `F` is visible in the very same Cell, the consumer fills that one slot and carries its witness. If another Cell already owned `F(x)`, or it was deleted between Runs, matching consults the ordinary route. The separate rule `f = F(x) → One(f)` illustrates the single-table parent projection.

## Two frozen eggcc Pass 3 inputs

The only switch between the paired runs was slot transfer. Both used ordinary storage, planned Free Join, shared entries, Sieve, column intersection, first-resumption Spine with receipts, static constructor aliases and range guards. Each lane ran once to warm up, followed by seven fresh-process pairs for sum-bits and five for Raytrace, with alternating order. Both lanes loaded the same previously frozen artifact. The loader reconstructed its assignments from the typed recipe. Newly emitted assignments and their validation are also covered by the compiler regression.

| Work per schedule | sum-bits off | sum-bits on | Raytrace off | Raytrace on |
|---|---:|---:|---:|---:|
| Full graph Rows decoded | 42,219 | 41,696 | 16,006,459 | 15,847,710 |
| Values materialized in those Rows | 131,496 | 129,927 | 47,636,141 | 47,157,179 |
| Copies into fresh query slots | 26,588 | 25,542 | 12,940,619 | 12,620,858 |
| Copies when sealing matcher inputs | 55,586 | 54,934 | 20,803,965 | 20,547,711 |
| Owned input slots at RHS frame formation | 54,425 | 51,110 | 20,799,038 | 20,327,580 |
| Value clone calls across inference | 376,936 | 374,770 | 160,127,451 | 159,396,969 |
| Candidate visits | 15,549 | 15,549 | 7,778,113 | 7,778,113 |
| Spine steps / max depth | 3,034 / 12 | 3,034 / 12 | 663,403 / 29 | 663,403 / 29 |

The untimed audit counts actual full Rows returned by graph fact or function decoding. Fresh-slot copies count assignments from the matcher into a previously unbound slot. Seal copies count owned snapshots taken by the ordinary binding builder, Spine and the shared-packet fallback. The two copy counts cover separate operations. Parent-projected slots count omitted entries in a consumer frame, including transient constructor frames. Whole-inference Value clones include nested clones and both copy boundaries. The counts have different scopes and must be reported separately.

Ordinary continuation matching accounts for all 523 and 158,749 successful sibling reads in these workloads. The equivalent multi-atom shortcut inside Spine has zero hits on both. Single-table Spine answers did carry a parent in 1,068 and 153,669 cases, removing 3,315 and 471,458 owned frame entries. A regression with two committed sibling constructors, a previously existing fact and deletion before consumption exercises the multi-atom Spine path and compares every intermediate graph and binding.

| Input | Schedule inference, off → on | Change | Whole CLI, off → on | Change | Faster paired inference samples |
|---|---:|---:|---:|---:|---:|
| sum-bits | 73.64 → 76.34 ms | +3.67% | 194.34 → 198.49 ms | +2.14% | 0/7 |
| raytrace | 31.00 → 31.57 s | +1.82% | 36.75 → 37.26 s | +1.40% | 0/5 |

Schedule inference covers execution of the whole original schedule. Whole CLI time also includes program loading, initialization, final state export and report writes. Neither includes Rust compilation or the untimed audit. On Raytrace, the optimized work covers about 0.99% of decoded Rows and 0.46% of all Value clone calls. Port refreshes, key lookups and candidate visits are equal in both lanes. Raytrace median search time rose from 13.80 to 14.05 s, and its RHS phase from 9.02 to 9.21 s. The experiment measures the combined cost of matching, moving and accessing slots; it does not separately attribute latency to each operation. Slot transfer remains opt-in.

All 1,428 and 16,883 ordered RHS digest entries match between lanes and match the frozen reference. Each digest covers the sequence of rule IDs and ordered slot/value maps in the sealed Run, before applying effects. Every timed sample also matched the frozen final graph, per-Run schedule and effect totals. Full intermediate graph snapshots were separately verified for sum-bits. Raw samples, phase medians, counters and hashes are in [the results](slot-transfer-results.json).

## Cargo reproduction

Use the same compiled file for both lanes. The timing build uses default Cargo features. The repetitions below reproduce the inference workload. For paired fresh-process timings, alternate the two benchmark invocations with one repetition per invocation.

```sh
for case_name in sum-bits raytrace; do
  cargo cell-compile --case "$case_name" --emit-free-join-entry --output "out/slots-$case_name"
  for modules in stage-spine,static-aliases,range-guards stage-spine,static-aliases,range-guards,slot-transfer; do
    cargo cell-bench --program "out/slots-$case_name/program.json" \
      --lane free-join-planned --modules "$modules" --repeat 1
  done
done
```

For the untimed copy and ordered RHS audit, run the diagnostic Cargo feature against the same artifacts. Use a fresh output directory per invocation.

```sh
export CELL_CPU_FREE_JOIN=planned CELL_CPU_SHARED_NODE_SCHEDULE=1 CELL_CPU_SIEVE=1
export CELL_CPU_JOIN_INTERSECTION=1 CELL_CPU_STAGE_SPINE=1 CELL_CPU_STAGE_SPINE_RECEIPTS=1
export CELL_CPU_KNOWN_RESULTS=static CELL_CPU_RANGE_GUARDS=1 CELL_CPU_FD_KEYS=0 CELL_CPU_SCALAR_MERGE=0
for case_name in sum-bits raytrace; do
  for flag in 0 1; do
    CELL_CPU_SLOT_TRANSFER=$flag cargo run --locked --release \
      --manifest-path engine/cell-runtime/Cargo.toml --features slot-transfer-audit -- \
      --program "out/slots-$case_name/program.json" --output "out/audit-$case_name-$flag"
  done
  cmp "out/audit-$case_name-0/rhs.json" "out/audit-$case_name-1/rhs.json"
  cat "out/audit-$case_name-0/copies.json" "out/audit-$case_name-1/copies.json"
done
```
