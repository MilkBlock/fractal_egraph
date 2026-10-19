# Cell locality and partial-query grading

Actual GPU/native runs. Controlled synthetic workload using one frozen analyzed Single/Get fragment; not whole eggcc.

Work-count run: 1024 instances, 8 five-atom consumers, 4 offers per instance.

| External-boundary tier | Partial bindings | Share | Actually reused Cell reads | Produced complete bindings |
|---|---:|---:|---:|---:|
| ready | 7,168 | 70.0% | 7,168 (100.0%) | 6,272 (87.5%) |
| suspected | 3,072 | 30.0% | 1,024 (33.3%) | 1,024 (33.3%) |
| unmapped | 0 | 0.0% | 0 (—) | 0 (—) |

Ready contexts cover 40.0% of LHS atoms: 2 of 5. They actually skip one internal lookup per ready seed, not two.
Classification performed 10,240 index probes and inspected 11,752 candidate rows. It carried 9,216 unique prefix rows and found 1,024 ambiguous seeds.
Suspected work must remain: some of it still reuses a Cell and produces valid matches. A missing route is not a proof of impossibility.

At the Cell-ready boundary, ownership certificates delegate 8,192 internal-delta seeds before deep matching. The ready-port lane produces the complete consumer bindings; these are not dropped.

| Ready-boundary execution | Context loads | Context words | Residual executions | Index probes | Complete port bindings |
|---|---:|---:|---:|---:|---:|
| cell_independent | 8,192 | 49,152 | 8,192 | 49,152 | 320 |
| cell_shared_context | 1,024 | 6,144 | 8,192 | 49,152 | 320 |
| cell_shared_joins | 1,024 | 6,144 | 2,048 | 12,288 | 320 |

Context reads decrease 8x, residual executions/probes decrease 4x, with identical outputs. These are logical work counters, not storage compression or measured physical memory traffic.

Single timing run: 8192 instances. Profiling and frontier audits OFF. Input construction and setup are excluded; input times remain in time.json.

| Variant | Make + consume (ms) | External consume (ms) | Live GPU buffers (MiB) |
|---|---:|---:|---:|
| full_gpu | 18.150 | 48.874 | 29.50 |
| cell_order_only | 13.428 | 50.339 | 29.50 |
| cell_independent | 17.054 | 50.766 | 92.50 |
| cell_shared_context | 18.548 | 49.204 | 92.50 |
| cell_shared_joins | 21.515 | 56.445 | 92.50 |
| cell_compact_seeds | 22.695 | 51.736 | 144.50 |
| cell_priority_seeds | 18.365 | 48.485 | 144.50 |

The timing does **not** demonstrate a substantial incremental end-to-end speedup over the full GPU baseline. Queue allocation/partitioning, synchronization, maintenance, native effects and transfers remain real costs. Routing adds device buffers. The strongest demonstrated result is sound classification and reduced repeated work, not a general speed claim.

All seven modes agree with complete native facts/equivalence/visibility at both boundaries. Check-mode native incremental frontier audits pass. Relevant GPU regressions: 29 tests.

During exploration, a one-step locator left 1,024 external seeds unmapped even though later matching reused Cell data for all of them. Following uniquely resolved bridges and prioritizing anchored Cell fragments fixes that missed opportunity; ambiguity still preserves every branch.

An earlier 32,768-instance initialization using ordinary per-action imports was stopped before timing. The final generator uses the native restricted ground-data loader for setup; this is outside measured inference and never substitutes for matching.
