# math mini3：时间与内存诊断

2026-09-23，基于 `4037735`，另加 SVG 生成开关用于对照。`source.egg` 仅把原始 `egglog/tests/math-microbenchmark.egg` 的 `(run 11)` 改为 `(run 3)`，所有规则与初始表达式保持原样。

## 单次测量结果

| 模式 | 用时 | 峰值 RSS | 结果 |
|---|---:|---:|---|
| 原生，无 trace | 原生执行 **5.39 ms** | **8.17 MiB** | 3 轮完成，208 条 constructor 行 |
| 默认分析，自动 SVG，无 ripen 上限 | ≥10 s | 采样 278 MiB | 外部时间保护停止 |
| 50 ms / 200K，自动 SVG | ≥30 s | 采样 538 MiB | 外部时间保护停止 |
| 50 ms / 200K，不自动 SVG | **6.62 s** | **1.55 GiB** | 分析结束；79 Saturated、6 ObservedUnfinished |
| 无 ripen 上限，不自动 SVG | ≥10 s | 采样 **3.55 GiB** | 外部时间保护停止 |

每配置只运行一次。默认分析、带上限且自动 SVG 另有初始 10 秒观察记录，全部原始数字见 `result.json`。原生执行时间来自进程内计时；分析时间为外部总时间，不能直接拿它们计算精确加速比。完成进程的峰值用 `/usr/bin/time -l`，被终止进程使用约 0.1 秒间隔的进程组 RSS 采样，因此是下界。

所有分析测试有外部 **10/30 秒、4 GiB RSS** 保护（区别于每个 ripen 的 50 ms / 200K）。没有复现到 25 GB；没有为了追到该峰值继续占用机器。

## 已确认的原因

1. **三轮只限制 tier0，不限制后续 ripen。** 当前默认仍没有时间/size 上限，50 ms 和 200K 必须显式配置。默认无 SVG 的测试在 10 秒窗口内已达到 3.55 GiB，仍未完成。
2. **自动 SVG 阻塞热路径。** 栈采样发现原生 `to_svg_file`，包括 `snapshot_unfinished → export_state → to_svg_file`。暂停时本来应只保存 unfinished 状态，实际还在等待 Graphviz。这部分不受 ripen 的时间/size 阈值保护。关闭自动 SVG 后，同一个带上限 mini3 能完成。
3. **关闭 SVG 后，Rust 分析层仍占用很大。** 1.55 GiB 的峰值属于 `egg_layout`，不是浏览器或 dot。mini3 只有 208 条 tier0 constructor 行，却派生了 85 个 ripen 任务；未完成 cell 中最大的观察图有 13,678 条事实。
4. **全量 binding 和多份报告也是实际开销。** 完成运行保留 626 条 weak 关系、50,552 个 binding tuple。队列 JSON 约 13.7 MiB，其中 weak 证据序列化约 7.37 MiB，桶触发匹配约 4.98 MiB；相近内容又写入多个 round 与 analysis。采样同时出现 native Join 与序列化/克隆路径。不过这些数字不能精确解释全部 1.55 GiB，还没有按分配来源做完整内存归因。

关闭 SVG、配置上限后的阶段累计：准备约 0.44 s，推进 ripen/共享约 2.28 s，精确目录约 0.12 s，weak 查询与证据约 1.85 s；剩余时间包含报告与文件操作。不要再将所有耗时归因于 coarse facts 构造。

## 可重复的受保护测试

```sh
cargo build --release --offline --bin egg_layout --example native_math_baseline
python3 experiments/math_mini3/bench.py --output out/mini3-new
# 只测配置 50ms/200K 且不自动生成 SVG
python3 experiments/math_mini3/bench.py --output out/mini3-nosvg-new \
  --bounded-only --no-svg
```

直接运行（不含外部整进程保护）：

