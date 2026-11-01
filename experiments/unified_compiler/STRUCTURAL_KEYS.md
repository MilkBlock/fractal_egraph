# Shared structural keys and merge contracts

## Scope

Native egglog, the standalone Cell CPU engine, and resident Metal now execute
the same structural-key ordering and source-derived merge contract through
`source.egg → source_sk.egg → source_sk_mk.egg → eggfuse_related_files`.
The tested SK pass selects an actual function; the marker test creates two
actual markers. An empty pass is not evidence of support.

The CPU engine remains a standalone interpreter/storage implementation. The
native egglog process is the oracle, not its execution engine. Metal owns its
key construction, comparison, merge reads, and selection; it does not receive
CPU-computed winners.

No Raytrace Pass 3 was executed. The integration workload is the frozen
**sum-bits Pass 3** from `experiments/profile_suite/suite.json`.

## Semantics and architecture

* `engine/structural-key` owns the Rust key type, exact order, literal encoding,
  DAG codec, and language-neutral ABI. Native and Cell CPU use this type.
  Metal implements the same ABI in `engine/metal/structural_keys.metal`.
* The order is anchored-first, height, UTF-8 constructor/literal label, then
  ordered children. Hashes accelerate equality but never decide collisions.
  Keys describe anchored structural witnesses, not canonical forms of entire
  eclasses. i64 labels use the existing decimal-string order; score comparison
  uses exact signed integer arithmetic, independently of that tie-break order.
* Transport preserves a DAG rather than expanding repeated children into a
  tree. Comparing independently restored DAGs memoizes equal node pairs on CPU.
  The depth-60 repeated-pair test uses 61 records, not 2^60 expanded leaves.
* `eggfuse/ir/structural_key.py` derives `merge-contracts.json` from the typed
  expression: constructor congruence, unique value, pure lattice, key-min,
  ranked selection, or ordered callback. It records function reads separately
  from constructor effects, plus equal-input behavior.
* Both artifact loaders check the contract against the source. The Rust engine
  independently checks expression, classification, reads, constructs, and
  equal-input behavior. Metadata cannot simply declare an ordered merge safe.
* A ranked choice reads the actual score/key ports. During repair, displaced
  keys are canonicalized; pending collisions can be folded only for recognized
  pure lattice merges. An ambiguous non-lattice read is an error, not an
  arbitrary choice. Ranked selection does **not** grant unrestricted reordering:
  that additionally needs a frozen port view and equivalent equal-rank candidates.
* `Key` is a base value, not a UF ID. Full key DAGs are retained in snapshots,
  checkpoint/resume, comparison, and GPU-to-native import.

Native bug fixed separately in `ead5232`: a nested function read in a merge
incorrectly returned outer `old` whenever `old == new`. For
`(+ (Lookup old) new)`, `Lookup(1)=10`, and inputs `1,1`, this produced 2 instead
of 11. Only the original **top-level** function merge keeps its equal-input
shortcut; nested reads now evaluate. Two native regressions cover numeric and
structured-key/score reads.

## Verification

Recorded results are in `structural-key-results.json`.

| Source fixture | Active choice | Markers | Exact native/CPU/Metal endpoint |
|---|---|---:|---|
| key-spine.egg | Winner | 2 | 45 rows, 20 eclasses |
| key-values.egg | Winner | 0 | 17 rows, 8 eclasses |
| key-refresh.egg | Winner | 0 | 11 rows, 4 eclasses |
| key-depth.egg | Winner | 0 | 17 rows, 8 eclasses |

The endpoint check includes facts, function values, equality partition,
visibility, and exact key values. It excludes only generated assertion probes.
The cases include large i64 scores around 10^16, escaped/non-ASCII strings,
f64 signed zero, anchors, rank changes, union repair, and CPU checkpoint/resume.
Each Metal case also passes a fresh-process per-Run replay.

Additional checks: shared Rust key tests; native merge tests; CPU runtime tests;
31 Python contract/compiler regressions; the 50 existing CPU/native fixtures;
Boolean merge and nested port-read cases; rejection of forged merge contracts;
GPU snapshot import into native with zero inference or rebuild operations.
The legacy Swift worker was typechecked after assembling its companion sources.
It was not separately benchmarked with SK.

## Small Pass 3 integration, not a speedup result

