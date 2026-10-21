# A real Pass-3 hotspot: reuse the loop boundary before GPU search

This executes the original complete raytrace Pass-3 schedule in the existing
CPU/GPU runtime. It does not modify the original `.egg`, native kernel, GPU
matcher or RHS. The existing joint terms proofs are enabled in both controls.
It is **not a new proof learned by the five-state Analyze library**: the new
boundary program is compiled from the selected original typed rule.

## Why this query

Saved native timing puts the `PotentialExtractedExpr` cost update, `Smaller`
comparisons and term construction near the top. Several already have a Cell
execution path, so native timing alone is not a list of remaining GPU problems.

The GPU profile identifies a different dominant query:

```egg
(rule ((= loop (DoWhile inputs pred-and-body))
       (ContextOf inputs loop-input-ctx)
       (ContextOf pred-and-body loop-output-ctx)
       (= constant (Const c out-type loop-output-ctx))
       (HasArgType inputs in-type))
      ((lsr-inv loop (Const c in-type loop-input-ctx) constant))
      :ruleset always-run)
```

It visited **31,846,434 live candidate rows**, about 75% of the measured GPU
visits. Across its anchors, depth 1 emitted 19,319,615 partial bindings; depth 2
retained only 81,103. These are repeated work counts, not distinct failed matches.

## The execution change

```text
DoWhile commit → one live root instance
                    │
                    ├─ ContextOf(inputs, input-context)
                    ├─ ContextOf(body, output-context)
                    └─ HasArgType(inputs, input-type)
                              │
Const changes under output-context → wake subscribed roots
                              │
                 complete original LHS bindings
                              │
           original sorted commit, alongside other GPU results
```

The four ports are obtained in that order; all applicable rows are retained.
No assumption that a context/type lookup is unique is made. A partially bound
port uses a shared exact index over its known columns. Changed rows, removals,
canonicalization and missing-key arrivals wake dependent instances. The native
commit feed, including rebuild, is the authority; no diagnostic trace is stored.

When preceding witnesses are older than the rule watermark, only new terminal
rows can create a new incremental binding. A timestamp index enumerates those
rows directly. The filtered subset never becomes a cached full-port answer.
When a context or type itself changes, all applicable terminal rows are considered.

The CPU supplies the **complete incremental query answer**, including an empty
answer when justified. The original RHS and guards execute normally. This does
not skip ordinary Run rounds, invent saturation, or equate unseen Cell states
with impossible matches. Unsupported configured queries fail admission rather
than silently supply incomplete answers. Without the option, the usual GPU path
is unchanged.

Admission currently requires one unambiguous constructor root, a seminaive
structural query, and at most 64 columns per atom. Frozen equality/container
constants are rejected until their canonicalization contract is supported.
Existing v1 transfer artifacts retain their key/output-read contract; these
partial-key programs use the source selector and are compiled against the live
schema. Read order is a structural policy, not a globally optimal join plan.

## Same-binary, single-run result

| Metric | Boundary program off | On |
|---|---:|---:|
| Complete inference, including CPU maintenance/effects | 13.386659 s | **12.455758 s** |
| Device execution | 2.858860 s | 2.068191 s |
| Target GPU candidate rows | 31,846,434 | **0** |
| Total GPU candidate rows | 42,251,486 | 10,405,823 |
| Original target bindings | 123,254 | **123,254** |
| Target GPU query dispatches | 142 | 0 |
| CPU target candidate-row examinations | — | 126,320 |
| CPU target fresh port lookups / cached port reads | — | 1,346 / 2,814 |
| Terminal old rows avoided by the CPU delta index | — | 1,048,146 |
| Current root instances / dependency edges added | — | 84 / 336 |
| Native rows monitored by the CPU service | 265,102 | 303,422 |

Net improvement: **0.930901 s / 6.95%**. This includes the new indexes, event
handling and CPU query work. Setup, build, graph export/checks and Tiger are
separate. Profiling counters were collected in separate runs and are not timing
evidence. No averaging or selection of a fastest repeat was performed.

There are 1,798 CPU query requests, only 21 of which return nonempty batches.
Do **not** claim 1,798 GPU searches were saved: the old backend already had 1,656
quiet runs and one empty-table prune. The binding count stays unchanged; the
optimization removes search, not original rule effects. CPU and GPU row counters
describe different execution paths and are not directly subtractable costs.

The target had 73,660 GPU seeds before and none after. Its previous conditional
survival probabilities by depth were `[1, .77848, .02123, .02009, .02009]`; after
the query moves off device these probabilities are **undefined**, not zero.
Other queries together visited 771 more rows; seed ownership/retry distributions
can change. The comparison artifact reports per-rule counts and fixed-before-rule
weights for queries remaining on GPU; it does not claim fixed candidate identity.

## Correctness and attribution

- Full facts, values, equivalence and visibility match the same-commit CPU
  control and the existing GPU/Cell-off control.
- 32,364 Cell-supplied query checks against the native incremental oracle pass,
  including 1,798 for the new boundary program.
- Production Tiger's selected DAG matches the same-commit CPU output.
- The pre-existing fused-native graph/Tiger difference remains. The extraction
  command preserves that failure status and records the separate CPU/GPU pass.
  This is not a claim of pristine-native equivalence or a 7× native speedup.

The credit belongs to this source-derived boundary execution group, conditional
on the existing backend. It is not evidence that the old Analyze catalog has
improved, nor a new RipSat saturation theorem. The selector file is not a
self-contained Bake library: native source/schemas are still required and the
typed port plan is compiled at setup. Additional CPU indexes/rows cost memory;
RSS and index bytes were not measured, so no memory-compression claim follows.

## Reproduce with the skill

```sh
python3 skills/cell-assisted-egglog-bench/scripts/bench.py hotspots \
  --input out/analyze-credit-depth-reuse --native out/pass3-marker-native-clone/run \
  --output out/hotspots-new.json

python3 skills/cell-assisted-egglog-bench/scripts/bench.py run \
  --all-rules --joint-sat \
  --boundary-programs experiments/pass3_boundary_programs/loop-context.json \
  --reuse-controls /path/to/verified/pass3-compact-layout \
  --output out/boundary-new
```

Omit `--boundary-programs` for the same-binary control. Add
`CELL_SEARCH_COUNTERS=1` only for separate diagnostics. Use `audit --all-rules
--joint-sat --cell-only --boundary-programs ...` for the oracle and
`boundary-credit --before ... --after ... --profile-before ... --profile-after ...`
for the saved comparison. All source selection and file content enter the cache
identity. This remains explicit/opt-in, rather than a hardcoded DoWhile kernel.

Native regressions cover missing ports, multiple contexts, alias union, deletion,
reinsert and hidden roots. Existing joint/scalar/transfer tests remain passing.
