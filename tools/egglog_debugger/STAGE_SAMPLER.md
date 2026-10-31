# Stage 局部观察器

从真实 egraph 中随机选一个可见 eclass，沿参数接线显示有限局部；被裁掉的子图显示为 `RipenInput`。同一等价类的多条引用共享同一个输入，保留参数位置、共享结构和回边。折叠只影响显示，不改写执行中的 egraph，也不自动认证该局部是饱和的 RipSatCell。

## 使用

在仓库根目录运行：

```sh
python3 tools/egglog_debugger/server.py --port 8080 --sampler-only
```

打开 <http://127.0.0.1:8080/stage-sampler.html>。完整的原有 debugger 服务也支持这个地址，Cell viewer 工具栏增加了入口，原有图不变。`--sampler-only` 不构建旧 debugger，不依赖 Typst 或 VS Code 插件；需要本机 Rust/Cargo、Python 和 Graphviz `dot`。采集构建使用 `cargo --offline`，所需依赖须已在本地。

1. 选择 Math 微子集、多候选 merge，或冻结的 eggcc Pass 3 样例；也可输入仓库内自己的 `.egg` 路径。
2. 点击「记录 Stage」。采集执行真实 CPU staged engine，每完成一次实际 Run 就可查看；无需等整个程序完成。可以停止采集并查看已经完整记录的阶段。
3. 选择 ruleset 和实际执行位置，在「阶段入口／出口」间切换。同一 ruleset 的 repeat、嵌套 saturate 迭代分别记录，不混为一种阶段。
4. 调整深度、事实数、每个 eclass 的候选数，点击「换样本」。种子和 URL 可复现选中的局部；固定观察根后可逐阶段追踪它。
5. 点击 RipenInput 展开；点击 eclass 后可折叠或设为观察根；双击 eclass 可直接换根。root 和非 root 位置都可以抽到。
6. 「阶段规则」展示实际规则源码和该阶段完整匹配计数。可下载当前小图 DOT，图按竖向生长，同一 eclass 的等价节点收拢在同一个紧凑框内；参数箭头指向类边界，不指定某个等价节点作为唯一代表。

默认按「本阶段读取类型」抽样。这只筛选规则会读取的表类型，不声称局部满足某条完整 LHS。也可以选择「本阶段变化」或全图。随机性是对符合筛选条件的不同可见 canonical eclass 均匀抽样，不是对所有可能子图均匀抽样。固定的根在某阶段尚不存在时，页面明确显示不存在，不偷偷换成另一个根。

## 筛选样本

点击图上方「筛选」，填好条件后点「应用并抽样」。输入留空表示不限；选快捷条件只填写表单，不立即执行。可组合：

- 阶段变化：新增、函数／可见性更新、移除、union、任意变化或完全未变化。移除从阶段入口看，出口已经没有该事实；查看被 subsume 的事实须打开对应显示选项。
- 展示局部包含／排除哪些 constructor、function 或 relation（准确表名，逗号分隔，含输入提示）。包含支持全部／任意。
- 根的原图返回记录包含哪些类型；已展开的某个 eclass 实际包含至少多少个 constructor enode。
- 局部某个 eclass 被至少多少条不同事实引用；同一事实的多个参数重复引用；RipenInput 被多处引用。同一事实的两个参数只计一条引用事实。
- 展示事实、展开 eclass、RipenInput 的最少／最多数量。上限可以填 0；上限留空才是不限。

例如 `阶段有新增 + 全部包含 Add,Mul + 某类原图 enode ≥3 + 不同引用事实 ≥2 + 展示事实 5–10`：Math 的 distribute 阶段在深度 2、每类最多 3 个等价节点、不带上游时，从 14 个根筛出 3 个。

图上方显示「符合条件 / 初始候选」，展开后可以看每个条件依次剩下多少候选、哪一项首次排空，以及当前图的具体依据。点击 eclass、共享输入或重复参数的依据可定位图中对象。条件按展示顺序叠加；“某项排空”不是说这项单独就无解。

包含／排除、共享和规模只检查当前裁剪范围，**排除 Add 不代表整个被折叠子图里不存在 Add**。原图分支数检查的是已展开类的全部可见 constructor，即使显示上限只画出了其中一部分；详情同时报告实际数和展示数。

