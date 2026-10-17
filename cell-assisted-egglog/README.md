# cell-assisted-egglog

A local research fork of the existing egglog kernel at parent repository commit
`9ea85e47824fcef4e4336fe2f2e3160d37169f73`. It runs **analyze-backed partial queries
before the original consumer Join**, then executes the original egglog RHS.
It is not yet an automatic replacement for the eggcc planner, and the tiny
controls below do **not** demonstrate a speedup.

The main workspace is unchanged. This fork lives on `codex/cell-assisted-egglog`.
The imported kernel keeps its `egglog` crate API; this executable is named
`cell-assisted-egglog`. The immutable `egglog-baseline` tag is unchanged. Nothing
was published or fetched.

## What actually runs

```text
analyze/history.json + catalog completed Cell states
  │ source-match names, parameter roles, typed facts, equalities, literal values
  ▼
finite Cell projections (compiled once, including non-root positions)
  │
native producer match + committed facts/rebuild
  ▼
Cell instance: template ID + native parameter bindings
  │ keyed validation of the Cell body; no recognition by a whole-graph query
  ▼
cached fragment bindings ─── external committed row notifications
  │                        (one bootstrap scan, then updates)
  ▼
CPU / Metal residual compatibility matrices
  │ accepted complete bindings; exact CPU check of returned shared coordinates
  ▼
original compiled RHS → native inserts / union / rebuild
```

The assisted consumer is excluded from native rule execution. Native helper rules
still run, with dependency tracing. Its original actions are compiled once into
a one-relation worklist consumer, using egglog's existing `BackendRule` machinery.
There is no second full-LHS consumer competing with the assisted path.

A logical producer match alone does **not** admit a Cell. Every required visible
Cell row and equality must be present after native commit/rebuild. Admission is a
sequence of exact, forward hashcons lookups from known parameters; it cannot
materialize missing terms. Failed admissions remain pending. Runtime recognition
is driven by producer events; it is not a scan over all possible Cell occurrences.

This implementation still stores ordinary explicit egglog facts. It is a search
experiment, not a virtual-fact storage or memory-compression claim.

## A real eggcc example

`eggcc-if` uses the existing min/max/Single subset from
`experiments/cell_spine/eggcc_if_tiny/source.egg`, retaining all branch/context/type
conditions. Training has min, max and a near miss; execution uses new `InFunc`
values. Rules and schedules are explicitly named for this test.

The min rule has **13 lowered atoms**. The two atoms reading `Get(thn,k)` and
`Get(els,k)` can come from analyzed Cells. Other atoms, including the predicate,
input positions, branch contexts and types, remain external fragments. Matching
Cell fragments does not imply the min rule applies: all remaining shared bindings
must agree. The near miss does not produce a min or max result.

The second real-rule fixture uses `Single/Get` and `Add-Gets` from eggcc
`dag_in_context/src/utility/util.egg`; only names/scheduling and an opaque `Leaf`
input constructor are added. Upstream license and attribution are retained in
`../experiments/cell_spine/eggcc_if_tiny/UPSTREAM-LICENSE`.

## Matrix meaning

A row holds the **current canonical native IDs of shared, typed binding slots**.
It does not encode a Cell's permutation symmetry or interpret an ID as a numeric
parameter. Each 32-bit ID is represented by 32 entries in `{−1,+1}`. For `s` shared
slots, a compatibility matrix is:

```text
A: left fragment bindings × (32*s)
B: right fragment bindings × (32*s)
C = A * transpose(B)
compatible(i,j) iff C[i,j] == 32*s
```

Unequal IDs differ in at least one signed bit, reducing the score. FP32 inputs,
products and sums in the supported dimension range represent these integer
scores exactly; no identity is first converted to an imprecise float. Types are
fixed by the query schema. Literal tests are checked when constructing fragments.
A zero-width interface is an all-compatible block and needs no GEMM.

`gpu.swift` is a persistent **Metal/MPS** worker, actually executed on the GPU.
256×256 tiles bound transport buffers; they do not truncate candidates. The CPU
reference evaluates the identical candidate space. Neither path performs a native
full-LHS query to generate its candidates. Returned GPU positives are also checked
by exact CPU ID equality before any RHS can execute. GPU failure is an error,
not an unreported CPU fallback. Tests compare complete GPU and CPU pair sets.

This version still enumerates the Cartesian product of fragment rows at each
stage. Multiple stages produce intermediate binding rows. Removing this work,
sharing more conditions and batching larger useful tasks are outstanding
optimizations; fewer LHS atoms alone does not establish lower total search cost.

## Correctness and coverage boundaries

- Only positive constructor/relation queries and constructor/relation/union
  actions are accepted by the native sink. Primitive LHS filters, custom function
  lookups, globals and nonpositive actions are explicitly rejected. This is not
  all of eggcc.
- All query atoms must belong to exactly one explicit part. Internal Cell query
  projections are computed from completed analyze states, not a speculative
  parser call graph. Atom partitions and the assisted rule are currently selected
  explicitly; there is no automatic bottleneck detector or partition optimizer.
- The library loader currently admits single-application triggers with a direct
  source-variable interface and an identity catalog coordinate adapter. Unsupported
  trigger shapes are not silently promoted to guarantees. Concrete literals are
  retained, so a Cell baked at index 3 is not assumed to close an index-7 instance.
- Cell projection preserves alternatives and defers equality of different symbolic
  inputs. A later native union can enable a projection that was previously false.
- The Session exclusively owns its EGraph. Each residual batch is frozen through
  GPU completion and action admission. Following a native step, certificates are
  revalidated, including subsumption visibility and canonicalized keys. This is
  conservative whole-registry revalidation, **not** fine-grained invalidation yet.
