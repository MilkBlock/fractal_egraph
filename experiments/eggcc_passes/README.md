# Production eggcc: per-pass timing and frozen Cell-pattern coverage

For the proportion of time under `saturate`, use the new
[production schedule-boundary profiler](raytrace_saturation/README.md).
It reuses this runner with `--timing-only`, skips coverage/trace/Bake, and measures
the actual three-pass extraction chain. It distinguishes ordinary native
schedules from egglog-experimental's `run-schedule` command.

Measured 2026-10-07, one instrumented run per workload, no averaging. Both programs
execute all three actual production passes; Tiger extraction from a pass feeds
the next pass's newly built egraph. `--eggcc-schedule parallel`, default context
handling (`use_context=false`), production greedy extraction, no ILP.

## Timing, milliseconds

| Program | Pass | Execution incl. setup | Search/binding | RHS | Insert/merge | Rebuild | Extraction wall |
|---|---:|---:|---:|---:|---:|---:|---:|
| fact | 1 | 77.86 | 10.68 | 1.44 | 1.98 | 5.71 | 12.66 |
| fact | 2 | 44.45 | 1.38 | 0.39 | 0.31 | 0.80 | 8.25 |
| fact | 3 | 48.98 | 2.23 | 0.44 | 0.38 | 0.87 | 9.73 |
| loopfact | 1 | 71.75 | 12.25 | 1.35 | 1.32 | 5.10 | 13.37 |
| loopfact | 2 | 66.66 | 10.22 | 1.13 | 1.27 | 6.44 | 12.18 |
| loopfact | 3 | 49.32 | 2.34 | 0.46 | 0.43 | 1.35 | 8.21 |

Execution includes egraph construction, parsing, typechecking, rule installation,
scheduling and execution, but excludes building the source program. It is not
pure inference time. The unlisted residual (roughly 42–58 ms per pass) includes
setup, query compilation and other execution bookkeeping.

The kernel's existing "search" timer actually includes RHS. The probe measures
RHS instruction intervals at all three action-dispatch sites, only during the
main scheduled rule execution, excluding merge and rebuild. Search/binding is
existing search-and-apply wall time minus those RHS intervals. It is not a pure
hash-probe timer. RHS includes primitive computation, constructor lookup and
staging writes. Insert/merge is the existing table-commit phase; insertion and
merge are not independently timed. Rebuild uses the existing outer rebuild timer.
The production engine is configured single-threaded, so subtracting intervals
here does not mix parallel CPU time with wall time.

Extraction wall surrounds the production extractor call. Its own reported inner
extraction times are also in results.json (fact: 10.39/6.65/7.60 ms; loopfact:
11.19/10.20/6.65 ms). Serialization is separate (2.16–3.81 ms). Native parse/setup,
serialization and extraction must not be charged to pure Join work.

These are tiny fact/loopfact inputs, not a representative whole eggcc suite.
Counters introduce overhead; no speedup is claimed. A preliminary uninstrumented
fact run was used as a functional smoke check; its output matches the profiled
run, but its verbose-log timing is not used in this table.

## Coverage, separately for each pass

**This is the existing frozen fact Pass-1 observation library (163 entries), not
newly baked and certified libraries for each pass.** fact is in-training;
loopfact is held-out. Passes 2/3 measure transfer of that library. No library is
extended during evaluation, and no cross-pass denominator is presented as one
egraph. Entries include duplicates and are not 163 distinct certified Cell types.

| Program | Pass | Expr e-node | Expr e-class | All constructor rows | Pre-step full LHS in one Cell |
|---|---:|---:|---:|---:|---:|
| fact | 1 | 78.52% | 73.88% | 32.71% | 9.02% |
| fact | 2 | 95.10% | 90.24% | 32.21% | 6.91% |
| fact | 3 | 96.39% | 96.67% | 36.29% | 8.71% |
| loopfact | 1 | 61.04% | 55.41% | 27.26% | 5.50% |
| loopfact | 2 | 64.72% | 58.46% | 29.80% | 7.69% |
| loopfact | 3 | 87.80% | 87.76% | 28.36% | 5.60% |

Coverage runs separately on egg_layout's native diagnostic using sources exported
from the actual production pass chain. All six traced-control final-state checks
pass, and each retains 7,402 frozen signatures. This is not a proof of identical
whole states across the production and diagnostic engine versions. Coverage
means positive structural support, not certified runtime closure or Join savings.
Source macro expansion/internal-name normalization also means old signature rates
need not be byte-for-byte identical to earlier exports. Exact numerators,
denominators and source/library hashes are retained in results.json.

## Reproduce locally

```sh
python3 experiments/eggcc_passes/build_profile.py
LIBRARY_PATH=/opt/homebrew/opt/cbc/lib CARGO_TARGET_DIR="$PWD/../check_statewalk/target" cargo build --offline --release --manifest-path tools/eggcc_term_validate/Cargo.toml --bin flatten
cargo build --offline --release --example ripsat_coverage
python3 experiments/eggcc_passes/run.py --output out/new-pass-audit --library out/ripsat-coverage-remeasure-merged/train/library.json
```

The build helper temporarily instruments the existing production checkout and
cached ebba7bb dependency, copies the benchmark executable into out/, and restores
source files and the original executable. instrumentation.patch records the
changes. It invalidates only the two affected production dependency build caches.
LLVM 18, CBC and zstd paths are local macOS prerequisites. No network is used.
The frozen library is an existing experiment artifact; its creation is documented
in experiments/ripsat_coverage. Large source dumps, libraries and binaries stay
in out/; do not regenerate the library from held-out runs.
