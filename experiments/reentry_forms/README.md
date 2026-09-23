# 增长中的 CS 先入桶：DABC 与多叉回归

本次修复的是入桶条件，不是循环体归纳。

- `dabc.egg`：D 只提供一次 A，随后 A→B→C→新的 A 持续接续。
- `branching.egg`：每个 A 产生 Left 和 Right 两个 B；B→C→A，再次分叉。
- 两个程序都没有 length 衰减。tier0 仅 `(run 1)`，后续增长发生在 ripen 内。

```sh
cargo run --release --offline -- analyze \
  --source experiments/reentry_forms/dabc.egg --output out/dabc-new \
  --ripen-observe-facts 11
cargo run --release --offline -- analyze \
  --source experiments/reentry_forms/branching.egg --output out/tree-new \
  --ripen-observe-facts 18
```

| 情况 | 起始 fact 数 | 停下观察时 | D 的新 binding | 桶 |
|---|---:|---:|---:|---:|
| DABCABC… | 3 | 11 | 0 | 1 |
| 多叉 | 3 | 19 | 0 | 1 |

多叉保留 Left 3、Right 3、B 6。事实数按 cell 表行计，包含一个 RipenInput 行；阈值在完整 sweep 结束后检查，因此 18 可观察到 19。

两者均为 `ObservedUnfinished`，没有 Sat 共享目录，尚未确定真正的 starting condition。D 没有重现自己的初始 C，仍然正确存入增长观察桶。`result.json` 保存这次实际原生执行的计数与原因。

修复后的流程：达到显式观察阈值→保存未完成状态→若观察到事实增长，先存整个 CS→查询已有桶中 C 在未完成图中的匹配。匹配区分原有位置与新 binding，不冒充已证明的循环。桶内已有的成分比较、直接 coarse fact cover 和多个最小观察条件继续保留；没有恢复逐 apply 生成触发器的替代算法。

还验证了：新注册的 A 对应条件可以在 D 的未完成图中找到；同一 owner 的共享任务会一起标为未完成，不会卡在 WaitingEquivalent；时间阈值也能触发观察。相关测试见 `tests/growth_reentry.rs`。

限制：检查点不抢占单次原生 sweep。未完成图保存在各 cell 的 `unfinished-state.json`，仅作观察；目前没有进程退出后继续同一个原生引擎的恢复功能。没有证明 infi，也没有自动把 D 缩减成 A/B/C。
