# Individual runtime optimizations

Actual saved raytrace Pass-3 runtime, all 547 original rules, the same stable
choice policy and original Run boundaries. Each configuration ran once.
Unchanged CPU and GPU-off controls came from `out/pass3-compact-layout`.
No pristine speedup claim. Compact machine-readable results and fingerprints:
[individual-optimizations.json](individual-optimizations.json).

## The eight cost pools

| Priority | Change tested | Disposition |
|---|---|---|
| 1. GPU matching | Compile each pure constraint to its first fully bound stage; reuse one binding across Join backtracking | Enabled. Same-binary workspace ablation: **22.956 → 21.265 s**, device **7.192 → 6.280 s**. This reuses an ancestor binding within a search lane; it does **not** implement cross-rule partial-query materialization. |
| 2. Native effects | Borrow already canonical bindings during preflight; allocate only for changed aliases | Enabled. Native effect time 4.222 → 4.156 s in the first experiment; no isolated end-to-end improvement established. Every binding/guard is checked before any worklist write. Rebuild itself is unchanged. |
| 3. Joint closure | Partition the keyed row mirror by native table instead of scanning the whole mirror for each constructor | Enabled. No second membership index or duplicate row storage. The initial experiment reduced the closure pool from 4.009 to 3.736 s; complete closure time still fluctuates around 3.8 s. |
| 4. Commit ingestion | Move the original event batch; only clone live payloads needed by both mirrors. Resolve table names once per batch and remap in place | Enabled. Final event pool 2.030 s versus the historical 2.254 s. An invalidation-dedup experiment found **1,818,752 input / 1,818,752 distinct** notifications, so that extra set was removed. |
| 5. Host/worker | Preallocate packets; decode query IDs through a direct map; stop allocating slot-description vectors per binding and cloning receipts | Enabled. No isolated timing claim for these small changes. Batching Metal encoders preserved correctness but took 23.477 s versus 23.348 s; reverted. |
| 6. Stable tie keys | Share immutable constructor labels; fast-path identical key IDs in min | Enabled. Full structural comparison, collision checks and stable ordering are unchanged. No measurable overall speedup claimed. |
| 7. Cell frontiers | Filter numeric guards in place instead of allocating a second frontier | Enabled. Complete frontier contents and ordering unchanged. No independent speedup claim. |
| 8. GPU table maintenance | Reuse cleared index/event allocations during rebase | **Reverted**: 21.259 s versus 21.265 s is not a meaningful improvement. The previously measured device maintenance pool was only 0.131 s. |

Small host changes were tested together in the final workspace ablation. Do not
sum stage differences or attribute all cross-run variation to code changes.
The same-binary constraint-only ablation was 22.639 → 21.884 s, device
7.469 → 7.221 s. The later workspace ablation includes the same host changes on
both sides.

## Why a single binding is enough

The old executor stored four `atoms × slots` arrays per lane: native IDs, low
integer bits, high integer bits, and binding status. The new executor stores
one copy of each and a cursor stack. Its working state changes from
**O(atoms × slots) to O(atoms + slots)**; this is temporary GPU query workspace,
not graph-storage compression or an RSS ratio.

At a fixed Join position the typed query determines whether each slot is:

1. unbound;
2. bound to a native ID;
3. a computed exact integer whose native ID must still come from a row.

The compiler stores this information in two bits per slot per stage. A
successful descendant can only fill an unbound slot or supply an ID for the
same computed integer. It cannot change an ancestor value: row unification and
primitive checks reject that branch first. Backtracking therefore restores the
status mask, not every value. Failed partial binds obey the same invariant.

Primitive dependencies are topologically scheduled at each stage. Overflow and
guard failure still reject the binding; modulo remains exact signed i64.
Every returned table-bound slot has its actual native ID. Unit queries and
unsupported schedules retain the general executor. Cell segment jumps also
retain that executor; the compact path does not assume their traversal order.
The static masks add bounded query bytecode, not a record per match.

## Runtime results and limits

| Configuration | Inference s | Device s | Initialization s |
|---|---:|---:|---:|
| Historical correct Cell-frontier implementation | 22.636 | 7.287 | see saved report |
| Same-binary compact workspace disabled | 22.956 | 7.192 | 19.389 |
| Compact workspace enabled | **21.265** | **6.280** | **27.075** |
| Additional index-buffer reuse experiment | 21.259 | 6.277 | 3.509 |

Initialization includes query compilation and worker/shader setup. New shader
variants had substantial cold initialization cost. The index experiment reused
the already compiled query shape; its lower setup time is not an index
optimization benefit. **These are inference improvements, not a cold-start
end-to-end speedup claim.** Rust builds, graph export and validation are outside
inference timing as required by the benchmark skill.

All measured variants retain exactly the same per-rule submitted binding counts,
2,458,173 native row notifications and 239,476 union notifications. These are
not counts of novel facts. Cell coverage is unchanged: 30,566 supplied query
frontiers, 1,918,606 bindings, 12 joint dataflow boundaries and zero fresh-DAG
macro instances. The new GPU gain is a matcher implementation improvement,
not additional Cell sat-skip coverage.

Validation:

- Complete same-commit CPU/GPU facts, values, equivalence and visibility pass.
- Cell-on/off complete graph gate passes.
- Full independent native frontier audit: **478,136 queries**, complete.
- Production Tiger extraction: same-commit CPU/GPU selected DAGs pass.
  Fused-native/GPU selected DAGs still fail the pre-existing separate gate.
- Regression suites cover finite Run, updates, visibility, union, failed
  arithmetic, conflicting/stateful merges, deleted facts, missing ports, and
  backtracking through both rejected and successful siblings.
- The pre-existing difference from fused-native execution is still present.
  The fused-native and same-commit Tiger gates remain separate.

The remaining large opportunities are cross-rule common fragments, native
rebuild, and the 3.8 s joint closure dataflow. None is eliminated by this change.

## Reproduction

Use the repository's `cell-assisted-egglog-bench` skill. Current defaults enable
staged constraints and compact binding workspaces.

```sh
python3 skills/cell-assisted-egglog-bench/scripts/bench.py run \
  --all-rules --joint-sat --reuse-controls out/pass3-compact-layout \
  --output out/my-optimized-pass3
```

`CELL_COMPACT_BINDINGS=0` disables only the compact workspace.
`CELL_STAGED_CONSTRAINTS=0` restores dynamic constraint scheduling and disables
the compact workspace for programs that have primitives. Original matches,
native effects and commit boundaries remain identical.

Rejected worker experiments are preserved as small source patches:
[batched-encoder.patch](batched-encoder.patch) and
[reused-index-storage.patch](reused-index-storage.patch). Apply one at a time;
their enabling environment variables are in the patches. Neither is installed
in the default worker. The interrupted `out/opt-encoder-batch` run used an old
worker and is excluded from every result above.
