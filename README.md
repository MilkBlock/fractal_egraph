# egg_layout

在固定的 egglog 上游源码之上，研究局部规则历史、重复结构检测和 Zobrist 共享存储。

## 查看我们新增的修改

`egglog-baseline` 是未包含本地修改的基线标签。其 `egglog/` 子树与
`saulshanabrook/egg-smol` 的 `ebba7bb902bdc1b0f377b6bb22c06ac305912674`
源码树完全一致；它是本项目实际导入的版本，不是另选的最新主线版本。

```sh
# 全部本地改动，不重复展示上游导入
git diff --stat egglog-baseline..HEAD
git diff egglog-baseline..HEAD

# 只看 egglog 内核修改
git diff egglog-baseline..HEAD -- egglog/

# 按功能查看本地提交
git log --oneline --reverse egglog-baseline..HEAD
```

## 提交导航

这段历史根据原有工作区整理而成；基线来自真实上游 Git 对象，其余提交按逻辑边界整理，没有伪造过去的提交时间。

| 提交 | 内容 |
|---|---|
| `c7553a6` / `egglog-baseline` | 原始上游源码，503 个文件 |
| `0aea2f8` | 内核 TraceSession、逻辑匹配与 action 通道结果 |
| `523eb93` | Rust 实验框架、依赖锁和缓存忽略规则 |
| `e138ba4` | 局部 rule-history 消融、数据和结果 |
| `63fe94b` | 动态共享存储、Zobrist 增量对照和测量结果 |

## 实验入口

- [依赖驱动的活跃 block 索引](experiments/dependency_blocks/README.md)：A→B→C 增长、组合 prefix、跨块交互和来源失效传播；尚未改变原生匹配或存储路径。

- [提交结果驱动的组合见证](experiments/dependency_witness/README.md)：内核记录实际插入与后续行读取，生成带 producer–consumer 证据的 A+B prefix seed；明确保留 union 来源缺口。

- [Prefix 见证消融 1](experiments/prefix_witness/README.md)：单独测量 rule apply 见证能否替代局部 prefix 重新匹配，保留全局和同 class 覆盖缺口反例。

- [连续 rewrite 在线组合](experiments/rule_combine/README.md)：预处理等价快捷规则，按局部匹配动态启用，比较完整闭包、轮次和实际开销。

- [Slotted e-graph 真实基线](experiments/slotted_baseline/README.md)：固定上游版本，验证跨层 slot 共享，并与本地 egglog 比较实际构建和饱和。

- [局部规则历史消融](experiments/rule_history/README.md)：1,536 个真实执行快照；区分结构、规则身份和绑定关系的贡献。
- [动态 Zobrist 存储实验](experiments/zobrist_storage/README.md)：原始存储、仅检测、共享并重算指纹、共享并增量更新四组对照。
- [上游来源](egglog/UPSTREAM.md)：导入版本及本地内核扩展的边界。

共享存储目前是独立实验原型，尚未替换 egglog 表和查询索引。`Survived` trace 表示 action 通道保留，不表示某条 mutation 实际提交。

## 验证

```sh
cargo test --manifest-path egglog/Cargo.toml -p egglog --test trace_instrumentation
cargo test --lib
python3 -m unittest discover -s experiments/rule_history -p 'test_*.py'
cargo fmt --package egg_layout -- --check
```

构建目录、Python 缓存和可重新生成的二进制事件流不纳入版本控制。复现所需的源码、文本快照、指标和报告均已提交。
