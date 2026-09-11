# 相对 binding 地址

`relative_bindings.py` 将 deep comb DAG 的 EventRef 编号变成根接口相对地址：

```
Address = Root | Via(Address, ReadInterface)
```

ReadInterface 保留 producer/consumer 规则、表、read slot，以及双方的
规则内位置。位置进一步表示为 `RuleAnchor(head/body, atom, expr)` 加
`ArgRoute`，不删除 anchor 或把不同 LHS atom 混在一起。

共享祖先使用最短、再按接口字典序选择的同一地址。不同事件不会因规则名
或实际值相同而合并；地址有歧义时拒绝。`compose_address` 可拼接路径并
检查已知规则接口；`relative_suffix` 在跨接口引用时返回 None，不伪造
本分支的后代路径。这些是地址语法运算，不能代替真实 binding/guard 验证。

所有 124 个样本都能通过 renaming 证据精确恢复原 DAG。编号变化后相对
表示一致的测试也覆盖了共享祖先。原 normalized wiring、类型和 boundary
不变；这不是一般的图同构、e-class 等价或可执行快捷规则推断。

## 学习结果及成本边界

沿用 99/25 划分和最多四个库的 babble 配置；全部无损展开通过。

| 相对表示内部 | 原始 | 库表示 |
|---|---:|---:|
| 学习 AST | 69866 | 60344 |
| 留出 AST | 16559 | 15021 |
| 学习 DAG codec 字节 | 83970 | 79946 |
| 留出 DAG codec 字节 | 33385 | 32959 |

相对表示内部可以压缩，但地址本身更大。之前原编号的学习 DAG 基线只有
51049 字节，明显小于 79946；恢复原编号所需 renaming 证据还要额外存储。
因此不能用相对表示内部的降幅宣称净存储更优。现阶段它适合作为学习和
查询的派生视图；紧凑编号仍可作为实际存储。固定规则字典另列，元数据
不计入两侧 codec 数字。

`learning.json` 固定本次库定义，供 lookahead 回放冻结使用。较大的
`corpus.json`、`renaming.json` 和 `.programs.json` 由脚本重建，不提交。
