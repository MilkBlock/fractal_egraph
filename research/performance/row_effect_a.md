# A：精确行事实匹配

仅实施 A：新增 `Effect::RowFact(Val)`，native importer 将已解析的行版本/外部边界行 token 编码为 RowFact。需要的 effect 与已有 effect 直接按同一个标识 join。通用 HasFact、Equal/EqAt、祖先 Provides 传播与 SupportsUse 查询范围均保留。没有实施 B/C。

## 正确性边界

行 token 是 scope + 已提交 write ID，或者 scope + table + exact boundary row。相同表达式值不意味着同一个行版本。它们是带 Fact sort 的不透明标识；native importer 的 union effect 是 Math 值，不是 Fact 标识。因此旧单元素 read-row Args 比较在这个输入域只能以同一 token 成功，可以由精确 RowFact 匹配替代。通用 HasFact 即使名字也是 read-row，仍保留参数等价推理。

同一份 `out/online-drained6/history.json` 分别用优化前/后重新分析：2366 对 SupportsUse 完全相等；移除原生分配编号后，完整视图、combined rule 文本、source steps、binding routes 全相等。独立源规则重放、在线/离线/历史重放及 RowFact 身份反例通过。

## 实测

实际 egglog release runtime，macOS sample/ps。单次测量，RSS 为采样峰值，不是严格分配上界。数据见 [row_effect_a.json](row_effect_a.json)。

| Math 6 轮 | baseline | A |
|---|---:|---:|
| 程序总耗时 | 5.860 s | 1.149 s |
| tier-1 导入 | 0.851 s | 0.778 s |
| tier-1 饱和 | 4.792 s | 0.128 s |
| RSS 峰值 | 311.1 MiB | 52.3 MiB |
| NeedArgs | 47748 | 0 |
| Provides | 28173 | 28173 |
| Satisfies | 2400 | 2400 |
| SupportsUse | 2366 | 2366 |

11 轮依然没有完成：40.138 秒超时，完成第 9 轮后进入第 10 轮导入，采样 RSS 1391.2 MiB。先前版本同样 40 秒预算只完成第 7 轮并停在第 8 轮饱和。两次进度不同，不能将 RSS 差解释为相同最终图上的内存压缩率。

A 的第 9 轮新增导入 9.111 秒、饱和约 7.68 秒；较大规模下导入和 binding 解析仍需优化。这里不声称已解决 11 轮的性能问题。

## 复现

```sh
cargo build --release
python3 research/profile_capture.py --rounds 6 --timeout 30 --max-rss-mib 6144 --output out/row-a-six
python3 research/profile_capture.py --rounds 11 --timeout 40 --max-rss-mib 6144 --sample-delay 12 --output out/row-a-eleven
cargo test --release --test row_effect --test native_history --test native_single_process -- --include-ignored
```

baseline 算法为 `72d9c81`（本次对照额外加了只读 support 快照观测）；在独立 checkout 编译该版本可运行同一 profile 命令。可选 `EGG_LAYOUT_SUPPORT_SNAPSHOT=/absolute/path/pairs.json` 会输出以原始 apply ID 表示的排序 SupportsUse 集合，用于跨构建比较；默认不输出。
