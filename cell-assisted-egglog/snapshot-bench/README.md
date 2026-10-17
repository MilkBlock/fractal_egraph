# Real eggcc checkpoint: raytrace candidate extraction rules

Measured 2026-10-18 on the local Apple Silicon machine, one release execution.
This replaces the three-If demonstration as a **meaningful native reference
workload**. It is not a resident-GPU speedup result: most of these original rules
are not accepted by the current assisted compiler.

## Source and exact workload

- Production input: `check_statewalk/benchmarks/passing/raytrace/raytrace.rs`,
  source hash `a0cc2caf63bded3e280ea23c9395358a60f4b9fa63a19949dd28159cc6638f22`.
  The current file still matches the saved capture.
- Captured **Pass 3** input came after the actual first two passes and their
  extractions. This is not three independent runs of the original input.
- Replay the original flattened `.egg` schedule to packet **349**,
  `command1158/3/repeat0/4`, retaining all facts, canonical values, rule timestamps
  and existing plans. This is an in-memory execution checkpoint, not a JSON graph
  imported as fresh facts.
- Run its intact nested saturation, containing **14 rules**:

```egg
(saturate
  (seq
    (run terms)
    (saturate
      (seq
        (run terms-helpers)
        (saturate (seq (run terms-helpers-helpers)))))))
```

The rules construct candidate Terms and costs, update `ExtractedExpr`, and use
`Smaller`/`TCPair` to select winners. Arithmetic, comparisons, custom-function
reads/updates, merges, unions and the original schedule are preserved.

This is the modified native fork on the current branch, not pristine upstream.
The saved capture predates this measurement; the **checkpoint replay and timings
are fresh**. No new frontend run, trace, analyze, bake or extraction was performed.
The capture metadata notes that its original CLI later failed in Graphviz;
the completed production pass inputs and extraction boundaries were retained.

## Timing and correctness

One clone executes with per-row probe-counter increments disabled. Another clone
executes diagnostics with counters enabled. Both start at the same checkpoint.
Input parsing, prefix execution, cloning, snapshots and comparisons are outside
the measured target interval. Profiling still has opt-in checks in the binary;
this is not a completely instrumentation-free upstream build.

| Target interval | Value |
|---|---:|
| Wall time | **8.406 s** |
| Search + apply | **7.100 s (84.46% of wall)** |
| Insert + merge commits | **0.0678 s** |
| Congruence rebuild | **1.177 s** |
| Native ruleset iterations | **1,017** |
| Logical matches | **285,337** |
| Table rows before → after | **421,397 → 655,832** |
| Visible rows, net increase | **234,435** |
| Successful native UF displacements | **81,353** |
| Snapshot clone, excluded | **18.36 ms** |

Search+apply includes RHS staging and primitive computation. Pure Join versus
RHS time was not separated in this run. Net row growth is not an inserted-write
count or a per-match effect count; rebuild can rewrite/deduplicate rows. UF
counts include congruence and internal merges, not just explicit union actions.

The diagnostic replay agrees **exactly on all final canonical table keys,
values and subsumption flags**, and on every rule's match count. The parent
checkpoint remains unchanged. These assertions are stronger than merely
comparing total node counts. Extraction quality is not measured at this packet;
there is no transformed runtime result yet to compare against it.

## What is sparse, and what is not

| Probe path, separate diagnostic | Operations | Candidate-row visits | Rows / operation |
|---|---:|---:|---:|
| Enumerate indexed keys | 7,901,150 keys | 8,044,656 | **1.018** |
| Look up one key | 5,143,505 calls | 5,919,083 | **1.151** |

There are **5,166,731 index-handle constructions** and **13,963,739 candidate-row
visits**. These are multiple stages and repeated queries, including internal
rebuild work. They are **not** a single 49-candidates-to-one-match rejection
funnel. Near-unit per-key fanout establishes sparse *keyed join results*, not
that every relation or every equivalence class in the entire graph is sparse.

Graph structure, counted separately from declared constructor rows:

| Sort | Before nodes / classes | After nodes / classes | Interpretation |
|---|---:|---:|---|
| Expr | 69,455 / 23,155 | unchanged | about 3 nodes/class; not almost all singleton |
| Term | 6,789 / 6,789 | 62,749 / 62,749 | every Term class is singleton |
| TermAndCost | 13,175 / 7,698 | 134,798 / 62,749 | 1.71 → 2.15 nodes/class |

Other sorts are not uniformly sparse: BaseType has many alternatives. Flattened
relation encodings such as `___PotentialExtractedExprSort` are listed separately
in JSON and must not be pooled into a misleading singleton-class average.

Per-table search/apply work from the independent native read profiler:

| Table | Row positions scanned | Rows refined |
|---|---:|---:|
| TCPair | **208,745,907** | 0 |
| Smaller | **61,695,023** | 14,575 |
| PotentialExtractedExpr | **22,869,009** | 0 |
| ExtractedExpr | **2,066,289** | 584,859 |

These scans include index construction and revisits; they are neither distinct
facts nor the same counter as candidate-row visits. Per-stage row statistics are
not populated by the native plan reporter; JSON records them as unavailable.

## Original admission result (before function support)

The current assisted sink accepts only **3/14 rule bodies**, representing **226**
of 285,337 logical matches. Even that is only syntactic admission, not proven
baked-Cell coverage. They are equal-cost `Smaller`, Const, and Arg construction.
The other eleven are rejected for missing primitive predicates, custom-function
evidence, or custom-function updates on the RHS. Full definitions and exact
rejections are in `results.json`.

No guard was removed to make this benchmark fit. There is therefore **no honest
whole-ruleset CPU/GPU speedup ratio yet**. The previous tiny If timings cannot
predict performance here. This checkpoint is the reference against which broader
Cell support and the real baked transition executor should be validated.

## Reproduce

From the `cell-assisted-egglog` worktree root:

```sh
python3 cell-assisted-egglog/bench_snapshot.py \
  --snapshot /Users/mineralsteins/Repos/egg_related/egg_layout/out/raytrace-interactive-20261010/pass-3-flat.egg \
  --output out/raytrace-cell-reference-new
```

The script extends the existing `rule_repl` and builds its release target with
`native-read-profile`. It drives `:until /3/repeat0/4`, then `:bench measured`.
It does not rerun all three production passes. Use a fresh output directory;
`--skip-build` reuses the current executable. To obtain a new production capture,
use the `eggcc-pass-benchmark` skill and its existing flattening workflow first.

The REPL command works at another reached schedule position too:

```text
:until /3/repeat0/4
:bench measured
:quit
```

`bench` preserves the current cursor and parent state. It never resumes or
rewrites the schedule on behalf of the benchmark. Relevant scheduler and
snapshot regressions:

```sh
CARGO_PROFILE_DEV_DEBUG=0 cargo test --offline --example rule_repl \
  --features native-read-profile --target-dir target/analyzer -- --test-threads=1
```

The single test thread is required for process-global profiling counters.

The [function-support follow-up](../function-support/README.md) removes those
11 compiler rejections and validates all 14 query binding sets on this native
checkpoint. This does not replace the native timing above with a GPU speedup or
turn syntactic admission into certified Cell coverage.
