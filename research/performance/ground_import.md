# Ground 导入与 binding 索引

本轮以 `a2d53f3` 为基线，保留 RowFact、祖先 Provides 传播及完整 support 查询。

## 修改

- `EGraph::run_ground_import`：先校验整批 ground 数据的函数 schema，再复用 `(input)` 的 TableAction 原生写入接口。每个顶层 action 完成后 flush，后续 lookup 能看到之前的 set。避免将每条数据重新做通用约束求解、编译成临时执行规则。
- 仅支持已声明 constructor、no-merge function 的读取/set、整数与字符串常量。拒绝 union、primitive、变量、自定义 merge、global 及 proof/trace/encoding 模式。它只用于元图数据导入，tier-0 规则执行不变；运行时错误与普通 action 一样不是事务回滚。
- `TypedOutputAt` 和 `NeededParentPort` 将已有 OutputAt/V 与 NeedLocalPort/ParentPort 的连接结果物化为带类型、位置的索引关系，再解析 LocalResolved。仍由 egglog 规则推导，不直接把期望 Binding 当作验证结果写入。

## 验证

在同一份 6 轮历史上，2366 对 SupportsUse 与基线完全相等。除 native 分配编号外，完整视图、combined rule 文本、source steps 和 binding routes 一致。在线/离线/历史重放、独立原规则重放、RowFact 反例全部通过。

新增内核测试比较普通 action 和 ground loader 的连续 set 可见性、重复 constructor 去重、完整批次类型检查，以及缺失 key、冲突 set、primitive/union 等拒绝路径。

## 性能

实际 release egglog 单进程运行；单次 macOS sample/ps 测量，编译不计时。原始数值见 [ground_import.json](ground_import.json)。短任务 RSS 采样可能漏掉瞬时峰值。

| Math 6 轮 | 上一版 A | 本轮 |
|---|---:|---:|
| 程序 wall | 1.149 s | 0.279 s |
| tier-1 导入 | 0.778 s | 0.079 s |
| tier-1 饱和 | 0.128 s | 0.096 s |
| 采样 RSS | 52.3 MiB | 42.5 MiB |

两项改动一起测量，不能把所有收益都归给其中一项。只有索引改动的历史重放也已检查 support/视图一致，但不据此声称独立速度收益。

11 轮仍未完成：40.287 秒超时，完成了第 10 轮，第 11 轮尚未完成。第 10 轮累计 121120 个有效 apply，新增导入约 6.65 秒、饱和 8.34 秒。峰值 RSS 4404.6 MiB；与上一版只完成第 9 轮时的内存不在同一进度，不能直接比较压缩率。下一步需要继续分析大轮次的 collector、binding join 和祖先 effect 传播。

```sh
cargo build --release
python3 research/profile_capture.py --rounds 6 --timeout 30 --max-rss-mib 6144 --output out/ground-six
python3 research/profile_capture.py --rounds 11 --timeout 40 --max-rss-mib 6144 --sample-delay 12 --output out/ground-eleven
cargo test --manifest-path egglog/Cargo.toml --test ground_import
cargo test --release --test native_history --test native_single_process --test row_effect -- --include-ignored
```
