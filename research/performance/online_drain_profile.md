# 在线消费之后的性能定位

测量实际 egglog runtime，release 编译完成后运行，无历史 JSON 输出。使用 macOS `sample` 与 `ps`，结果包含采样开销；单次运行，不是严格的统计对照。

```sh
cargo build --release
python3 research/profile_capture.py --rounds 6 --timeout 30 --max-rss-mib 6144 --output out/profile-six
python3 research/profile_capture.py --rounds 11 --timeout 40 --max-rss-mib 6144 --sample-delay 12 --output out/profile-eleven
```

详细数据：[online_drain_profile.json](online_drain_profile.json)。`EGG_LAYOUT_PROFILE=1` 输出累计规则耗时及关系行数；profile_capture 自动设置它。规则匹配数是逻辑 match，不是已提交 mutation 数。

## 6 轮

程序内总耗时 5.860 s，采样 RSS 峰值 311.1 MiB，结果与改动前视图完全一致（1529 个有效 apply，13 个 HigherRule）。分阶段累计：

| 阶段 | 秒 |
|---|---:|
| tier-0 各轮运行 | 0.0076 |
| 原生批次解析、释放 | 0.0097 |
| tier-1 导入 | 0.8509 |
| tier-1 饱和推理 | 4.7923 |
| tier-1 验证 | 0.0031 |
| tier-2 | 0.1492 |
| lowering / view | 0.0145 |

阶段计时不覆盖全部启动、输出和元数据成本，不能直接把阶段和当成总耗时。

最热规则来自 `experiments/tier1_effects/tier1_rule_comb_ir.egg:123`：

```egg
(rule ((NeedEffect i (HasFact name wanted))
       (Provides i (HasFact name actual)))
      ((NeedArgs i actual wanted)) :ruleset tier1)
```

原生 RunReport 记录它累计 search/apply 3.961 s，约占 tier-1 饱和墙钟时间 82.6%。其他热点各约 0.056 s。最终 `NeedArgs` 47748 行、`Provides` 28173 行，而 `Binding` 1529 行、`Satisfies` 2400 行、`EqAt` 0 行。

此规则只以实例和 fact 名称约束候选。导入器把读事实统一写为 `HasFact("read-row", [token])`，同一实例的不同 read-row 因而也进入配对，再通过 ArgsEqual 排除。父链继续复制 Provides，增加这些候选。调用栈显示时间在 `run_join_stages` / `run_plan` / `ColumnIndex::build_for_subset`，支持“候选 join 与索引构建昂贵”的定位；不能把行数当成字节分配结果。

## 11 轮有界测试

40.315 s 后按预设超时停止，峰值 3317.4 MiB，**没有跑完 11 轮**。第 7 轮完成，第 8 轮停在 tier-1 饱和；在线阶段的旧 run.json `phase=tier0` 标签不能用于归因，应看逐轮日志。

| 完成轮次 | 累计有效 apply | 该轮 tier-1 导入 | 该轮 tier-1 饱和 |
|---|---:|---:|---:|
| 5 | 636 | 0.183 s | 0.673 s |
| 6 | 1529 | 0.460 s | 3.669 s |
| 7 | 3723 | 1.108 s | 21.967 s |
| 8 | 8535 | 2.714 s | 超时前尚未完成 |

第 7 轮 tier-0 运行仅 0.0054 s、collector 0.0122 s。延迟 12 秒取得的活跃 worker 样本全部落在 tier-1 join 路径。当前缩放问题主要在元图的 support 查询，不是原始 trace 跨轮堆积。RSS 是进程采样值，尚未做堆分配归因；不声称 3.2 GiB 都由某一个关系占用。

## 建议的下一步（未实施）

1. 对已知精确 producer 的 read-row 使用直接 fact/token 匹配，避免同名事实先两两配对。涉及等价值的通用 HasFact 查询仍保留独立后备路径，不删除 union 语义。
2. 将祖先 Provides 的全面传播改成按 NeedEffect 查询证明，减少不相关事实传播及中间候选。
3. 先用当前规则作基准检查 SupportsUse、Binding、combined rule 以及 union 反例，再评估速度；不以跳过验证或少生成正确结果制造加速。
