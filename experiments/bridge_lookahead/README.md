# 有预算的 bridge 探索与相对 binding

本轮在既有 prefix 回放上增加最多三步的方案与保护窗口，并提供可逆的
相对 binding 地址。没有修改原优化器的规则执行或 tier-0 存储。

## 桥接预算

`bridge_plans.py` 对已知历史 continuation 最多向前看三步，每个候选最多
检查 128 个路径状态。只有净估计为正且进入额外库复用的多步方案，才替换
单步方案。这里的 library entry 指压缩后出现额外的库调用，不是已经证明
闭合的 fractal trigger invariant。

评分保守地扣除启动费用：

```
净估计 = 整段的编码压缩优势
       - 首次库复用增加之前，prefix 编码的正增长
```

两侧都驻留相同的冻结库。启动费用是该表示的保守代理，不是实际新增的
tier-0 字节或规则执行时间。完整条件、binding 和边界仍在编码中。

选中后最多保护两个后续步骤。Rust Policy 的 `record_reserved` 每步更新
等待时间，记录被占用的探索配额，并在下一次普通选择补回。计划的模型、
图版本或下一候选可用性改变时取消。不会创建不存在的历史 continuation。

## 实测

同样的 25 个重叠留出 cone，每例最多六次选择：

| 编码和计费 | 完成的多步方案 | 首步没有压缩优势的方案 | 到达目标 |
|---|---:|---:|---:|
| 原编号，忽略启动费（对照） | 9 | 4 | 25 |
| 原编号，计入启动费 | 6 | 0 | 25 |
| 相对接口，计入启动费 | 4 | 1 | 25 |

即时选择对照也到达全部 25 个目标，因此不能据此声称总体搜索更优。
三步方案额外检查的路径状态分别为 224、252、272，工作成本未被隐藏。

相对接口例：从 event 203 的 R23 开始，依次选择 362/R1、906/R9、
1575/R1。首步压缩优势为 0；整段优势 451 字节，扣除进入模式前的
130 字节编码增长，净估计 321 字节。完整记录在 `relative.json`。
当前真实案例没有观察到“首步压缩优势严格为负但整体为正”；合成测试
覆盖了两步先变差、第三步才盈利的情况，不冒充真实 workload 结果。

这是已知历史上的 lookahead，未来依赖已经存在于记录中；在线使用还
需要候选推演及事实可用性验证。这里不能把收益解释为在线加速、因果必要性
或内存节省。不同基点和重叠窗口的局部净估计也不能相加成全局节省。

## 相对表示

见 `../relative_bindings/README.md`。相对地址由 root interface 与连续
read-interface 组成，可以组合；跨分支共享祖先保留同一个引用。规则内
位置拆成 anchor 和 argument route，原有 wiring、等式约束与类型不删除。

## 复现

```
python3 tools/babble_adapter/scripts/relative_bindings.py
python3 tools/babble_adapter/scripts/bridge_lookahead.py --ignore-startup-cost
python3 tools/babble_adapter/scripts/bridge_lookahead.py
python3 tools/babble_adapter/scripts/bridge_lookahead.py --relative
python3 -m unittest discover -s tools/babble_adapter/scripts -p test_bridge_plans.py
CARGO_INCREMENTAL=0 cargo test --lib prefix_
```
