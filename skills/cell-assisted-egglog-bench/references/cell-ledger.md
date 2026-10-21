# Automatic Cell work ledger

Every successful `run` and `audit` writes `cell-ledger.md`, `.csv`, and `.json`
next to its report. All loaded and rejected Analyze Cells are listed, including
Cells without a compiled projection. Source-compiled programs and the joint
proof group have separate namespaces; they are not silently renamed learned
Cells. `analyze-credit --output` also writes a ledger beside the comparison.

For an ordinary uninstrumented `run --all-rules --analysis DIR`, the script
finds or collects two **separate diagnostic companions** with the same binary,
worker, input, frozen library and public execution configuration: library
plan-only and projection reuse. It reuses native/CPU controls and verified
matching diagnostics, never averages or reruns unchanged controls. No N-way
leave-one-Cell-out timing sweep runs automatically. The original timing remains
uninstrumented; companion atomic times are never speedup evidence.

`--no-cell-metrics` suppresses additional companion executions but still writes
all available rows, marking missing quantities as null. Source-only joint
programs already publish their counters, so they do not automatically cause a
new GPU profiling run. The same-library candidate work comparison applies to
Analyze projections, not to an unrequested removal of the entire CPU observer.

For saved results, this one read/derive command replaces ad-hoc parsing:

```sh
python3 skills/cell-assisted-egglog-bench/scripts/bench.py cell-ledger \
  --input out/diagnostic-reuse --baseline out/diagnostic-plan
```

`--baseline` is optional. Without an appropriate counterfactual, candidate savings
and marginal seconds remain unmeasured. `--output PATH` selects a filename stem
without an extension. Derived ledger files may be regenerated deterministically;
unrelated JSON at that path is not overwritten. The ordinary `report` command
remains read-only.

## Rows and ownership

- `analyzed_cell`: every loaded/rejected semantic Cell and its shared plan links.
  Do not sum shared-plan work again through these rows.
- `shared_projection`: one alpha-normalized fragment, all its source Cells and
  consumers, its actual work counters and hit denominators.
- `source_program`: existing CPU rooted programs; fresh/cached reads and nonempty
  complete query answers, with no automatic closure-proof credit.
- `proof_group`: closure contract consumption; independent timing requires the
  existing `cell-credit` ablation.
- `consumer`: one original query's live candidate visits before/after enabling
  reuse. Generic empty-table requests are displayed, not credited to Cells.

Connected components of the plan–consumer graph form attribution groups. Each
consumer delta occurs in exactly one group, even if several equivalent or
non-equivalent Cells justify its shared projections. A group's observed candidate
savings are not each parent's independent causal credit. No hit-weighted or
arbitrary equal distribution is performed.

## Counter meanings

| Metric | Numerator / meaning | Denominator |
|---|---|---|
| Segment hit rate | successful certified segment substitutions | attempts to use a compiled segment |
| Port hit rate | ready-port evaluations returning at least one binding | ready-port evaluations |
| Internal read cache rate | existing witness rows reused | reused rows + new certification index reads |
| CPU program answer rate | nonempty full query answers | all query requests |
| CPU port cache rate | cached reads, including cached missing keys | cached + fresh reads |
| Saved candidates | baseline live row visits minus candidate live row visits | none; can be negative |
| Net seconds | independently measured conditional time difference | never inferred from counters |

A zero denominator is undefined (null), not 0%. Unprofiled and pre-telemetry
reports retain null counters, not fabricated zeros. New per-plan counts include
certification attempts, probes, waits, publications, version invalidations,
row reuse, segment checks/hits/skipped reads, port work and version-valid watch
checks. Old/stale watch bucket traversal is reported as shared unassigned work;
it cannot safely be charged to a reused instance slot's new owner.

Unified `search_work` counts include compact matching, the general certificate
executor and port residuals. Candidate rows are live physical row visits before
binding constraints and ownership filtering, including output-buffer retries.
They are not unique rows, successful matches or committed effects. Compact
per-depth distributions retain their existing narrower scope; the unified total
does not invent missing legacy per-depth statistics.

Per-plan seconds, allocator slack, shared index memory and shared maintenance
remain unallocated without a valid measurement. The ledger reports shared GPU
bytes separately; it never labels them RSS or divides them equally over Cells.

## Validation and overhead

Worker layout 17 adds GPU diagnostic arrays only when profiling is enabled
(`CELL_SEARCH_COUNTERS=1` in this workflow). Normal runs compile the increments
away and do not allocate the per-plan/query GPU counter arrays. Host aggregation keeps u64 cumulative counts; per-packet u32
increments stop on overflow rather than silently wrapping. Plan provenance keeps
all supporting states and consumers when normalized plans merge.

The counterfactual checker rejects changed binaries, inputs, schedules, output
bindings, common CPU proof modes, generic pruning flags, libraries, or different
query layouts. Timing and diagnostics keep separate provenance. Failed graph
checks cannot receive saved-candidate credit. Tests cover shared ownership,
zero/unknown denominators, negative savings, stale controls, automatic companion
reuse, and deterministic file output.