抽样池采用基础裁剪设置，忽略个别根的手工展开／折叠。这些手工操作只改变当前图的指标，避免“换一个样本”时继承另一个根的特殊裁剪。固定根或手工操作使当前图不符合条件时，保留图并显著提示，不悄悄换根。点击「应用并抽样」或「换样本」重新从符合条件的根中选择。URL 保存已应用条件、ruleset、种子、根与显示设置；未应用的表单草稿不保存。

筛选先共享一份表／邻接索引，再对每个候选生成受显示上限约束的局部结构，只为最终样本运行一次 Graphviz。不使用随机试探次数上限，因此候选计数是精确的。服务端为同一 Stage、筛选和基础裁剪保留最近一次候选列表；换种子复用它。初次筛选仍随候选数量增长，不是常数时间，且不会把全部候选的小图留在内存中。

## 观察语义

- 入口是该次 Run 真正开始前的已提交状态；出口是原生执行及 rebuild 完成后的状态。跳过的 `until` Run 不计为一次实际执行。
- 绿色表示本阶段新增，紫色表示函数值／可见性更新，入口视图可显示出口已移除的事实；青色轮廓标明本阶段合并了此前的等价类。union 的身份重编号不会被误报为新事实。
- 匹配数是原生逻辑规则匹配数，不是扫描量或实际修改次数。同名规则的统计可能合并，界面不会为每条同名规则重复认领这个计数。
- `RipenInput` 保留类型和共享身份。展开会重新读取同一阶段的局部，不会执行规则。递归回边保留，不能假设 egraph 总是 DAG。
- scalar 按类型显示；大整数保留字符串避免 JavaScript 精度丢失。未支持导出的值明确显示 opaque，不伪造内部结构；container 预览截断，不代表完整展开。
- 当前记录来自实际 **CPU egglog staged runtime**，不是 GPU 快照，也不是重新跑一遍局部 saturation。此工具用于观察阶段计算结构，不作为新 Cell 的闭包证明。

## 数据与成本

每次采集写入 `out/stage-sampler/<id>/`，保留冻结源码、源码／二进制 SHA256、编译程序、日志、任务状态以及 `archive/`。运行下拉菜单复用这些结果，不为每次取样重跑推理。

`manifest.json` 保存 typed core；`frames.ndjson` 保存初始状态及各阶段前后表增量、interned typed value 和 alias；`stages.ndjson` 保存调度位置和实际匹配统计。表版本及 live row count 未变时跳过扫描。原生执行可单独使用 `--mode staged --plan PLAN.json --stage-archive DIR` 开启记录；默认推理不启用。

浏览器只收到有大小限制的小图。服务端重放当前全图状态后裁剪，最多缓存两个 archive；记录时仍需扫描变化表，向前／向后浏览也有回放成本。此机制避免每轮保存完整 JSON 图，但不能声称其内存或采集成本与局部图大小相同。大 Raytrace 的采集开销本轮未测，UI 保留取消操作。

## 验证（2026-10-31）

```sh
python3 -m unittest discover -s tools/egglog_debugger -p 'test_stage_sampler.py'
python3 -m unittest discover -s tools/egglog_debugger -p 'test_stream_errors.py'
cargo build --offline --release --manifest-path engine/cpu/Cargo.toml
python3 tools/egglog_debugger/verify_stage_archive.py --output out/stage-sampler-check
```

输出目录必须为新目录。14 项 sampler 单元测试（含组合筛选、裁剪口径、共享引用、union、固定根、候选缓存和种子复现）及既有 streaming 错误处理回归通过。5 个原生程序同时核对 staged 最终图、archive 回放图与 native 最终图，包含事实、函数值、等价关系及 subsume 状态：

| 程序 | 实际 Stage 数 | 最终行数 | archive 字节数 | 原生整图一致 |
|---|---:|---:|---:|---|
| Math | 9 | 31 | 11,561 | 是 |
| 自定义 merge／Smaller | 5 | 13 | 11,030 | 是 |
| 阶段可见性 | 2 | 3 | 3,473 | 是 |
| Set 内 union／alias | 1 | 3 | 2,765 | 是 |
| subsume／delete | 3 | 1 | 3,770 | 是 |

另完成真实 `fact` Pass 3 采集，archive 为 1,554,878 字节。这些是观察器正确性验证及产物大小，不是推理加速比。

浏览器验收覆盖：实际采集、阶段选择、固定根前后对比、RipenInput 展开、手工折叠、规则源码、桌面及窄屏布局。Math distribute 阶段固定同一根，入口显示 5 条事实，出口显示 8 条；折叠嵌套 Add 后入口降至 3 条，展开恢复。浏览器无控制台错误。
