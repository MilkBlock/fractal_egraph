# Native CPU Cell engine — 2026-10-31

Actual local egglog, one native thread, release build. Each row is one pair of
fresh processes using the same binary; no averaging or GPU execution. These are
**inference times**, including completion-table maintenance, excluding source
installation, analyzer/native compilation, serialization and graph comparison.
The source/binary hashes and separate setup costs are in `results.json`.
Preexisting local kernel edits were preserved; the control is not pristine upstream.

| Workload | Ordinary native CPU | CPU Cell target | Inlined successors | Full endpoint |
|---|---:|---:|---:|---|
| Synthetic 28-stage × 1,024-input chain | 3.458 ms | 2.743 ms | 27,648 | equal |
| eggcc fact Pass 3 | 13.436 ms | 12.545 ms | 0 | equal |
| eggcc loopfact Pass 3 | 12.756 ms | 13.227 ms | 0 | equal |
| eggcc sum-bits Pass 3 | 50.788 ms | 50.784 ms | 0 | equal |
| eggcc Raytrace Pass 3 | 21.856 s | 21.855 s | 0 | equal |

The chain is 1.26× in this single measurement. It uses 28 original effect tables
as implicit completion receipts, no private receipt tables, and skips 55 complete
rule queries. Guarded successor invocation counts are not mutation counts.
This is a favorable small demonstration, not a general performance conclusion.

All four real Pass 3 programs currently hit the order-sensitive function-merge
admission barrier and execute natively. Their timing differences are fallback
overhead and run variation, **not Cell savings**. Raytrace has 697,922 source rows
in this frozen input/control configuration. Do not compare its timing directly
with the older mixed CPU/GPU or marker-transformed workloads.

Before that admission check, a Raytrace trial inlined 1,412 successor invocations
and took 21.903 s versus 21.933 s native, but had 697,938 versus 697,922 final rows.
The differences included Smaller, TCPair and TermConcat. That trial failed the
full endpoint test and is not a certified performance result. Local positive
closure does not establish that subsequent order-sensitive choices preserve the
same complete program state. A richer execution/choice contract is needed before
that path can be admitted by default.

Validation: 21 native semantic fixtures, 20 existing shared Recipe/region tests,
fresh-process replay, saved-plan/new-input execution, forged-source rejection,
malformed-binding fallback, and the four full Pass 3 endpoint comparisons passed.
The unique-result extraction fixture also agrees. Full reports and states are
under `out/cpu-cell-release-bench` and `out/cpu-cell-ready-verified`; only reproducible sources and this compact text
report are tracked.

The next coverage work is explicit: extend parameterized numeric/function-update
recipes and establish an order/identity contract for noncommuting merges. The
current implementation already supplies the CPU native execution seam, exact
binding handoff, completion receipts, native residual execution, and paired audit.
