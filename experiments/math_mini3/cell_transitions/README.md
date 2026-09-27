# RipSatCell：可执行的参数化转移，第一条闭环

新增共享类型 `RipSatCell`。`Macro` 仅保留为 Rust 兼容别名；v2 Bake 文件的
字段和格式保持可读，不要求重建已有库。

这一版实现的是**共享 cell 配方 + 持久化内部接口视图 + 原生增量外部查询**。
没有根据观察频率猜测正确性，也没有把具体 binding 或 Inserted/Updated
差异枚举成新的状态。功能显式启用，默认运行不变。

## 具体执行过程

源 cell 包含 `A(x) = N0(N1(N2(N3(x))))`，消费规则是：

```egg
(rewrite (Pair (N0 (N1 (N2 (N3 x)))) (Flag y)) (Done x y))
```

载入 Bake 库时，编译器从 cell 的符号事实索引中证明内部片段确实存在，
得到 source slot 到接口参数/root 的映射。源 cell 的 RHS 直接附带：

```egg
(set (adaptive_cell_view_0 x) adaptive_root)
(adaptive_cell_cover_0 x)
```

消费规则随后查询：

```egg
(rule ((= adaptive_root (Pair adaptive_cell_cut_0 (Flag y)))
       (= adaptive_cell_cut_0 (adaptive_cell_view_0 x)))
      (... 原有的目标 RipSatCell 构造配方 ...))
```

内部四个构造器原子变成一个接口原子；外部 Pair、Flag 仍由 native Join
验证。目标配方继续使用原生构造器、hashcons 和 union，实际落到旧端口还是
其他 canonical class 不会创建另一种转移。

完整、可解析的生成代码见 `nested-generated.egg`；它是规则库，没有自带
输入或执行调度。`nested-proof.json` 保存符号槽位映射及检查条件。

## 不是只处理已有 cell

未被覆盖的内部片段由单独的原生 builder 查询产生同一个接口视图。
它在每个原始规则执行步之前运行，仅增加元数据，不执行额外 Math rewrite。
因此当外部构造器或 union 使一个新片段成立时，原生补全仍然能够发现它。

视图可以由完整 RipSatCell 直接提供，也可以由原生补全提供。
**视图存在只证明被投影的内部片段存在，不能反过来假定整个源 cell 存在。**

## 为什么能在 Join 前跳过 builder

第一版要求指定一个唯一的、具有非空键且键参数为变量/字面量的构造器锚点，且它的键
包含内部片段的全部变量。对于例子，这个锚点是 `N3(x)`。

给定这个键：

1. 所有自由变量已经确定。
2. 正构造器的函数性使整个内部表达式的结果唯一（模 native union）。
3. 如果视图已经保存了该结果，builder 再匹配同一键只能重复 set/覆盖标记。
4. 核心 Join 在任何 join stage 前，从当前 semi-naive 子集中去掉这些已覆盖键。

这里不是屏蔽整个 root eclass；另一组输入产生的新匹配不会被一起隐藏。
覆盖关系由 native 表保存，union 后由原生 canonicalization/rebuild 更新。
与它一起维护的视图是 `function ... Math :merge old`：键决定结果，保留函数依赖
供 planner 使用。不同旧键被 union 合并时，原构造器事实仍在，由 congruence
使其结果相等；优化器没有删掉这些事实。

证明依赖当前正构造器运行时：无 delete、subsume、可变函数读取或额外副作用。
非法/非功能锚点配置会报错，不做启发式降级。当前每个 consumer 只接受一个
显式 cut；不会自动挑选观察目录里的 90 个连接或合并任意 coarse layer。

静态编译不做递归子图搜索：先索引 cell 事实，再枚举锚点行、绑定变量并
向前求值。设 cell 有 N 行、A 个锚点行、片段 AST 有 P 个节点，忽略固定
arity 后，BTreeMap 实现约为 O(N log N + A P log N)，输出绑定另计。
运行时仍有残余查询成本，不声称任意规则都可变为常数时间。

## 实测

实际 egglog 运行时，固定同一份库与源码，单线程；无 logical tracing。
64 个源实例，每个接入 32 个外部上下文。正常和优化版本都产生 2,112 次主规则
匹配。验证进程与计时分开；最终 Math 图逐构造器、逐等价类精确对应。

