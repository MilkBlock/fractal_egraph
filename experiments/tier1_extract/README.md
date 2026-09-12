# 直接提取 tier-1 Comb，并对照 tier-0 原规则

输入为已保存的真实 Math 前 6 轮 native tier-1 构造器图。
每个 Comb e-class 选择一个实际存在、具有有限 unit-tree-cost 的表示；跨所有根共享 Comb 定义。
不枚举候选子图，不读取旧 composer 的合成结果，不调用组合算法。
这不是全局最优共享 DAG 成本求解器。

- `existing_combs.egg`：1,389 个已有 Comb 定义，包括 Empty；父组合用变量引用，不重复展开。
  每个非 Empty 节点附有当前步骤对应的 tier-0 原规则注释。
- `tier0_rules.egg`：被引用的 17 条 tier-0 原规则，只保存一次，无输入和运行日程。
- `index.html`：左右对照 tier-1 表达式与本步 tier-0 原规则，可以按 R23/comb 编号搜索、点击父组合。
- `extraction.json`：native e-class/node 来源、所选表示、引用计数和原规则映射。
- `native_templates.json` / `tier0_rule_dictionary.json`：固定的源图与规则字典，供重现和核对。

**右侧是本节点末端 apply 的原规则，不是把整个父组合重合成为单条 tier-0 快捷规则。**
完整组合沿父引用表达；当前 tier-1 并未存储组合整体的 Math LHS/RHS，不能把外部合成冒充直接提取。

```
python3 experiments/tier1_extract/extract.py
python3 experiments/tier1_extract/render.py
python3 -m unittest discover -s experiments/tier1_extract -p test_extract.py
cargo test --test tier1_direct_extract
```

原生加载提取文件后，Empty=1、SmoothComb=1,270、CoarseComb=118，数量与源图一致；
未导入任何 Occurrence。原规则字典也通过原生类型检查，全部 1,388 个非 Empty Comb 均有对应项。
