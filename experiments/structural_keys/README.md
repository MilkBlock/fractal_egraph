# Full structural keys: a small real eggcc case

**Result: final choices stabilized in this case; complete graph order-independence
was not established. No analyzer determinism option was added.**

Input is the complete `fact` Pass-3 snapshot exported from production eggcc
`ae80f82a`, with all its original declarations, rules and main schedule. This is
one frozen Pass replay on the local egglog fork, not three passes rerun from
Bril and not a GPU benchmark. The source and its checksum are committed here.

## Changes under test

- Generate shared structural keys for all equality sorts produced by constructor
  declarations, including lowered relations; not only those reachable from Expr.
- Preserve the existing input-anchor scheme and exact structural comparison
  after hash equality. No eclass-ID or hash-only tie-breaking.
- Maintain keys before each original Run and once at the end.
- `to-hoist`: maximum size then structural key.
- `Smaller`: minimum cost then Term key; allow a later equal-cost but better-key
  candidate to update `ExtractedExpr`.
- `IVTMin`: minimum length then the whole IVTAnalysisRes key.
- Unknown container fields are never encoded by their allocation IDs. An unused
  declaration is allowed, but an actual unsupported constructor instance raises
  an error. The first diagnostic run exposed `ExprSetPrim`; this sample does not
  instantiate it.

The `policy-rules.egg` generated per lane records selection rewrites. It requires
the existing Rust structural-key primitives and generated helper rules, plus the
maintenance schedule. It is not a standalone pristine-egglog program.

## Single-run observations

| Variant | Main execution | Strict final graph vs full-key reference | Actual Tiger DAG vs full-key reference |
|---|---:|---|---|
| Original policy | 17.62 ms | Different policy; different graph | Not used as the determinism reference |
| Full keys + stable choices | 96.06 ms | Reference | Reference |
| Same configuration, fresh process | 91.26 ms | Pass | Pass |
| Four-thread configuration | 98.83 ms | Pass | Pass |
| Reverse contiguous rule declarations | 110.40 ms | **Fail: Smaller** | Pass |

Each lane runs once. The perturbation runs are correctness checks, not samples
for averaging. Four configured threads do not prove this tiny workload actually
executed parallel work. Main execution includes full key maintenance and the
changed selection policy; it is about 5.45 times the original in this case.
This is not a measurement of hash computation alone. Initialization and exports
are separate; full-key setup was about 134.82 ms.

All **811 final constructor-output eclasses across 36 populated sorts** have a
key. This includes the lowered relation sorts. It does not establish key
availability at every intermediate point or support for every declared container.

Production Tiger selected identical constructor DAGs for `fact` and `main` in
all four full-key variants (61 serialized rows before reconstruction). A separate
strict graph diagnostic finds that excluding `Smaller` makes the reverse-order
comparison pass. This projection is diagnostic only: **Smaller is a real table
read by rules, so we do not remove it from the acceptance gate.**

The difference demonstrates the distinction between choosing a stable winner
and constructing a stable history of intermediate merge nodes. Structural keys
do not automatically canonicalize `(Smaller a b)` versus other merge histories.
We have not implemented normalization of that history, and do not claim a
universal graph-determinism guarantee from successful replay or extraction.

## Reproduce

```sh
python3 skills/cell-assisted-egglog-bench/scripts/bench.py structural-keys \
  --snapshot experiments/structural_keys/fact-pass3.egg \
  --output out/fact-structural-keys-new
```

Use Python 3.9+. The skill builds the example and the existing Tiger output
checker, runs five isolated processes and production Tiger, and saves raw graphs
and exact controls under `out/`. It returns nonzero when the strict determinism
gate fails, even if extraction succeeds. No analysis, trace capture or GPU worker
is started. Read `results.json` here to reuse the recorded result.

Focused tests cover late same-cost choices, IVT ties, extra constructor sorts,
unsupported instantiated containers and the existing stable-hoist behavior.
The primary runtime and analyzer are unchanged. The implementation is exposed
only as an experimental library helper and benchmark, in an isolated worktree.