```sh
EGG_LAYOUT_EAGER_SVG=0 cargo run --release --offline -- analyze \
  --source experiments/math_mini3/source.egg --output out/mini3-analysis \
  --ripen-observe-ms 50 --ripen-observe-facts 200000
```

`EGG_LAYOUT_EAGER_SVG=0` 仅关闭自动 SVG，保留原生 DOT、JSON 和分析；页面已有从 DOT 按需渲染的路径。未设置时仍维持旧的自动 SVG 行为。本次没有偷偷修改默认时间/空间上限，也没有删减规则或推理结果。

原始日志、采样栈和结果保存在 `out/math-mini3-*`。报告是快速诊断，不是统计显著性测试，也不是“25 GB 已经解决”的声明。

## 用户指定进程的现场确认

随后检查 Python PID 10975 的子进程 `egg_layout` PID 63343，`sample` 明确报告 **physical footprint 25.4G**。这与前面受保护 mini3 未达到该峰值是不同的观察。

现场输入实际仍是 `(run 11)`，最后打印完成的 tier0 进度为第 3 轮。该次队列配置是时间/事实数均无上限、每 sweep 最多 4 轮。日志连续记录 2,334,405、8,589,833、33,250,992 条 trace events，以及最后 8,538,587 个 producer 记录。主线程采样全部落在 `Session::advance_with_observation → collect → BTreeMap::retain/ExtractIf`，此时不是 SVG 渲染。

因此已直接确认用户报告的高内存现场：大量 trace 批次和持续保留的 producer/历史正在处理。`drain_completed` 只是把事件从缓冲移交给 collector，不等于这些数据及其衍生历史已经释放。要进一步分摊每一种分配仍需专门内存分析。

只对该子进程发送 SIGSTOP 保留现场，未终止 Python；暂停不释放内存。现场摘要见 `live_process.json`，栈保存在 `/tmp/egg-layout-63343.sample.txt`。PID 状态是当时记录，不保证日后仍有效。

## Native admission protection and compact presentation trace

One actual egglog runtime run (`native_admission.json`), using the **defaults** with no explicit ripen flags and SVG disabled: 6.19 s elapsed, 1,527,922,688 bytes peak process RSS (~1.42 GiB), exit 0. Command: `python3 experiments/math_mini3/bench.py --output out/native-admission-mini3 --default-only --no-svg --seconds 15`. The recorded commit is the parent; measurement included the working-tree patch described in the result.

Default ripen limits are now 50 ms / 200,000 facts. Native join/admission checks stop before additional match trace allocation; admitted effects and rebuild finish. Pending write reservations may stop before actual facts reach the limit. This is not a total-process memory cap. Default presentation omits per-application DOT/Typst stream events; analytical provenance is retained. The remaining ~1.42 GiB is still substantial, and this experiment does not establish a major memory reduction.

## Graphless optimization (actual runtime)

Reproduce the baseline comparison with `python3 experiments/math_mini3/bench.py --output out/graphless-comparison --no-graphs --compare-native --seconds 15`. This does not generate DOT/SVG. `graphless_optimization.json` records one run per code revision, with matching source and observation defaults. Final `/usr/bin/time` wall time: **5.12 s**, compared with **6.33 s** before the changes; native process wall time **0.18 s**, native internal timer **0.005185 s**. Do not compare the internal timer directly as an end-to-end speedup. Compilation is excluded.

Changes: typed scalar export avoids whole-graph serialization, identical typed weak queries share compiled plans within the same native engine, report inspection borrows instead of cloning, and progress/final reports avoid redundant copies. Final phase totals: registration 0.400 s; advance/share 1.498 s; exact catalog 0.136 s; weak queries/evidence 1.790 s. There are still 2,001 compiled query pairs and 2,722 current scans. This is not near native execution speed. Peak RSS was 1,400,029,184 bytes, higher than the pre-change 1,269,071,872 bytes; no memory improvement claim.

