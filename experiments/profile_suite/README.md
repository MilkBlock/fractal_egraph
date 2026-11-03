# One-command Eggfuse Pass 3 profiling

Run from the repository root:

```sh
cargo profile                         # three small complete Pass 3 workloads
cargo profile --cached-only           # summaries only; fail on any cache miss
cargo profile --fresh                 # new timings, reuse compiler/Metal caches
cargo profile --hardware              # add cached live hardware counters
cargo profile --case raytrace         # large workload, explicitly opt in
cargo profile --case raytrace --hardware
cargo profile --pipeline              # explicit, much more expensive marker lane
cargo profile --list
```

The default suite is **fact, loopfact, sum-bits**. These are real Pass 3 exports
after the production Pass 1/2 extraction chain. They retain the complete Pass 3
rules, original finite Repeat and nested Saturate schedule. They are quick
regression cases, not substitutes for Raytrace's search/memory stress. Raytrace
is included in the manifest but never selected by default; `--all` selects all
four. Inputs are frozen, so profiling never reruns eggcc's first two passes.

The user-selected default is **`resident-core`**, the same unmarked resident
configuration as the retained 162.65 s Raytrace profile. Multi-stage Cell stays
enabled. `--pipeline` explicitly selects full SK/source-marker compilation;
its caches/configuration are distinct and its timings must not be mixed with
the default. The current marker lane's first `batched_cell_pack` compilation
exceeded 18 minutes in Apple's compiler, so it is not the routine regression lane.

Output is `out/profile-suite/latest.md` and `latest.json`, with links to each
case's offline HTML dashboard. Failed runs write `last-failure.*` and retain
their logs without replacing the last successful report. `NEW` / `CACHED` and
original measurement timestamps are explicit. No averages or fastest-run picks.

## Execution and cache boundaries

The new **Rust driver** is `tools/eggfuse-profile`, a small independent Cargo
crate. `cargo profile` builds this driver, not the large legacy analyzer crate.
It does not modify any existing Python source or select another inference engine.

```text
frozen Pass 3 source
  → existing frontend → analyzer from_core → resident lowering (default)
    OR source.egg → source_sk.egg → source_sk_mk.egg (--pipeline)
  → verified multi-stage regions artifact
  → existing resident GPU engine: one clean run
  → full encoder diagnostic: original quantum/private stages, complete replay check
  → optional hardware window / capture
  → cached JSON + Markdown + HTML
```

The optional full pipeline uses the **same frozen Bake training input** for all cases, inherited
from the previous Raytrace compiler experiment. Its typed rule environment is
checked by the existing compiler. This controls Bake variation and shares the
analysis pass cache; it does **not** imply full-workload Cell coverage. The actual
execution input and schedule remain the complete workload. Better Bake inputs
can be installed explicitly in `suite.json`; their content hash invalidates the
dependent results. Full-workload Analyze is deliberately not mixed into routine
profiling: the current debug analyzer's full fact Bake ran for more than nine
minutes with growing memory before we stopped that discovery attempt.

Stages have separate content keys and successful-output checksums:

| Stage | Relevant key inputs |
|---|---|
| Compile | Exact source/Bake bytes, capacities, compiler/analyzer/IR/marker/Metal source files, native helper executable hashes, solver version, toolchain/environment |
| Clean run | Compiled artifact, runtime sources, wire-adapter hash, GPU model/core count, macOS/Xcode, execution environment |
| Encoder diagnostic | Exact saved program/result, runtime, profiling source files |
| Hardware/capture | Same reference and profiler identity, selected outer-command window, counter template |

Completed stages are reused; missing, corrupted or partially written results are
not hits. Each retry gets a fresh attempt directory. A cache lock prevents this
driver from overlapping its own GPU measurements. `--fresh` only remeasures;
it does not erase compiler history or previous runs. `--plan` displays decisions
without executing the compiler/GPU, stopping at the first missing dependency per
case. `--cached-only` guarantees that a miss cannot launch inference.

Each clean run freezes the actual worker image, materialized Swift source and
fingerprint. Subsequent diagnostic runs use that worker, not whatever happens to
be in the shared runtime directory after a code change. The existing engine's
content-addressed Metal binary archives are shared under `metal-cache/`. Cold
generated-shader compilation can take minutes; it is reported as setup time and
is not hidden inside an inference-speed claim. Repeated cache hits execute no
compiler, shader or inference workload.