Historical resident-core times were used **only to select a small workload**:
fact 0.325s, loopfact 0.359s, sum-bits 1.054s. That configuration does not have
the current source SK/marker transformations.

The full sum-bits pipeline selects `to-hoist`, creates **48 markers**, and
finishes on both targets. CPU executed 5,115 key operations and 28 merge-port
reads; Metal executed 4,110 key operations and 12 merge-port reads. These are
operation counters, not distinct keys, logical matches, or committed mutations.

Metal completed 1,459 Runs with per-Run replay equality and numeric/publication
audits. The measured integration invocation took **3.571s inference**
(3.232s device time). CPU's integration invocation took **0.556s inference**.
These are different engines with different endpoint behavior, not a clean
performance comparison or a demonstrated speedup.

Full endpoint equality with native **failed**:

| Engine | Final rows |
|---|---:|
| Native | 10,186 |
| Cell CPU | 10,339 |
| Resident Metal | 10,267 |

Row-count differences occur in `Smaller`, `CellSpineView10`, and
`CellSpineApplied254/256/258`. Excluding these five tables still does not pass
the equality comparison: CPU finds a different TermBop row, and Metal finds a
different equality partition. This is retained as an unresolved integration
limitation; it is not dismissed as harmless instrumentation or attributed to
old code without a first-divergence investigation.

Cold Metal compilation was **3,237.803s**, despite the small fact input:
the full ruleset/marker program has 776 rules, 946 templates and a ~14MB shader.
With the same shader and a saved binary archive, the next invocation had
100 pipeline-cache hits and zero misses, but source-to-library preparation still
took **48.367s**. These costs are reported separately from inference.
`EGGFUSE_METAL_CACHE_ONLY=1` now refuses a pipeline cache miss; it does not remove
source-to-library preparation. Never silently repeat the cold run for a quick test.

## Reproduction

Small, strict integration fixtures (never run Raytrace):

```sh
python3 experiments/unified_compiler/check_keys.py \
  --solver-python /absolute/path/to/solver-venv/bin/python \
  --output out/shared-key-check
```

Use the existing solver environment supporting the marker selector. Build the
usual native frontend/analyzer tools first; the script builds the CPU backend.
It invokes the unified compiler for both targets and requires nonempty SK.

For the small Pass 3, assemble `sum-bits` using the three manifest `parts`,
prepend the explicit policy below, and use the existing small `bake.parts` with
the same policy plus `(run boundary-analysis 1)`. This last training Run makes
the choice active in Bake; it does not modify the workload's execution schedule.

```egg
; cell-choice/v1 {"function":"to-hoist","score":"Expr-size","order":"max","tie_break":"structural"}
```

Run the ordinary compiler's SK pass on that Bake source, then use its
`source_sk.egg` as `--bake-source` for the workload. Both targets use
`python3 -m eggfuse.compiler`, the same source/Bake environment, capacity 65536,
match capacity 16384, and `--multi-stage-cell`; select `--backend cell-cpu` or
`--backend gpu`. Default routine checks should stop at `--stop-after device`
until an appropriate Metal archive exists. To reuse the measured GPU archive:

```sh
export EGGFUSE_METAL_CACHE="$PWD/out/shared-keys-metal-cache"
export EGGFUSE_METAL_CACHE_ONLY=1
```

Saved evidence: `out/shared-keys-final-suite/results.json`,
`out/shared-keys-final-cpu/`, `out/shared-keys-sum-bits-cpu-final-run/`, and
`out/shared-keys-sum-bits-metal/pipeline.json`. Binary archives and generated
programs remain untracked. Relevant summaries and fixture sources are committed.

## Remaining execution limits

Metal key interning currently scans its immutable DAG arena; key operations and
merge-port reads run in the ordered executor. This is a correctness integration,
not a parallel-key or tensor-core optimization. Arena exhaustion reports an
error; dynamic key growth/collection is not implemented. Explicit key primitives
in query bodies are rejected because parallel matching cannot share the ordered
comparison scratch. Generated SK rules publish their keys in RHS and are supported.
Existing restrictions on top-level mutations after schedules remain unchanged.

Next performance work should address shader size/compilation and exact indexed
key/port lookup, while retaining ordered treatment of state-dependent callbacks.
The sum-bits native endpoint difference needs its own first-divergence audit
before any native-equivalent speedup claim.