All 79 completed state JSONs are exactly equal before/after; six unfinished prefixes remain time-dependent. Time-bound work can grow farther after an optimization; unfinished evidence is not a fixed-work benchmark. Tests cover identical-query sharing, preservation of literal distinctions, explicit payload erasure, and existing CS/ripen regressions.

## Query normalization and epoch reuse

`query_reuse_optimization.json` records the next bounded experiments. Default final configuration: **4.55 s** process wall time, **1,354,629,120 bytes** peak RSS, versus the preceding revision's **5.12 s** and 1,400,029,184 bytes. These are single measurements, not confidence intervals. All 79 completed states match after excluding run-local IDs (78 are byte-content equivalent as parsed JSON); six time-limited states remain incomplete.

Queries normalize vertex numbering by first occurrence, omit unused boundary vertices, and cache results within one ripen execution epoch. A native step reporting an update invalidates the cache; unchanged saturation checks reuse it. Facts limits read backend table sizes from cached names instead of reparsing declarations and scanning all rows. Graphless CLI analysis skips display snapshots, but preserves final analysis, catalog, and queue JSON; the web stream retains live statistics.

The ablation `EGG_LAYOUT_RIPEN_HISTORY=0` omits the isolated cell's diagnostic layer history (tier0 analysis and committed effect observations remain). Finite-state/weak-dominate regression agrees with full history, but the timed mini3 becomes slower because unfinished states advance farther and produce more query results. Therefore **full local history remains the default**. A one-worker ablation also failed to establish a meaningful end-to-end win and is not made the default.

The final refinement invalidates query results on actual native updates, not on empty saturation checks: 2,178 current scans and 544 result-cache hits, versus 2,722 scans before this round of work. Final measured wall time **4.77 s**, RSS **1,545,388,032 bytes**. The 4.55 s preceding sample should not be cherry-picked as the final result; fixed wall-clock limits change unfinished workloads and single-run variation is material. Completed states remain structurally equal (excluding run-local IDs).

## Near-second native analysis

`near_second.json` records complete native analysis with DOT/SVG disabled, 50 ms / 200K facts, four workers, and ThinLTO. Three process-wall measurements were **1.33, 1.06, 1.07 s** (first launch is kept, not discarded); peak RSS was 585–624 MiB. **The requested sub-second end-to-end target is not yet consistently met.** Compilation and sampling are excluded. Profiling is now opt-in in `bench.py` via `--profile` and `--sample`.

All 79 completed states have the same structure as the pre-change baseline, and all 488 weak-dominate decisions sourced from completed states agree. Six unfinished prefixes remain wall-clock dependent. Kernel tests cover independent clone notifications, parallel clone execution, exact existential/difference queries against complete enumeration, union, subsume, and effect-summary equivalence. DABCABC and branching history-mode regressions remain enabled.

Local ripen history now defaults to committed-effect summaries. `EGG_LAYOUT_RIPEN_HISTORY=1` restores full local binding/layer diagnostics. `EGG_LAYOUT_FULL_TRACE=1` separately restores exhaustive weak-query binding lists; default lists are explicitly marked as existence/freshness witnesses. The proposed local provenance template feedback to tier0 is **not implemented**.

Discarded experiments: alternate MinCover/PureSize planners triggered kernel assertions; vectorized effect actions changed fixed-budget branching histories; neither is retained. Parallel mutation initially exposed the shared database notification queue and was retained only after fixing clone isolation. Full LTO did not outperform ThinLTO here and was not retained. Larger thread counts and sweep sizes did not establish a stable win and did not replace defaults.

## Native virtual facts comparison (2026-09-27)

Measured the actual patched egglog runtime, using the same binary and the same
`source.egg` (three rounds). This is not a standalone storage simulation. Native
means no tracing, Bake library or virtualization; it is not a build of the pristine
`egglog-baseline` tag. Measurements were taken on dirty HEAD `19de2fd`; source and
binary hashes, individual samples and commands are in `virtual_facts.json`.

