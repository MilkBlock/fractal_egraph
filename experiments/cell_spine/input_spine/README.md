# 输入直接构造选定 spine

`compiler` 默认接入；原生 egglog 执行，未修改执行引擎。求解器在已经选定的 spine/marker 下选择输入初始化和需要保留的行，不是按大小贪心，也不是重新展开饱和图。

| 输入 | 原输入流程的物理行 | 直接构造 spine 后 | 展开后 e-node / e-class |
|---|---:|---:|---:|
| 三项左结合 | 12 | **7** | 15 / 7 |
| 三项右结合 | 22 | **9** | 15 / 7 |
| 两个三项式，内部结果被外部 Use 引用 | 29 | **24** | 33 / 17 |
| 普通输入与重复绑定输入混合 | 24 | **19** | 25 / 13 |

同一行的两种方式使用**相同生成库、相同输入值和同一原生内核**。包含辅助表、全局根引用和已 subsume 的 marker。这里只比较表行及展开规模，不声称 RSS 或速度改善。正常 Bake 的小输入还通过了原有的原生完整状态校验。

左结合输入的初始化只需：

```egg
(let root (Add!! (Add!! (Leaf! "a") (Leaf! "b")) (Leaf! "c")))
(Summary-1! (Add!! (Add!! (Leaf! "a") (Leaf! "b")) (Leaf! "c")))
(run-schedule (saturate Run!))
```

3 行叶子、2 行 Add、1 行 marker、1 行根引用，共 **7 行**。没有先创建两行 `Add!` 再复制、删除。右结合样例还需要继承接口用的额外 canonical 节点和 marker，故最终为 9 行，不能声称它也只剩一条纯链。

边界样例中的 `inner` 和 `held_inner` 被命名或被外部使用。原来的两行内部 `Add!` 被保留，另外两行外层 `Add!` 可以省掉。测试验证了后续插入 `Use! held_inner`，以及先插入 `Use! fresh`、再 union 到 `held_inner`，都能继续匹配并导出正确状态。

另外，`input_spine_specialized.egg` 同时含普通三项式和 `Add x x → Repeated x`。大 cell 覆盖了相同的节点形状，并不保证这个额外 effect。模型要求已有接口覆盖证明，否则保留原始匹配行；负例测试确认强行删除这些行确实会丢掉 `Repeated`。

当前范围：无初始 union 的有限输入 DAG，直接构造单根、无等价 guard 的已验证 coarse facts，并直接构造选定的 canonical 表示。多根/带 guard 的匹配、借用型布局和初始 union 保持原流程。初始化目标是共享表达式行的成本，未反馈进前一阶段的 spine/marker 选择；也没有承诺任意未 Bake 的后续规则都能查询隐藏结构。

## 复现

先分别生成四个例子（输出目录需为空）：

```sh
for case in left right boundary specialized; do
  out/cell-spine-venv/bin/python -m tools.cell_spine.compiler \
    --source experiments/cell_spine/fixtures/input_spine_${case}.egg \
    --output out/input-example-${case} --skip-build
done
```

原生表行对照，不再采集 trace：

```sh
out/cell-spine-venv/bin/python experiments/cell_spine/input_spine/compare.py \
  --bakes out/input-example-left out/input-example-right out/input-example-boundary \
          out/input-example-specialized \
  --output out/input-example-comparison
```

`results.json` 是本次精简结果；完整表计数由脚本保存在输出目录。`input-selection.lp`、`input-selection.sol` 和 `input-plan.json` 可查看具体选择。两个求解阶段都无候选预算；许可证不足时会明确失败。
