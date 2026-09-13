# 有证书的有限 DAG 结构嵌入

```sh
cargo run --release -- embed-dag experiments/dag_embedding/small.json \
  experiments/dag_embedding/extra_root.json out/embedding.json
```

结果应为 `found`，node_map `[1,2]`：小图根 0 映射到大图内部 1，大图自身的根仍是 0。输入端口和输出接口同时给出投影。输出文件必须不存在，可在末尾传搜索预算，默认 100000 个候选赋值状态。

## 判定范围

- 在大图所有相容节点中搜索小图的根；不是根对根匹配。
- Node.label 精确相同；小图的显式 constraints 必须在大图中成立。对象按递归字段包含检查，数组/标量按精确值检查；不做一般逻辑求解。
- 节点映射单射且全局一致，保留共享节点、不同 apply 角色、边的方向、完整 binding 标签和重边数量。
- 非诱导嵌入：允许新增节点和边。边界接口可以投影到大图的内部端口，不要求还是大图的外部入口。
- 找到一份映射即可给出正证书；穷尽失败返回 absent；预算耗尽返回 unknown_budget，不作为反例。
- 仅处理有限 DAG。循环输入拒绝；递归 grammar 要显式展开后再检查。

公共 Rust API 位于 `src/dag_embedding.rs`。目录适配器位于 `src/catalog_embedding.rs`：默认将已有模板的有限单元转换成 DAG，显式 `dependency_dag` 可提供更复杂的有限组合图。默认适配器不是任意递归展开器，也不归纳额外 egraph 等价式或 alpha-renaming。不能把当前标签下的 absent 理解为所有语义表示下都不存在 dominance。

## 排序

目录新增 structural_dominance：逐有序模板对检查嵌入，记录 node_map、root_image、interface_projection 和未决预算项。反向已知不成立的嵌入形成严格偏序，优先用于排序。相互嵌入的有限图保留为非严格结构关系，不自动 union；启动条件、effect 和执行语义仍需独立证明。

原 observed_dominance 仅作为历史覆盖指标保留，不再决定结构排序。

## 验证

`cargo test --release --test dag_embedding --lib --test recursive_patterns`

包括根上新增节点、额外分支、binding 标签不符、alias 约束不符、共享 diamond 不得匹配成两份 D、两个独立节点不能压成一个、循环输入、预算未知，以及 512 对小 DAG 与独立全排列枚举的对照。目录测试使用互不相同的事件 ID 和入口，验证新的结构关系不依赖观测集合包含。
