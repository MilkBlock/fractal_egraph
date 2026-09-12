# 可恢复的 fractal 候选前沿

这是候选族观察框架，不是无限规律认证器或新的 egglog 调度器。
关键区分：下一步没有被调度、trigger 缺条件、已启用但缺后续效果、查询未穷尽，
都不能直接当成真正的 turning point。

## 框架

`src/fractal_frontier.rs` 复用 `BindingRelation` 定义候选：

- `Family`：trigger 和 entry→exit 递推关系，端口有类型且递推出口能再次作为入口。
- `State`：family + 具体 typed binding，保留数据身份和别名，不以结构 hash 替代 binding。
- `Transition`：已见证的递推边，或 coarse bridge 到另一候选族；有分叉时保留多条边。
- `Pending`：trigger gap、effect gap、enabled-but-unmaterialized、unknown。
- `Frontier`：保留入口，每个快照 epoch 从入口重新查询；不因别的规则运行而丢掉该族。

每个 epoch 都重新 canonicalize 和验证可达状态，不沿用 union 前的终点或旧边。
这是保守的全量重查策略，尚未实现按事实/union 订阅的增量唤醒索引。
访问状态数和边数均有显式预算；超限记录 deferred/unknown，不声称饱和。
环与共享后继通过完整状态去重，但重复状态不构成无限 fractal 或幂等证明。

Oracle 适配器负责在同一稳定快照中求解候选关系、枚举具体 binding 并验证效果。
它必须自行限制一次查询的候选生成量；核心预算限制保留的展开图，不能阻止适配器自身
执行昂贵 join。`complete` 只相对于适配器供应的候选空间。

没有自动挖掘递推关系、bridge、外部 binding，也没有把结果反馈给 prefix 选择或 babble。
没有从普通 match 冒充 committed effect，更没有按事件时间邻接创建依赖。

## 原生 egglog 调度对照

`interleaved.egg` 提供有限链：

```
At(n) + Next(n,m) + Permit(n) + Gate(n,g) + Have(g) -> At(m)
```

接口转移 n→m，Next 显式限制在 0…8。
- n=2 缺 Permit，coarse 规则补入真实事实后恢复。
- n=4 的 Gate 与 Have 不满足连接，union (V 4)/(V 99) 后恢复。
- noise 规则只推进独立的 Tick 链。

两种日程均执行 8 次 grow、8 次 noise、1 次 coarse、1 次 unlock。
每步执行后通过原生查询检查 trigger 与整个递推关系。适配器自动枚举 Next 的候选出口。

| 日程 | 最长连续 grow 调用 | 最终前沿图状态 | 递推边 | At(8) |
|---|---:|---:|---:|---|
| 分段集中 | 4 | 9 | 8 | 原生检查通过 |
| 每次 grow 后插入 noise | 1 | 9 | 8 | 原生检查通过 |

这里的“调用”是显式 ruleset step，不是内核单个 match 的执行次序。
结果说明在这个小用例中，交错缩短了连续调用段，但没有破坏 binding 链；
不能据此断言真实 math 调度没有影响，或它就是此前覆盖率的原因。
最终闭合只相对于显式 Next 表；不是从数据归纳出了无限规律。

`results.json` 保存各阶段状态、具体 binding、缺口与结构见证。
这些是原生已存事实支持的结构连接，不提供 committed 事件来源归因。

## Math 接入

`src/bin/tier0_probe/fractal.rs` 将已验证的 R23 原生 LHS/RHS/union 索引适配到同一框架，
沿用 T(a,b,x)=(Diff(x,a),Integral(b,x),x)。输出位于
`experiments/integration_family/results.json` 的 `resumable_frontier`。
这是最终图上的有限观察；不是 math 运行中逐轮记录，且尚不自动搜索 coarse 后继。

## 复现

```
FRACTAL_FRONTIER_REPORT=experiments/fractal_frontier/results.json cargo test --test fractal_frontier
python3 experiments/integration_family/run.py
cargo test --bin integration_family --test trigger_bridge
```

额外检查涵盖：分叉保留、跨族 coarse 边、环去重、重新 canonicalize、
状态/边预算、未完整枚举、以及 epoch 回退拒绝。

当前 math 验证：固定 binding 的 8 个已访问状态覆盖 50 个 e-node；
两个入口合计 16 个已访问状态覆盖 99 个 e-node。两者都保留 deferred 后继，
明确标记预算耗尽。数值与此前有限深度对照一致，没有新增覆盖或压缩率主张。