Default profiling collects clean time and full encoder timing, work counts and
replay verification. Apple counter/capture tools are opt-in to avoid expensive
attachments and large traces on every edit. `--hardware` chooses a bounded
window starting at a measured hot **outer command**, not a guessed logical Run;
the real observed Run span and sample coverage are recorded. `--capture` saves
the hottest command's `.gputrace`; it does not claim Apple replay counters are
available. Use the profiling skill for embedded replay. Capture staging must be
APFS, and archival to an external exFAT disk should use the skill's tar helper.

## Existing tools and future Rust compiler

Required once for the default: Xcode/Metal, `python3` and the existing native
`target/debug/examples/eggfuse_frontend`. The optional `--pipeline` additionally
needs a marker-solver Python, `lhs_ast`, `structural_key_source`, and
`target/debug/egg_layout`.
Existing native tools are treated as **pinned binaries**, keyed by their bytes;
the driver does not silently label them as built from today's dirty Rust source.
Rebuild them explicitly after changing the native analyzer. The local solver
environment is discovered at the project's existing `out/cell-spine-venv`; set
`EGGFUSE_SOLVER_PYTHON` or `--solver-python` elsewhere. All Cargo builds are offline.

```sh
cargo profile --tools /path/to/native/examples --solver-python /path/to/python
cargo profile --pipeline --compiler /path/to/future/eggfuse
```

For the full pipeline, the future compiler only needs to accept the existing `compile` arguments and
publish the same `eggfuse_related_files` manifest/artifact. Selection, caching,
measurement, provenance and reports stay in Rust. The default's small
`COMPILE_RESIDENT` adapter similarly serializes the existing `from_core/lower`
APIs; it contains no new optimizer or inference implementation. Its portable
artifact uses the loader's `source_sk_mk.egg` filename but retains the original
source bytes and `marker_program=null`. The old Python compiler and
measurement scripts remain unchanged and can be replaced at that boundary later.

There are two explicit compatibility details, neither an inference shortcut:

1. Child processes use a 64 MiB `RUST_MIN_STACK` default (caller override honored),
   because the existing debug analyzer overflows its default worker-thread stack.
   This is recorded; RipSat's time/fact limits are unchanged.
2. The old loader compares recomputed Python tuples with JSON arrays. The small
   process-local adapter in `src/bridge.rs` converts only the expected certificate
   to its JSON wire form. Source, manifest, recipe and full certificate checks
   remain enabled. An altered certificate was tested and rejected. No old file
   or GPU operation is patched.

## Corpus provenance and verification

`inputs/common-prefix.egg` and `common-suffix.egg` are byte-common portions of the
four flattened exports. The `.eggpart` files are not standalone programs. The
driver concatenates parts and requires the original complete-source SHA-256.
This is storage factoring only, with no rewrite or schedule transformation.
`suite.json` records source paths and checksums. Eggcc's MIT license is retained
in `LICENSE.eggcc`.

- fact/loopfact: saved production three-pass audit exports from 2026-10-07.
- sum-bits: exported on 2026-10-30 by the same retained `eggcc-profile` executable,
  with `optimized-pretty-print`, `--eggcc-schedule parallel`; no coverage pass or
  Graphviz. The exported third pass was flattened with the existing `flatten`.
- raytrace: the exact `c727cdf7…` source already used by the Raytrace compiler
  and GPU experiments, not a newly reconstructed initial graph.

Encoder validation compares the full same-executor state/history, including
private multi-stage counts. It is an instrumentation/replay check, not a new CPU
whole-graph equivalence or extraction-quality claim. Physical row counts include
marker tables and must not be mislabeled as Expr e-node counts. Encoder shares
describe the instrumented run, not an exact decomposition of clean time.

```sh
cargo test --offline --manifest-path tools/eggfuse-profile/Cargo.toml
cargo profile --cached-only
```

The Rust tests cover default selection, byte-exact inputs, input/worker-key
changes, corrupt results, cache-only refusal, failed-attempt recovery, fresh-run
retention, and outer-command window coordinates. Real run/cache checks are
recorded in `RESULTS.md`.