- Input facts added between steps have opt-in committed-action receipts. Ordinary
  tracing and nonassisted execution keep their previous behavior. The trace batch
  is drained; no complete history is retained by this runtime.
- Unknown/not-yet-closed instances are pending, not negative matches. There is no
  claim of native semi-naive completeness at each step. A driver round runs native
  helpers then an assisted batch; it need not correspond to one original ruleset
  iteration. Fixed-point equivalence is checked on these finite fixtures; finite
  eggcc pass scheduling requires further integration.
- The sink is a documented trusted optimizer interface. Session constructs full
  typed bindings from validated facts; it never submits arbitrary GPU guesses.
  Callers of the lower-level sink API bear the same proof obligation.

`--audit true` runs a separate native full-LHS query at each batch boundary and
compares complete binding sets. It is a diagnostic oracle, never an input to
candidate generation. Do not include it in a speed measurement.

## Verified controls

All comparisons use the **actual forked egglog runtime**. `off` runs native rules
without Cell assistance or tracing; it is not a pristine-upstream measurement.
The assisted paths include producer tracing, Cell admission, residual calculation,
original RHS execution and rebuild. `results.json` records one release execution
per control; graph export and full-LHS audits are separate from `run_seconds`.
GPU time includes first-dispatch MPS overhead; setup and input times are also
reported. These tiny runs are correctness controls, not sustained-throughput tests.

| Input | Complete graph equality, CPU and GPU | Snapshot bindings missed per diagnostic round |
|---|---|---|
| eggcc getter rules | 12 Expr nodes / 8 Expr classes; also all 3 relation rows | 0, 0, 0 |
| eggcc If min/max | 60 nodes / 52 classes, all six sorts | 0, 0, 0 |
| two neighboring Cells | 15 nodes / 9 classes | **2, 0, 0, 0** |

The two early uncovered bindings are important: the first producer wave has not
yet established the entire completed Cell body. Admission waits. They are found
once the helper closure is present, and the final graph is identical. A single
rule's ability to *eventually* form a Cell is not proof that the runtime already
has that Cell certificate.

The If run was approximately **0.565 ms native, 0.947 ms assisted CPU, 30.743 ms
assisted GPU**. This is **slower**, not an acceleration result. The important
verified change is that the consumer's full native query is replaced by proven
Cell fragments and residual wiring. Current point-read revalidation, full native
producer tracing, Cartesian stages, repeated dispatch across rounds, and small
GPU submissions remain costs. No storage-compression ratio substitutes for these
measurements. No pristine or complete eggcc pass speedup is claimed.

Counters such as `candidate_pairs` sum `left_rows * right_rows` over stages and
rounds, including initial one-row expansion. `accepted_bindings` and
`unique_dispatched_bindings` are totals over batches, deduplicated **within** each
batch; they are not distinct historical mutations. `assisted_full_lhs_queries=0`
is enforced by lane construction, not a new Free Join profiler. Audit queries
have their own counter. Native helper matches are also logical matches.

## Reproduce offline

From the parent worktree root:

```sh
CARGO_PROFILE_DEV_DEBUG=0 cargo build --offline --bin egg_layout --target-dir target/analyzer
python3 cell-assisted-egglog/validate.py --output out/cell-assisted-new
```

The script builds the release executable, freshly runs analyze for three small
programs, builds the Metal worker, compares full typed graphs, and separately
checks full native binding sets. Use `--cpu-only` where Metal is unavailable.
Output directories must be new; no prior results are overwritten.

```sh
CELL_METAL_WORKER="$PWD/out/cell-assisted-new/metal-residual" RAYON_NUM_THREADS=1 \
  cargo test --offline --manifest-path cell-assisted-egglog/Cargo.toml \
  --target-dir target/cell-assisted
```

There are 16 passing tests: Cell/boundary behavior, union, late fact insertion,
subsumption, alias-dependent non-root projections, duplicate instances, malformed
partitions, unsupported guards, false GPU positives, real Metal cross-tile equality,
and five reused native tracing regressions. Without `CELL_METAL_WORKER`, the Metal
integration test does not execute; the verified run supplied it.

Inspect a lowered rule or run it directly:

```sh
target/cell-assisted/release/cell-assisted-egglog \
  --analysis out/cell-assisted-new/eggcc-getters/analyze \
  --consumer route-get --mode inspect

target/cell-assisted/release/cell-assisted-egglog \
  --analysis out/cell-assisted-new/eggcc-getters/analyze \
  --consumer route-get --native single-get \
  --parts '[{"atoms":[0],"cell":true},{"atoms":[1],"cell":false}]' \
  --input out/cell-assisted-new/eggcc-getters/input.egg --rounds 3 --mode cpu
```

`--mode off` selects the native control. `--mode gpu --gpu-worker PATH` uses Metal.
`--rounds` counts explicit driver rounds, not a promise of native semi-naive
iteration equivalence. The small checked-in `fixtures/analyze` directory is the
actual analyzer output for the getter fixture, retained for fast regression tests.

## Code boundary

- `../egglog/src/cell_assist.rs`: typed native query schema, positive-action sink,
  exact visible reads, selected native rules, diagnostic binding oracle.
- `src/lib.rs`: analyze import, historical producer-to-Cell admission, symbolic
  projections, frozen residual batches, evidence revalidation.
- `src/matrix.rs` and `gpu.swift`: CPU reference and GPU dot products.
- `validate.py`, `fixtures`, `tests`: reproducible actual-engine controls.

The next implementation target is narrower runtime maintenance and larger shared
residual programs. Automatic rule selection, arbitrary primitive/function guards,
all catalog coordinate adapters, and production eggcc pass integration remain
explicitly outside this first executable slice.