| Mode | Worker time, median | Peak RSS, median |
|---|---:|---:|
| Native egglog | 3.591 ms | 7.281 MiB |
| Library matching only | 10.337 ms | 10.188 MiB |
| Library matching + virtual storage | 10.170 ms | 9.891 MiB |

Three independent processes per mode, rotated execution order. Worker time includes
source parsing, library loading, matching/packing, execution, report construction,
and engine/library destruction; it excludes process startup and final stdout
serialization. macOS `/usr/bin/time -l` supplies process peak RSS; its wall clock
resolution is too coarse for these runs. Compilation and separate exact verification
are excluded. This small sample does not establish a significant difference between
matching-only and virtual storage.

The library is built offline from round 1 only (343,263 bytes). Earlier preparation
measurements were 0.62 s for capture and 0.01 s for library construction, excluded
from runtime. Loading the library is included: virtual median 6.208 ms. Matching
and packing together cost 0.382 ms. After round 1, only 25 unique existing native
rows are virtualized: Add 13, Mul 10, Pow 1, Const 1, using three templates/binding
matrices. Rounds 2–3 run normally; new rows remain in the dense overlay, without
continuous repacking. Global native hash/provenance indexes are retained.

Exact final graph comparison in separate processes passes for both matching-only
and virtual modes: 208 rows and 124 typed values, including literals (not 124
eclasses). The check compares facts, equality and visibility. Native virtual-fact
regressions also cover union/rebuild, function merge, subsume/delete, producer
certificates, cached joins, clone isolation and materialization.

**No speed or memory benefit on mini3:** virtual worker time is approximately
2.83 times native and peak RSS is 35.8% higher. Library loading dominates runtime;
25 packed rows do not amortize library, template and index overhead. Row-payload
accounting is not a substitute for process RSS.

Separately, the full analysis pipeline (history, CS, ripen, equivalence and weak
dominate; graph output disabled) took 1.54 s and 639.41 MiB peak RSS in one run.
It performs additional work and is not a like-for-like native execution comparison.
Time-limited unfinished ripen work can vary between runs.

Reproduce (new output directory required):

```sh
cargo build --release --offline --bin egg_layout --example virtual_math_bench
python3 tools/bench/math_virtual.py --output out/math-virtual-new
```

`--library path/to/library.json` reuses an existing library; `--runs 3` controls
repetitions and `--skip-analysis` omits the separate analysis measurement. External
30-second/4-GiB process guards protect the experiment, not the inference algorithm.

### Expanded graph and library accounting

Additional untimed verification is saved in `virtual_facts_structure.json`.
Native, matching-only and virtual storage all have **208 unique logical Math
e-nodes, 116 canonical Math e-classes, 1.7931 e-nodes/class**; all 208 are visible.
These counts scan `function_for_each`, decode virtual storage recipes and deduplicate
canonical constructor rows. E-classes count canonical Math result IDs; primitive
values are excluded. This is the finite e-graph, not recursive tree unfolding
(which could be infinite). No template-size multiplication or instance double count.

The library is **343,263 bytes (335.22 KiB)**: eight shared bodies (seven full,
one delta), thirteen enclosures and nineteen entries. Compact-JSON section sizes
are bodies 31,838 B, enclosures 10,365 B and entries 192,435 B; these exclude original
pretty-print whitespace and are not heap sizes. Entry provenance/recipes account
for much of the serialization. The benchmark's three physical row templates are
not the same count as the library's eight semantic bodies. Library retained heap
has not been isolated; peak process RSS includes it, but cannot attribute it exactly.

`external_overlay_joins_virtual_body_and_writes_new_native_facts` packs A/B rows,
then inserts an External relation and executes a Join across two virtual atoms and
that external fact. The expected Out row appears, the unmatched instance does not,
and virtual rows remain virtual. The existing displaced-boundary regression verifies
union enables a previously unavailable Join. All twelve virtual fact tests pass.
These are concrete boundary correctness checks, not proof of every possible rule.

