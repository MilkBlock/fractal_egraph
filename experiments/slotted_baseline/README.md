# Slotted e-graph 基线

接入真实的 `slotted-egraphs = 0.0.36`，固定 Cargo 版本及依赖校验和。发布包记录的 Git revision 为 `ddda46116edfb3170d8b9caca08527deebc4854f`，来源为 [slotted-egraphs 上游](https://github.com/memoryleak47/slotted-egraphs/)。

对照是仓库内的 egglog，源自 `ebba7bb902bdc1b0f377b6bb22c06ac305912674`，运行时不启用 trace。两个引擎分别构建自己的真实图并执行规则，不经过 SharedStore，也不重放快照。

## 工作负载

- `renamed`：`Add(Mul(a,b),Neg(a))`。每个输入使用不同的自由变量名，保留两个分支共享 `a` 的连接关系。测试规模为 1、64、1,024 个输入。
- `constants`：同样的嵌套结构，但叶子是互不相同的数值常量。它们不能作为 slots 重命名，是负对照。
- `ac`：从 `(a+b)+(c+d)` 出发，执行加法交换律、结合律及反向结合律直到饱和。测试规模为 1、16、128 个输入。

`renamed` 与 `constants` 测量的是构建，不执行 rewrite；`ac` 测量完整构建和 rewrite 饱和。每个输入直接生成后插入当前引擎，没有放大重放已有图快照。

## 核心结果

优化构建、3 次独立进程测量，中位数。分配量计入整个引擎及所有保留的外部 root 句柄；slotted 的 root SlotMap 也包含在内。

| 数据 | 引擎 | 构造节点 | 活跃 e-class | 保留分配量 |
|---|---|---:|---:|---:|
| 1,024 个变量重命名实例 | egglog | 5,120 | 5,120 | 1,412.35 KiB |
| 同上 | slotted | **4** | **4** | **134.80 KiB** |
| 1,024 个不同常量实例 | egglog | 5,120 | 5,120 | **1,412.35 KiB** |
| 同上 | slotted | 5,120 | 5,120 | 18,811.07 KiB |
| 128 个四变量 AC 实例，饱和后 | egglog | 6,912 | 1,920 | 3,108.00 KiB |
| 同上 | slotted | **7** | **4** | **56.10 KiB** |

最后一组 rewrite 耗时约为 egglog **13.15 ms**、slotted **0.459 ms**。这些是两个具体实现的测量，不能把所有差异归因于 slot 机制；特别是它们的固定开销、数据布局、规则执行和前端实现不同。

变量实例的 root class 可以共享，但带不同自由变量映射的 `AppliedId` 不因此语义相等。1,024 个变量实例仍保留了 **2,048 个 root slot 映射条目**，所以总内存并不是常数。

## 正确性检查

`cargo test --bin slotted_baseline --features slotted-checks` 启用上游内部检查并执行四项测试：

1. 嵌套表达式跨层共享；不同自由变量映射仍可区分；将 `Neg(a)` 错接成 `Neg(b)` 不会被误判。
2. 不同常量不能通过 slot 重命名共享。
3. lambda 的 alpha 等价成立，同时保留自由变量，避免变量捕获。
4. 两个引擎分别饱和后，都能通过只读查询找到四个不同变量的全部 **120 种排列与二叉括号组合**，并拒绝跨输入根的错误等价。

每次测量结束后还验证首个、中间和最后一个 root；AC 对这些 root 检查全部 120 个表达式。验证不通过或 32 轮内没有饱和时直接失败，不把截断执行算成功。

## 测量边界

- 保留分配量来自 System allocator 计数器，包括引擎内部索引、union-find、缓存、规则对象与外部 roots；扣除进程参数等初始化基线，不包含分配器隐藏开销或 Rust System 之外的分配。
- 峰值 RSS 来自 macOS `/usr/bin/time -l`，是整个基准进程的峰值，也包含之后的只读统计和验证；与保留分配量不是同一指标。
- 构建耗时包含引擎初始化、表达式生成、各自的解析和插入 API。egglog 还包含解析后的类型检查/求值开销。它不是纯粹的 hashcons 操作耗时。egglog 使用 `eval_expr` 保存外部 roots，没有为每个 root 创建一个内部 let 函数。
- 节点数按语言构造节点统计；egglog 的 primitive i64 不计入节点数，但其内存仍计入分配测量。
- 这些是受控的结构与 AC 基线，不是生产编译器负载。常量负对照已经说明 slotted 并非总是更省内存。
- 当前没有集成规则历史或 Zobrist 增强。后续增强应当相对这个真实基线证明额外收益。

## 复现

在仓库根目录执行：

```sh
cargo test --bin slotted_baseline --features slotted-checks
cargo build --release --bin slotted_baseline
python3 experiments/slotted_baseline/run.py
```

单独运行：

```sh
target/release/slotted_baseline slotted renamed 1024
target/release/slotted_baseline egglog renamed 1024
target/release/slotted_baseline slotted ac 128
```

完整的 54 次运行在 `results/slotted_baseline/runs.json`，汇总在 `summary.json`，表格及方法在 `report.md`。版本、构建特征、机器信息及二进制/lockfile 校验和均记录在结果中。基准必须关闭 `slotted-checks`；脚本会检查这一点。
