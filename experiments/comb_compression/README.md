# 压缩生成的 rule comb

学习对象是已生成的 124 个组合类。原子 rule 的定义只保存一次，作为固定
字典；每个 comb 表示消费者、producer→consumer 连接、双方位置、producer
共享引用、typed binding、端口标签与 boundary。输入不重复嵌入原子 rule
的 body/head 包装。连接位置由字符串变为可参与 anti-unification 的结构。

`comb_corpus` 将每个结构还原为原始 shape 并逐项核对 wiring；例如两条边
都引用 p0 时不会被误编码为两个 producer。规则字典保留完整原始定义，
包括 union。这里保留的是生成 comb 的结构，不是重新推断已提交的 effects。

## 学习和检查

调用 upstream babble 的候选生成与去重，再只保留包含具体 `CombEdge` 的
模式，送入同一 beam 库选择。这个通用过滤器不包含任何具体 rule 名称：
抽象必须包含至少一条 producer→consumer 连接，不能只共享包装或路径。
beam=16、最多选择 4 个库、最多 3 个参数、2 次 library rewrite 迭代，
没有领域等式。固定 99 类学习、25 类留出，发生次数未参与学习加权。

输入得到 457 个原始候选，其中 137 个包含连接，最终选择 4 个库。
全部 124 类的库展开与编码前程序精确一致。精确子树 DAG codec 同样
检查展开一致性，不能只用树节点减少来声称超越 hashcons 的压缩。

## 提取出的子组合

精确定义和调用实例见 `libraries.md`、`library_usage.json`。本次 ID：

| 库 | 连接模板 | 学习引用 | 留出引用 |
|---|---|---:|---:|
| F5 | R15→R15 的 Diff 连接，producer 位置 `args[k]/args[1]` | 5 | 2 |
| F57 | R1 的 Mul 输出连接到 R15 的乘法位置，其余支撑作为参数 | 4 | 0 |
| F74 | R0 的 Add 输出连接到 R2 根位置，另一份支撑作为参数 | 4 | 1 |
| F46 | R15 的 Mul 输出连接到 R1，producer 分支位置作为参数 | 2 | 0 |

R15 是乘积求导。F5 的完整位置是 producer
`head/0/expr/1/args/k/args/1` → consumer `body/0/expr/1`，记录中的 k
取 0 或 1，对应输出的两个求导分支。这是跨不同 comb 复用的递归依赖
连接模板；并不是对任意深度成立的 fractal 归纳证明。binding 仍保留在
调用者的完整 comb 中，不能把这个位置模板当成独立可执行的快捷规则。

## 压缩结果和反例

库定义和调用开销已计入：

| 度量 | 学习原始 | 学习后 | 留出原始 | 留出后 |
|---|---:|---:|---:|---:|
| AST 节点 | 5793 | 5666 | 1442 | 1438 |
| 精确子树 DAG 的独立 JSON codec 字节 | 22071 | 22487 | 10955 | 11182 |

树表示仅减少约 2.2% / 0.3%；在已经共享精确子树的表示上反而增加
416 / 227 字节。固定规则字典为 5566 字节，两侧相同且另列；codec 包含
自己的符号表与节点引用，不包含两侧相同的发生次数等元数据。这不是运行
时 RSS。因此当前结果证明能发现子组合，但尚无超越这个 DAG 基线的存储
收益。若目标是本样本的最小存储，应保留原始 DAG；当前 babble 选择目标
仍是 AST 成本，没有针对 DAG 字节成本优化。

## 复现

```
python3 tools/babble_adapter/scripts/compress_combs.py
CARGO_INCREMENTAL=0 cargo test --bin comb_corpus
cargo test --release --locked --manifest-path tools/babble_adapter/Cargo.toml
```

语料和结果是文本产物。较大的 `results.json.programs.json` 复现时生成，
包含完整压缩后的库和 comb 调用程序，Git 不跟踪。此实验未使用新运行，
未将局部 producer–consumer 组合扩展为更长历史 DAG，也没有把已学抽象
递归回灌为下一轮学习节点；这些限制不应被压缩数字掩盖。
