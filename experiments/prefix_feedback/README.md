# 用压缩反馈选择 prefix 分支

本轮实现“冻结 babble 库 → 给候选 prefix 估计边际收益 → 选择一个 active
prefix → 保留其他分支”的接口与历史回放。没有修改 egglog 的 rule 调度，
也没有删除、合并或替换 tier-0 节点。

## 策略

`src/prefix_policy.rs` 是实际使用的策略状态机：

- 有正边际收益时优先选择收益最大的候选，平局按等待时间及 ID 排序。
- 每 4 次选择留一个机会给 coarse 或非正收益候选。
- 等待达到 8 次选择后获得优先权；这不是无条件的最大延迟保证。
- 没有正收益时退回 FIFO，不把最小负收益当成已证明的好分支。
- 库 epoch 不符、候选 ID 重复时拒绝；策略状态不能跨 block 混用。

`DependencyBlockStore::choose_prefix` 接入现有可用 CombinedPrefix 选择，
要求完整的候选评分集合、正确的 block version 和冻结模型 epoch。
`selected_prefix` 只返回当前仍 active、usable、版本匹配的选择。其他
prefix 不删除；union/revalidation 引起版本或可用性变化后，旧选择不能
继续使用。该方法由调用方显式调用，没有暗中启用新的运行时优化策略。

现有 store 仍是观察索引，且已有可执行 prefix 的生成范围没有在本轮扩大。
多源 DAG continuation 的回放使用同一个策略，但不冒充已编译的 shortcut。

## 分数与冻结模型

使用 `deep_combs/depth-4.learning.json` 中已经学好的库，SHA256 和 epoch
写入结果；回放期间不训练。符号匹配保留重复参数约束，并逐个验证展开。
评分器是对冻结库的精确语法匹配和局部 AST 成本选择，不是再次运行 babble
beam 搜索，也不是针对 DAG 字节成本的最优提取器。

每个待选分支带着自己的 base prefix，计算：

```
gain = (raw_child_bytes - raw_parent_bytes)
     - (coded_child_bytes - coded_parent_bytes)
```

两侧都包含相同、已经驻留的库定义，并使用同一个精确子树 DAG codec。
否则普通符号表或子树与库的重叠，也会被误算成宏复用收益。绑定、边界和
源/目标位置属于编码内容；固定原始规则字典、原始证据元数据在两侧省略。
分数是该局部 prefix 表示的增量估计，不是 tier-0 实测字节。

## 回放范围

对同一份历史的 25 个留出根，各使用其深度 4 祖先 cone，从固定 seed
开始发现已记录的 consumer continuation。候选引用各自的 parent prefix，
选完会发现后续 continuation，未选分支留在 frontier。每个案例预算 6
次选择，对照 FIFO。coarse 表示目标的直接 producer 超出其 base prefix。
这些外部支撑已经存在于历史中，回放不会假装凭空生成它们。

这是过去事件的表示选择，不是按新顺序重跑规则。cone 相互重叠，也可能
与训练窗口共享事件，因此不是独立工作负载上的泛化或因果收益证明。

## 本次结果

| 指标 | FIFO | 压缩反馈＋探索 |
|---|---:|---:|
| 选择次数 | 89 | 89 |
| 到达目标根 | 25 | 25 |
| 正收益选择 | 20 | 19 |
| coarse 选择 | 35 | 34 |
| 探索配额／等待优先选择 | 0 | 15 |
| 首次正收益选择的平均序号（同样的 8 个有正收益案例） | 1.875 | 1.625 |

25 个案例中有 6 个改变了选择顺序。案例 scope 0 / match 2130 中，策略
将正收益的 940 提前到 457 前，下一步通过探索配额处理 457。完整候选
表、选择原因、相对 base 和分数保存在 `results.json`，便于核查。

局部分数加总为 839 / 727，但分支 base 不同且窗口重叠，这两个和不能
解释为全局存储节省。本例只说明反馈确实影响了选择，并保留探索机会；
总体上没有证明优于 FIFO。tier-0 共享表示及其匹配接口仍是后续工作。

## 复现

```
python3 tools/babble_adapter/scripts/prefix_feedback.py
CARGO_INCREMENTAL=0 cargo test --lib prefix_
python3 -m unittest discover -s tools/babble_adapter/scripts -p test_prefix_feedback.py
```

测试覆盖 coarse 在持续正收益到来时仍有机会、等待优先、负收益退回、
模型版本与 block 版本、候选覆盖、失效 prefix、重复 binding 以及“仅安装
库不能算压缩收益”。
