# 用 egglog 替代 babble 的候选生成与匹配：差分实验

这一步使用真正的 egglog 规则计算 anti-unification，不是在 Rust 中算完
公共模式后只把结果存进 egglog。实现规则见 `anti_unify.egg`。

## 已迁移和未迁移的部分

| 部分 | 当前实现 |
|---|---|
| 两个节点配对后的递归公共结构 | egglog Need / NeedList / AU / AL 不动点 |
| 重复洞身份、不同洞数量上限 | egglog Hole 构造器和 Set 集合 |
| 不可共现子项和超出参数上限时的回退 | egglog DeadList 和 Hole 规则 |
| 候选对输入图的匹配 | 共享 SubMatch 子模式关系，egglog 自底向上不动点 |
| 变量重命名、非平凡候选过滤 | Rust，按 upstream 条件规范化 |
| 匹配签名去重和最小模式大小 | Rust，消费原生 egglog 匹配表 |
| 输入 DAG、配对 frontier 和可共现关系 | 固定 upstream babble 提供的测试夹具 |
| 库集合选择、beam extraction、lambda lifting | 尚未迁移，既有适配器不变 |

因此这里验证的是分层替代的可行性，不是完整的 egglog 版 babble。

## 差分基线

`tools/babble_adapter/src/au_reference.rs` 在同一份 typed-AST 学习语料上
直接调用 upstream `LearnedLibrary::new`，导出原始候选和去重后的匹配签名。
设置保持 `learn_constants=false`、最大参数数 3。没有领域等式。

输入是原 math combine 语料的 99 个学习类。夹具含 1372 个不同语法节点、
100180 个配对状态。另一后端只读取这些原始节点和可共现信息，通过
egglog 从空 AU 表开始推导；expected 候选仅用于最终比较，不参与推导。

第一层比较：候选按首次出现顺序命名洞后，集合必须逐项相等，不能只比较
候选数量。第二层比较：所有原生匹配形成的 `(root, sorted(actuals))`
签名及每组最小模式大小，必须与 upstream 一致。相同最小大小的语法
代表元可以不同。匹配分组是语料内的抽取成本判据，不是规则语义等价证明。

完整结果保存在 `results.json`。独立于这里的库选择及展开基线仍保留在
`experiments/babble/`；本轮未用新的候选后端重跑完整库选择，也不从候选
一致直接宣称最终压缩率或资源消耗相同。

本次完整差分结果：

| 检查项 | egglog | upstream | 结果 |
|---|---:|---:|---|
| 原始候选集合 | 915 | 915 | 逐项一致，无缺失或多余候选 |
| 匹配签名分组 | 915 | 915 | 每组签名及最小展开树大小一致 |

egglog 生成 99451 条 AU 结果和 3992 条候选匹配记录。匹配签名会保留
实参多重性，因此重复绑定不能简单变成集合。本例所有候选的匹配签名
互不相同；额外单元测试构造了两个不同模式属于同一组的情况。

实现中先将不变语法表建立为 keyed functions，再共享子模式匹配关系，
避免把大模式直接展开为一条很大的 join。这里不报告时间或内存加速比。
测试基线的模式成本使用展开树节点数，与 upstream `PartialExpr::size`
一致，不能误用 egg 中已经 compact 的 PatternAst 存储长度；这一区别
有独立回归测试。

## 当前语义边界

只支持无环、每个 e-class 只有一个 e-node 的静态语法 DAG。babble 基线
未运行领域 equality saturation，因此本轮 corpus 满足这个条件。尚未
处理含多个等价节点或循环的 e-class，也未把 co-occurrence 分析迁入
egglog。所有 ID 是该夹具内的语法身份，不是 union 后仍稳定的运行时 ID。

虽然语料保留原始 union 动作的语法，这里的 AU 不动点不是在原程序图中
重新执行那些 union；它是在另一张学习用的关系数据库里构造公共模式。
这不证明 fractal 递推、不替换原优化器调度，也没有内存节省主张。

## 复现与测试

```
python3 tools/babble_adapter/scripts/compare_egglog.py
CARGO_INCREMENTAL=0 cargo test --bin egglog_babble
cargo test --release --locked --manifest-path tools/babble_adapter/Cargo.toml
```

脚本固定沿用已有 bootstrap 的源码和 Cargo.lock，并对学习和原生推导
进程设定超时。较大的 `math.fixture.json` 由原程序生成，不提交。
边界测试覆盖重复洞计数、不可共现子项回退、参数上限回退、重复洞匹配
约束，以及不同候选共享相同匹配签名时的去重。
