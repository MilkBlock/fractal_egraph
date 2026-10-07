---
name: eggcc-pass-benchmark
description: 统计 eggcc 各 pass 的执行、search、RHS、insert/merge、rebuild、extraction 时间和 RipSatCell 覆盖率，或用已有结果快速生成表格。适用于 egg_layout/check_statewalk 的分阶段 benchmark，不用于泛泛讨论优化算法。
---

# eggcc 分 Pass 统计

默认仓库 `/Users/mineralsteins/Repos/egg_related/egg_layout`，生产 eggcc 在旁边的 `check_statewalk`。先用本技能的脚本，不重新研究 profiler、LLVM 配置或编写另一套统计器。

## 最短路径

`$SKILL` 表示本 SKILL.md 所在目录；脚本也支持 `--repo`。

- **看已有表／解释结果**：
  `python3 "$SKILL/scripts/bench.py" report`
  默认读已提交的 `experiments/eggcc_passes/results.json`，明确告诉用户这是报告所标日期的保存结果。
- **重新测量**：先运行 `python3 "$SKILL/scripts/bench.py" check`；工具齐全后直接
  `python3 "$SKILL/scripts/bench.py" run`
  默认 fact、loopfact，各完整运行一次三个 pass，不求平均。输出写到新的 out/ 子目录。
- **重绘其他结果**：
  `python3 "$SKILL/scripts/bench.py" report --input out/某次运行/summary.json`

不要因为“重新统计”就重建所有工具、重新 analyze、重新采集 trace 或 bake。覆盖率使用现有冻结库。用户要求重新 bake 或不同样例时，明确扩展现有 runner，而不是把默认两个样例当成不可更改要求。

## 缺依赖才读这里

`check` 会列出缺失项。只在缺工具、构建失败或用户要求最新源码时读 [references/build.md](references/build.md)。默认复用指定版本的缓存工具；存在二进制不等于与当前源码一致。若要求最新版本，核对来源或重编译。

## 必须保持的统计口径

- 真实生产链：前一 pass 做 Tiger extraction，结果传入下一 pass 的新 egraph。不能独立把原始程序跑三次充当三个 pass。
- 计时使用生产 eggcc 的插桩二进制；覆盖率是另一轮本地诊断，不混入计时。
- `execution_ms` 包含解析、类型检查、安装等准备工作；不是纯推理时间。
- `search_and_binding_ms` 是 search+apply 减去 RHS 指令计时，仍含匹配管理。
- `rhs_ms` 包含 primitive、构造器查找、写缓冲；`insert_merge_ms` 是提交插入和 merge 的合计，不能伪造单独 insert 时间。
- 主表用 `extraction_wall_ms`；提取器内部计时和序列化保留在 JSON。不要把序列化算进 search。
- 默认库是旧 fact Pass-1 的 163 个观察模式条目，不是每个 pass 新 bake 的认证 Cell。fact 为训练内，loopfact 为 held-out；Pass 2/3 是旧库迁移覆盖。
- 表里同时显示 Expr 节点、Expr e-class、constructor 行及完整 LHS 覆盖。节点覆盖不等于免 Join。
- 覆盖测试必须 `same_final_state_as_native_control=true`，冻结签名数量不变。保留分母、source/library hash，不把各 pass 的图合成一个分母。

## 交付

给两张表：时间和覆盖率。附报告路径，简短说明引擎版本、库口径与未拆分计时。报告复用不能冒充新测量；不从两个小样例推广到整个 eggcc 工作负载。