原生读计数（独立 profile 构建，包含新接口/覆盖表的成本）：

| 样例 | 总扫描行访问：正常 → 优化 | 键查找：正常 → 优化 |
|---|---:|---:|
| 单原子片段 | 8,384 → 8,448 | 14,528 → 14,656 |
| 四层内部片段 | **14,720 → 8,448** | 20,864 → 20,992 |
| math6 的单 Add 切法 | 27,959 → 30,054 | 10,429 → 13,021 |

四层内部片段总扫描减少 **42.6%**。其中 N0–N3 的扫描从 8,448 降到 64，
新增视图承担 2,112 次扫描；源 cell 已提供全部接口，因此 builder 完整匹配为 0。
RHS 的构造器查找基本没少，覆盖表另增加 128 次查找。

这些是扫描/探测工作量，**不是抽象 Join 算子次数**。索引构造和被过滤行
也计入扫描；某些底层索引探测并非逐行扫描，不能由这个计数推算全部 CPU 工作。

最终七次交替执行的中位数（无读计数 instrumentation）：

| 样例 | 执行阶段 ms：正常 → 优化 | 总驱动 ms：正常 → 优化 |
|---|---:|---:|
| 单原子片段 | 2.08 → 2.00 | 63.20 → 60.52 |
| 四层内部片段 | **4.36 → 2.21** | 66.43 → 64.60 |
| math6 的单 Add 切法 | 8.42 → 8.73 | 42.01 → 43.07 |

执行阶段包含 builder 和覆盖检查；总驱动还包含解析、规则/视图准备、
packing、逐轮统计及库输出。四层样例执行阶段约减半，完整驱动仅约改善 2.7%。
前一组七次对照的整体改善约 1.5%；不要把阶段收益说成整体翻倍。
单原子样例整体耗时在不同批次中有正有负，且读取工作增加，不能认定有结构收益。

**math6 这条单原子切法是负结果**：还有 474 次额外 refine 行访问、606 次
原生补全匹配。当前不默认启用，更不宣称整个 math microbenchmark 已加速。

等价图规模：单原子 4,352/2,240，四层 4,544/2,432，math6 2,167/748
（e-node/e-class）。优化器自有的 custom function 不是 Math 构造器，验证时
排除这些元数据，但实际进程 RSS、读计数和执行时间均包含它们。
库大小分别为 1,833、3,117、22,017 bytes；生成代码/计划大小、接口行数和
每次进程峰值内存均在报告中。四层样例维护 64 条视图和 64 条覆盖记录。
RSS 存在波动，没有从这组实验推导内存压缩收益。

`pilot.json`、`timing.json` 保留较早对照；最终结果是 `timing_final.json`、
`profile_final.json`。本实验对照的是同一实际 runtime 的原查询与残余查询，
不是 pristine egglog，也不是包括 ripen 学习阶段的全项目速度比较。

## 复现

```sh
cargo test --offline --test reuse_cell_transitions
cargo build --release --offline --example adaptive_math
RAYON_NUM_THREADS=1 python3 tools/bench/cell_transitions.py \
  --output out/cell-transition-timing --runs 7

# 计数单独测，避免把 profiler 成本混入上面的计时
cargo build --release --offline --features native-read-profile --example adaptive_math
RAYON_NUM_THREADS=1 python3 tools/bench/cell_transitions.py \
  --output out/cell-transition-reads --runs 1 --profile
```

单独使用已编译的切分配置：

```sh
EGG_LAYOUT_CELL_TRANSITIONS=experiments/math_mini3/cell_transitions/nested-plan.json \
 cargo run --release --offline --example adaptive_math -- \
 frozen SOURCE.egg out/my-transitions LIBRARY.json
```

也可用相同环境变量调用 CLI 的 `reuse-adaptive frozen`。库的规则环境必须
和源码一致；当前不同时启用旧 RHS/Join 剪枝或演化 tracing。

七项新回归覆盖：直接 cell 投影、无 cell 的补全、union 启用新内部选择、
输入别名导致元数据 rekey、非法锚点拒绝、生成 `.egg` 解析、实际触发 native
并行路径的混合覆盖测试、元数据与真构造器区分、literal 锚点/零参数视图。
