# Slotted Canon 与 RipSatEgraph Canon 对照

本次加入的 `slotted_canon` 是第二种 canonicalize。它处理的是 **Graft attachment
space**：同一个 Grafting coarse rule 接到哪些结构对称的 `RipenInput` / `Apply`
边界位置。它不合并 tier0 e-class，不删除 producer/version，也不证明
`RipSatEgraph` 已经饱和。

`RipSatEgraph` canon 的输入是完整的已饱和主体，比较 typed value graph、row
visibility、ports 和 equality 形状。两个主体只有完整结构判等后才共享一个
`RipSatEgraph` body；trigger、EnclosureDescriptor 和实例仍分开。

`SlottedCanon` canon 的输入是：

- boundary slot 的 sort、role、upper/lower connection degree、alias class；
- Grafting coarse rule；
- Graft target、upper/lower connection 的 slot-class 多重集合；
- `RelativeBinding` 的结构；
- inherent residual 的 operator 集合。

它把 slot ID 擦除为结构 slot class。两个候选只有在这些结构字段都相同，且
只是对同一 slot class 内的位置置换时，才进入同一个 `GraftOrbit`。runtime
`event`、`instance` 和实际 `RuntimeFact` 不进入 canonical key。

示例：

```text
Input(0): sort=E, role=input, upper=1, lower=1
Input(1): sort=E, role=input, upper=1, lower=1

Graft(External slot 0)
Graft(External slot 1)
```

两者共享一个 orbit，保存一个代表 Graft 和两个成员映射。如果第二个 slot
是 `role=right`、或者 connection degree 不同，则不再共享。这个 role split
保留了类似 `m+n -> m-(-n)` 产生的 slot 不对称性。

当前第一版使用结构计数做有限 orbit key，属于保守的结构 canonicalize：它只
用于压缩 Graft 候选和安排一次验证，不会把候选直接变成语义 union。更复杂的
slot permutation、CoarseLayer/SmoothLayer dependency 和 EqualityHypothesis
应在验证器中继续细化；若这些字段不同，候选必须拆 orbit。

运行对照测试：

```sh
cargo test --offline --test slotted_canon
```

测试输出的核心对照是：

```json
{
  "ripsat_canon": {
    "bodies": 2,
    "equivalent_bodies": 1,
    "meaning": "complete saturated body equivalence"
  },
  "slotted_canon": {
    "candidates": 3,
    "orbits": 2,
    "orbit_sizes": [2, 1],
    "meaning": "Graft attachment orbit equivalence"
  }
}
```

因此两种 canon 压缩的是不同对象：`RipSatEgraph` canon 压缩已验证的局部闭包
主体；`SlottedCanon` canon 压缩尚未分别 ripen 的边界接入候选。前者决定
主体存储共享，后者减少 Graft 组合空间。两者不能互相替代。

## math_mini3 实际分析

用已经成功完成的 `math_mini3` bounded native 分析产物
`out/math-mini3-no-svg/bounded/layers.json` 运行：

```sh
cargo run --offline --example slotted_canon_math -- \
  out/math-mini3-no-svg/bounded/layers.json
```

这次分析的输入包含 102 个 `Comb`、129 个 Apply occurrence、53 个
`CoarseLayer` 和 40 个 `SmoothLayer`。按实际 CoarseRule 的 input roles
构造 478 个 Graft attachment candidates，`SlottedCanon` 压成 119 个
orbit，压缩比例为 24.90%，其中 55 个 orbit 含有多个成员。

完整结果保存在 `experiments/math_mini3/slotted_canon_report.json`。这是对已保存
native layers 的结构分析，不是一次新的完整 ripen 执行；它也不声称这些
attachment candidates 都已经实际提交为 Graft。
