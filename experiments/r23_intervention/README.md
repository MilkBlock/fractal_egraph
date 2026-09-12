# R23：调度截断还是 trigger 缺口？

真实 egglog 配对干预。程序来自 math_microbenchmark 的导出文件，保留原始
24 条规则与初始表达式，按原始 `(repeat 11 (run))` 的单轮边界观测。
未运行 visualization-only combined 规则。

## 选点与处理

从已有 product-integral seed 的两个因子方向沿 T(a,b,x)=(Diff(x,a),Integral(b,x),x)
前进，直到 LHS 已存在、RHS 或 union 尚不完整的第一个位置。在第 0、2、4、6 轮取快照。
第 0 轮尚无交换后的方向，所以总计 7 个样本。这些是同一 seed 谱系的配对检查点，
不是七个独立随机样本，不报告统计显著性。

每个样本的相同图状态、相同具体 binding 做三组处理：

1. 原始调度继续 1…4 轮，使用未修改原始轨迹的后续快照测量。
2. R23 连续 1…4 次，每次将原始规则追加 canonical binding 等式，只作用于一个
   (a,b,x)，完成后沿 residual 的 T 转移；不是把整个 R23 ruleset 全图运行。
3. 同一 binding 的 Mul(a,b) 先执行合法原规则 R1，再做相同的局部 R23。

第三组不是“按缺口选出的修复”：检查发现纯 R23 不需要额外 trigger 前提，因此这里
用 R1 作为额外 effect 对照。它在七例都净增一个交换节点，但没有改变该族深度。

对齐的是 R23 执行机会数，而不是总 match 数、CPU、写入预算。原始全局调度做了
其他工作，第三组多一次 coarse 操作。不能用本实验宣称一般优化结果等价或性能提升。

绑定通过现有表达式的 global 引用固定，并检查锚定未新增原始节点。执行仍由原生
规则、匹配和 union 完成。小图测试确认另一个 binding 不会被局部操作展开。

## 实测结果

七例的三组深度向量全部为 1/2/3/4。没有出现“连续执行更深”或“只有注入后才能继续”。
每个局部 R23 的 RHS 都提供了下一步完整 LHS；最后的位置仍 enabled，而非语义终点。

四步目标族在固定 11 轮参考图上的去重覆盖全部为 26 个 e-node。
局部四步净增 24 个节点，R1 对照净增 25 个；原始全局调度净增 354…134,870 个。
净增是表行总数差，不是 provenance Inserted 数，不包含辅助 global 表，也不是物理内存。

## 为什么这是这个族的结构性质

定义

```
L(a,b,x) = Integral(Mul(a,b),x)
T(a,b,x) = (Diff(x,a), Integral(b,x), x)
```

实际 R23 只有这一个正向 constructor LHS，没有额外 guard。其 RHS 的 residual 子项恰为
`L(T(a,b,x))`。所以每次合法完成 R23 后，下一次 LHS 已被这次动作构造出来。
在该规则和普通单调 constructor/union 语义下，trigger 对 T 闭合；不需要族外规则
为这个直接递推补条件。这个论证不证明每一步都有新的不同节点，也不保证调度公平，
更不证明其他含 coarse 条件的组合族同样闭合。

因此实验不支持把这个 R23 族的有限短链归因于 data-related 条件不足。原始调度中
全局其他结构的快速增长是实际观测，但其必要性与能否安全避免属于后续问题。

## 固定参考图与跨图身份

不同分支新增的 Value ID 不能直接比较。我们将明确构造的表达式按 constructor lookup
映射回同一份原始 11 轮参考图，统计目标链的显式 LHS/RHS 节点并集；a/b/x 作为洞，
其内部节点不计。所有参考查询都只读，并最终检查参考图原始行未变化。

lookup 无法解析的 reference occurrence 单独计数；不把无法解析直接认作语义上新节点。
本轮七例、三组、四个观察点均无未解析 occurrence。覆盖不含 R1 独立效果，也不是
全局分支全部新增结构的覆盖率。

## 复现

```
python3 experiments/r23_intervention/run.py
cargo test --bin r23_intervention
```

[comparison.md](comparison.md) 是结果简表；[results.json](results.json) 包含具体入口表达式、
各次原生局部步骤、节点净变化、暂停原因和固定图覆盖。
