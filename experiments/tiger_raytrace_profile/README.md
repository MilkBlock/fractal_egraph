# raytrace 上 Tiger/StateWalk extraction 的分段 profile

对 `check_statewalk@ae80f82a` 的 Tiger C++ extractor 在
`benchmarks/passing/raytrace/raytrace.rs` 上做端到端分段测量，回答"哪个段最耗时"。
范围是**真实 eggcc release 流水线生成的 e-graph** 和**生产编译参数的 Tiger 二进制**，
不是合成输入，也不是 standalone 原型。

端到端 wall/提取占比见 [statewalk 到局部 extraction 的迁移边界](../statewalk_local_extraction/README.md)：
extraction 约 7.9 s / 53.7 s wall（≈15%）。本报告只拆 Tiger 内部。

## 结论（先看这个）

Tiger 一次 pass 的开销里，**最耗时的是 `read_nodes()`——把序列化 e-graph 逐 token 读进来**，
三 pass 合计 **3.26 s / 6.48 s ≈ 50%**；其次是
**`compute_statewalk_cost()` 的全局 greedy 代价预算 + 每个 effectful enode 的代价累加**，
**1.74 s ≈ 27%**。真正精巧的 StateWalk DP（`statewalkDP`，含 Zobrist/liveness/satellite）
只有 **0.075 s ≈ 1.2%**——**它不是瓶颈**。

单函数 self-time（dump 3，`sample` 1 ms，共 2593 样本）：

| 排名 | 函数 | self 样本 | 占比 | 出处 |
|---:|---|---:|---:|---|
| 1 | `__svfscanf_l` | 300 | 11.6% | `read_string()` 的 `scanf("%s")` |
| 2 | `tiny_free_no_lock` | 208 | 8.0% | 逐 token `std::string` 的释放 |
| 3 | `__do_rehash`（`unordered_map<string,int>`） | 175 | 6.7% | `raw_eclassidmp` 重哈希 |
| 4 | `free_tiny` | 152 | 5.9% | 同上 |
| 5 | `tiny_malloc_from_free_list` | 119 | 4.6% | token/节点分配 |
| 6 | `__emplace`（`unordered_map<string,?>`) | 105 | 4.0% | token 表插入 |
| 7 | `_platform_memmove` | 78 | 3.0% | token 字符串拷贝 |
| 8 | `read_next_token()` | 70 | 2.7% | tokenizer 本体 |
| … | `statewalkDP` | 13 | 0.5% | DP 本体 |

分配器（`tiny_free_*` / `tiny_malloc_*` / `free_tiny` / `_szone_free` …）合计约 600+ 样本 ≈ 23%，
其中绝大多数由 `read_nodes` 的逐 token `std::string` 与 `RawENode`/`raw_egraph` 构造驱动。

## 分段数据

输入：release eggcc 在 raytrace 上按 schedule 逐步把整个 e-graph 序列化后交给 Tiger，
`--egraph-out-dir` dump 出 3 个真实输入。

| dump | JSON 大小 | 节点数（`"op"` 计数） | tokens（词数） | Tiger warm wall |
|---:|---:|---:|---:|---:|
| 1 | 72.8 MB | 247,299 | — | 0.91 s |
| 2 | 189.1 MB | 653,093 | — | 2.86 s |
| 3 | 212.4 MB | 716,893 | 14,268,873 | 2.71 s |

三 pass 分段合计（warm，`steady_clock` 相位计时）：

| 段 | 秒 | 占 Tiger wall |
|---|---:|---:|
| **`parse_egglog_json`（总计）** | **3.560** | **54.9%** |
| └ `read_nodes()`（tokenizer） | **3.257** | **50.3%** |
| └ 其余解析（build_simple_egraph / mark_reachable / prune_top / propagate / mark_effectful / find_roots） | 0.28 | 4.3% |
| **`extract_all_fun_roots`（总计）** | **2.320** | **35.8%** |
| └ **`compute_statewalk_cost()`** | **1.742** | **26.9%** |
| └ 区域构造 `construct_regionalized_egraph` | 0.092 | 1.4% |
| └ 区域提取 `region_tiger_compute` | 0.563 | 8.7% |
| &nbsp;&nbsp;└ **`statewalkDP`（DP 本体）** | **0.075** | **1.2%** |
| &nbsp;&nbsp;└ `statewalk_greedy_extraction` | 0.289 | 4.5% |
| `output_egglog` | 0.009 | 0.1% |
| 未计入（进程启动 + 退出时释放整图） | ~0.59 | ~9.1% |
| **合计（warm）** | **6.48** | 100% |

`sample` 的 inclusive 样本（dump 2 / dump 3，线程总计 2660 / 2593）给出同样结论：

| 函数 | dump 2 | dump 3 |
|---|---:|---:|
| `parse_egglog_json` | 1083 (40.7%) | 1333 (51.4%) |
| └ `read_nodes` | 1003 (37.7%) | 1252 (48.3%) |
| `extract_all_fun_roots_tiger` | 802 (30.2%) | 671 (25.9%) |
| └ `compute_statewalk_cost` | 800 (30.1%) | 670 (25.8%) |
| &nbsp;&nbsp;└ `greedy_extract_compute_eclasses_pick` | 186 (7.0%) | 158 (6.1%) |
| `extract_regionalized_egraph_tiger`（含 DP） | 9 (0.3%) | 21 (0.8%) |
| `output_egglog` | 3 | 4 |

## 两个热点各自慢在哪

### 1. `read_nodes()` —— 50%

`json2egraphin.cpp:66-73` 的 tokenizer：

```cpp
inline string read_string() {
    static char buf[505];
    scanf("%s", buf);     // ← self-time 第一名
    return buf;           // ← 每次一个 std::string 堆分配
}
```