## Full math microbenchmark growth curves (11 rounds)

`growth11.json` and `growth11.csv` record a single native and virtual run of the
original `egglog/tests/math-microbenchmark.egg` with its `(run 11)`. The standalone
HTML has three plots against cumulative native match count: resident memory,
expanded unique Math e-nodes, and canonical Math e-classes, plus per-round data,
linear/logarithmic x-axis and point tooltips. No complete match history is captured.

```sh
cargo build --release --offline --example math_growth
python3 tools/bench/math_growth.py \
  --library out/math-virtual-mini3-20260927/library.json --output out/math-growth-new
# Open out/math-growth-new/index.html
```

To create a fresh library first, use the preceding `math_virtual.py` command;
pass the resulting `library.json`. `--source` accepts a mini3 source instead.
This driver supports simple `(run N)` schedules and the Math datatype workload.

Round 11: 944,254 cumulative matches, 1,047,896 expanded e-nodes and 443,832
e-classes. Every round's counts agree across native/virtual modes. RSS at the final
boundary is 482.625 MiB native and 493.891 MiB virtual. The library remains
343,263 bytes and only 25 existing rows were virtualized after round 1. This does
not measure continuous template discovery/repacking or demonstrate a compression
benefit at later rounds.

RSS is read with `ps` immediately before a diagnostic row scan, not peak RSS.
Earlier scans and allocator retention can affect later samples; this is an
instrumented growth experiment, not the uninstrumented speed benchmark. Native
match totals are not committed write counts or distinct bindings across all rounds.
Points are sampled at round boundaries; connecting lines are interpolation only.
Native tables after rebuild already deduplicate row keys; virtual rows are decoded
through the same iterator. Math result IDs are canonicalized, excluding primitive
values. Counts are finite graph sizes, not recursive tree unfolding. Counts agreeing
is weaker than exact graph equality (mini3 has a separate exact verification).

The generated HTML passed JavaScript syntax checking; browser visual verification
was blocked by the browser tool's local-file URL policy.

## Incremental native RipSat snapshot primitive

`reuse_graph::ripsat_runtime::RipSatRuntime` retains compiled native engines and
semi-naive frontiers. A graft forks shared immutable storage, applies native
actions, and saturates only the incremental work. A repeated exact `(state ID,
normalized action text)` transition returns the existing state without invoking
rules. Default-ruleset sources only; changing rules/declarations via graft is
rejected. Existing 50 ms / 200K observation limits apply and unfinished children
are not admitted. This API does not infer parametric binding correspondence.

`ripsat_incremental.json` is a **local microbenchmark**, taking the first completed
Add composition from the round-1 math library and grafting an Add with a new Var:
0.406 ms incremental graft (2 native matches), 0.017 ms cached transition (0
matches), 3.115 ms fresh compilation and saturation. Single samples, not a full
math-microbenchmark result or confidence interval. The commuted result check
passes. Two regressions additionally cover external relation activation, union
activation and parent snapshot isolation.

```sh
cargo run --release --offline --example ripsat_incremental -- \
  out/math-virtual-mini3-20260927/library.json out/ripsat-incremental.json
```

The adjacent `.recipes.json` is a portable replay recipe, not an executable
`bake_lib_round2` and not serialized native query cursors. Restoring it requires
rebaking. Automatic tier0 placement, binding transport, dynamic library admission
and end-to-end internal Join suppression are **not connected** by this primitive.

## Executable RipSat runtime and refreshed benchmark

See [adaptive_runtime/README.md](adaptive_runtime/README.md) for the actual tier0
macro/virtual-storage loop, round-2 executable library, positive-rewrite correctness
boundary and four-mode benchmark. The full 11-round adaptive modes hit the external
memory guard; this is not a successful acceleration result. The complete raw data
and readable library are committed there.
