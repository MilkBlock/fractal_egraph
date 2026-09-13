# 主干 + 增长旁支：有限候选实验

这不是任意参数化 DAG 的通用归纳器，也不生成可直接执行的 CouplingRule。
读取真实 tier-0 trace 产生并经 tier-1 校验的记录，沿 maximal FractalComb 主干分析直接旁支，尝试一个明确候选语法：

```
位置域：  0 <= j <= d
新位置：  j = d
继承：    (d-1,j) -> (d,j)
```

坐标由出生点及实际 predecessor 边建立，不按 event ID、全局执行轮次或兄弟执行先后编号。多个出生点、跳层、多父继承、外部依赖均明确报告；不会为了填满一个三角形而补造节点。没有直接 Inserted 行的旁支另列为 unclassified，不冒充已提交事实。

## 如何验证 binding

模板同时比较消费规则、源码、输入角色、relative routes、输入/输出别名、行写入及 union 的别名结构。坐标相同但模板不同，后续层会出现 unseen recipe。

命名变量在实际父输出中存在多个完全相同 typed token 时，优先使用旁支的唯一 RHS 构造器列作为稳定接口；这解决了“同一个 origin 值在早期恰好也能从主干取得”的表示漂移。保留原始路线和选定来源，绝不按值相似度更换物理 read 的 producer。RHS 有重复同名构造器时保留原来的完整来源角色，不擅自合并。

前 3 层建立模板字典，后续层只检验。`held_out_fit_with_boundary` 表示映射成功的子族通过有限检验，仍有明示的外部根或边界记录未纳入；不表示全图覆盖。缺位置时的 `observed_domain_mismatch` 可能来自尚未完成的调度，不证明未来不会出现该位置。

这是开发样例上的结构/接口回归与后续层检查，不是独立统计泛化评估，也没有证明 trigger invariant、无限外推、重排等价或压缩比。

## 使用

默认关闭，避免正常分析额外生成大量诊断信息。开启后仍是一个 Rust 进程，分析结束后输出 `dependency_growth.json` 和可点击坐标的 `dependency_growth.html`。

```sh
cargo build --release
EGG_LAYOUT_DISCOVER_GROWTH=1 cargo run --release -- analyze --recapture-tier0 \
  --source experiments/dependency_growth/growing.egg --save-history --output out/growth

EGG_LAYOUT_DISCOVER_GROWTH=1 cargo run --release -- analyze \
  --replay-history out/growth/history.json --output out/growth-replay

EGG_LAYOUT_DISCOVER_GROWTH=1 cargo run --release -- analyze --recapture-tier0 \
  --source egglog/tests/math-microbenchmark.egg --rounds 6 --output out/math-growth
```

## 结果

- `growing.egg` 在实际 egglog 中运行：grow-a 建立主干；birth-b 为每层创建新位置；carry-b 继承旧位置。**分析器没有按这些规则名分支**。
- 7 层映射出 28 个位置，前 3 层得到 3 个 binding/effect 模板，后 4 层无新模板；7 个来自主干范围外出生点的分支记录明确保留为边界。
- 候选域分别覆盖 1、2、3、4、5、6、7 个已观察位置。模板区分出生、首次继承的别名情况和后续继承。
- 原 `.egg` 删除后历史重放相同；交换 birth/carry 规则的声明顺序后，模板、位置域与结论相同。
- Math 6 轮：4 条 maximal 主干、23 个直接旁支族、5 条 unclassified 记录，**0 个族通过当前增长语法的有限检验**。结果见 [results.json](results.json)；不能解释成 Math 不存在更广义的规律。

单位测试另含同样节点数但 predecessor 错误、后续 binding/effect 模板改变、缺失位置及多个不可区分出生点。

```sh
cargo test --release --lib --test dependency_growth
```

完整旁支记录保留实际 fact 身份，没有替换或删除 tier-0/tier-1 数据，也没有将候选作为新 rewrite 反馈到引擎。