dump 3 有 ~1.43e7 个 token，即 ~1430 万次 `scanf` + ~1430 万次 `std::string` 构造，
token 还先进 `queue<string>`，再被 `read_node()` 拷进 `RawENode`（`string name/op/eclass`
+ `vector<string> ch`），并用 `unordered_map<string,EClassId>` 建 `raw_eclassidmp`。
1.69 s / 14.3M token ≈ **118 ns/token**。

### 2. `compute_statewalk_cost()` —— 27%

`greedy.cpp:231`：

```cpp
vector<Cost> eclass_cost = greedy_extract_estimate_all_eclasses_cost(g);  // ≈6-7%
vector<vector<Cost>> statewalk_cost(g.neclasses());                       // 全图裸 vector
for (i) if (effectful) for (j) statewalk_cost[i][j] = get_statewalk_enode_cost(...);  // ≈19-20%
```

- `greedy_extract_compute_eclasses_pick`（6-7%）是纯图上的 Dijkstra，其 `SCost +=`
  （`greedy.cpp:26-46`）内部是 `unordered_map<int,Cost>`，**每次加法都做哈希节点分配**
  （`operator new` 占该子树样本的 ~2/3）。
- 剩下 ~19% 是"为全图 `g.neclasses()` 个 eclass 各建一个 `vector<Cost>`"（每个 effectful
  eclass 一次 `resize` 分配）加逐 enode 的 `get_statewalk_enode_cost`。
- 注意 `greedy_extract_compute_eclasses_pick` 同时算了 `pick` 和 `dis`，而
  `greedy_extract_estimate_all_eclasses_cost` 只取 `.second`（`dis`），`pick` 是白算。

## 优化方向（按收益排序）

1. **换掉 `scanf` tokenizer**（预期最大单项收益，~50% → 个位数）。
   `mmap`/`fread` 整块读入，手写空白扫描，用 `string_view` 指向缓冲；
   node/eclass 名若为数字则直接解析成整数 ID；预 `reserve` token 表。
   逐 token `std::string` 消失后，顺带消掉 ~23% 的分配器 self-time。
2. **重写 `compute_statewalk_cost`**（~27%）。
   `statewalk_cost` 改扁平数组 + offset（只给 effectful eclass 分配）；
   `SCost` 的哈希 bag 换成排序小向量或复用缓冲；
   `pick` 与 `dis` 拆开，只算需要的那个。
3. **跨 pass 复用**：Tiger 每个 pass 是独立进程，整图被重新序列化 + 重新解析
   （3 pass 共 3.26 s 解析 + eggcc 侧 ~1.87 s 序列化）。若做成常驻进程或增量 delta，
   这一步可再省一大块（属 eggcc 侧架构改动）。
4. DP 本体（1.2%）不值得优化；`statewalk_greedy_extraction`（4.5%）与区域构造（1.4%）
   属次要项。

按 1+2 估算，Tiger 端到端有望从 ~6.5 s 降到 ~2 s 量级；对应 eggcc wall 从 53.7 s 降到
~49 s（extraction 占比从 ~15% 降到 ~4%）。

## 复现

```sh
# 1) 取真实输入（需要 LLVM18 与 zstd/cbc 在库搜索路径）
cd /Users/mineralsteins/Repos/egg_related/check_statewalk   # @ae80f82a
LLVM_SYS_180_PREFIX=/opt/homebrew/opt/llvm@18 \
LIBRARY_PATH=/opt/homebrew/lib:/opt/homebrew/opt/cbc/lib:/opt/homebrew/opt/coinutils/lib \
  cargo build --release --offline
./target/release/eggcc --run-mode optimize \
  --run-data-out /tmp/raytrace-rd.json \
  --egraph-out-dir ../../egg_layout/out/raytrace-egraphs \
  benchmarks/passing/raytrace/raytrace.rs > /dev/null

# 2) 构建带相位计时的 Tiger。patch 路径以仓库根为基准（-p1）：
#    也从仓库根运行，先把 prof.h 放进 tiger 源码目录。
P=.../egg_layout/experiments/tiger_raytrace_profile/instrumented
cp "$P/prof.h" dag_in_context/src/tiger/
patch -p1 < "$P/tiger_phase_timers.patch"
( cd dag_in_context/src/tiger && g++ -std=c++17 -O3 -g -Wno-unused-result \
    egraphin.cpp greedy.cpp json2egraphin.cpp main.cpp regionalize.cpp \
    statewalkdp.cpp tiger.cpp toegglog.cpp debug.cpp ilp.cpp time_ilp.cpp -o /tmp/tiger_instr )

# 3) 相位计数
/tmp/tiger_prof/tiger_instr < out/raytrace-egraphs/tiger_egraph_0003.json > /dev/null

# 4) 函数级采样
/tmp/tiger_prof/tiger_instr < out/raytrace-egraphs/tiger_egraph_0003.json > /dev/null &
PID=$!; sample $PID 60 1 -file /tmp/tiger_sample_3.txt -mayDie; wait $PID
```

`instrumented/` 只包含相位计时所需的 4 个文件 patch 加 `prof.h`；`out/` 下的 dump
（452 MB）与二进制不纳入版本控制。机器可读数字见 `profile.json`。

## 边界

- 测的是 Tiger C++ extractor 进程本身；不含 eggcc 的 `serialized_egraph`（另计 ~1.87 s）。
- `real` 与相位总和之间有 ~9% 残差，主要是进程启动与退出时释放 ~717K 节点图；
  dump 1 的首次运行含 page-cache 冷启动，故表中用 warm 值。
- 采样是 1 ms 统计采样，单函数小样本（<5）被 `sample` 折叠，故 `statewalkDP` 的
  self 样本（13）小于其相位计时（0.046 s / 1.7%），两者不矛盾。
