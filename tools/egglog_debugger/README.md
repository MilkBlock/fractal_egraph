# egglog-demo 原生调试桥

在 `egg_layout` 根目录启动（需要 Python 3.10+、Node.js、Rust、`typst`、Graphviz `dot`，以及已安装的 Eggplant Pattern Preview 插件）：

```sh
python3 tools/egglog_debugger/server.py --port 8080
```

打开 `http://127.0.0.1:8080`。默认使用相邻的 `../egglog-demo`；可通过
`--demo PATH` 指定。服务会构建本地 release 二进制，优先提供 demo 的
`static/` 源文件，WASM 等构建产物从该项目现有的 `dist/` 提供。
如果还没有 demo 构建产物，先在 demo 目录执行 `make`。

- 点击编辑器中 `rule` / `rewrite` 的任一行，预览整个规则的 Typst 公式或 DOT。
  使用原生 AST 源位置，支持多行、Unicode 和未加 `@pattern` 注释的规则。
  预览直接复用 VS Code 插件的 `.egg` 转译、注解处理、extractor、MathView、
  Typst、DOT 和 vendored Graphviz。DOT 节点内的 Typst 替换也使用插件的 webview 函数。
  `@egg-viz-json` 中的数学模板、precedence、binding/position 显示名称由插件处理。
  预览下方有三个可折叠面板：**Typst / DOT 源码**、**绑定、effect 与路径证据**、
  以及 **Rust 源码** —— 后者显示该规则实际交给 extractor 的 `add_rule(...)` 作用域
  （含 pattern、约束和 action），可直接核对公式是怎么从 Rust 生成的。
- 点击 **运行并识别** 执行本目录 patched egglog 的实际运行时。
  可先粘贴 `experiments/bake/increment-3.egg`：6 个有效应用、5 个组合、4 条 Fractal 证据。
- 日志窗口按有效应用、Rule Compose、Fractal 过滤；点击每一行读取该事件的公式快照、
  binding、effects、父应用和路径证据。事件 ID 是实际 trace ID，不是连续的数组下标。
- **导出日志 / 回放日志** 保存和载入源码及公式；更改编辑器不会改写旧快照。
  **载入日志源码** 恢复运行时的编辑器内容。预览窗口可滚动，公式保持原始大小。
- **停止** 会取消请求并终止对应本地进程；默认每次运行限时 120 秒，
  可用 `--run-timeout SECONDS` 修改。失败和取消后保留已收到的部分日志。
- 原 demo 的 **Run** 仍运行上游 WASM；**运行并识别** 才使用本地 instrumented runtime。
  不把 WASM 的输出当成本地 trace 的结果。

## 语义与边界

这测量的是 **实际 egglog runtime**，不是独立匹配模拟器。collector 只向分析层导入有
实际 effect 证据、且具备完整物理见证的应用；`logical_matches` 独立显示，不能理解为
提交的 mutation 数量。一个 application 也不等同于一个 mutation。

tier-1、tier-2 保留同一 EGraph 和导入游标，只导入新应用和新 extension，
在每个简单 `run` 轮次后重新饱和增量关系，并在下一轮前发送 JSONL。
复杂 schedule 维持其原始语义，当前在采集结束时交付数据。
重复的稳定相对 binding 路径必须得到原生 `FractalComb / Represents` 的见证。
这些是有限已观察路径，不是任意次数递推的证明。

继承主分析器的输入范围：执行分析要求单个显式、自包含 `Math` datatype，暂不接受
`include`。源码预览没有该 datatype 限制，但不支持 subsuming rewrite。
组合能够合法降级时显示单条 combined rule；否则通过步骤选择器逐步显示原规则的插件公式，
在证据面板保留完整分阶段依赖 DAG、绑定和中间效果，显示不能扁平化的原因。不会把它误报成一条可执行的等价 rewrite。

规则 head 里的 relation / constructor insert 会作为 conclusion 显示（extractor 提交
`ea8528b`）。此前没有 `semantic_text` 的 unbound effect 会被丢掉，于是
`(leq e1 e2)`、`(non-zero e)` 这类规则错误地显示 `no conclusion`；`set_*` 仍走
semantic text 路径。关系名按插件默认渲染成大写变体（`leq` → `Leq`），加 `dsl_type`
模板即可控制显示。

relation / function 匹配本身也是 premise：`(leq e1a e2a)` 这类查询没有 `Pat::new`
根，以前不会进入公式，递归 `leq` 规则会丢掉一半前提；现在“没有任何节点消费”的
非叶子查询节点都会作为独立 premise 列出（extractor 提交 `bb82f91`）。

变量名按 Typst 规则渲染：`e1` 这类“字母 + 数字”写成下标 `e_1`，其余标识符（如
`e1a`、`e1b`）写成 `upright("e1a")`。此前 `e1a` 会裸输出，Typst 报
`unknown variable: e1a`，整个公式降级为文本（extractor 提交 `16861bf`）。

已知仍会降级的边界：约束（side condition）的 `semantic_text` 为空时会回退到
提取器的 Rust 表达式，若其中包含 `prim_call::<…>`、`bigrat(…)` 这类片段，
Typst 仍可能编译失败（例如 `03-analysis.egg` 里带 `upper-bound` / `lower-bound`
约束的规则）。这类规则显示文本 fallback 与原始 Rust，但仍可从 Rust 源码面板
核对实际表达式。

Fractal 的 `Depth / context / event` 证据单独列出；它不是插件支持的一种源规则，不伪造新的 MathView。步骤选择器展示这条实际路径上各规则的插件公式。

日志格式 v2 保存已查看的插件渲染结果（公式源码、SVG、DOT、节点公式、配置和渲染器指纹），
导入后相同配置直接使用这些快照，不会随编辑器内容或插件版本变化而重画。
尚未查看的配置按日志保存的源码惰性渲染；旧 v1 日志也可从源码重新生成插件视图。
回放不是重新执行 tier-0 的原始 trace。

默认选择本机 `~/.vscode/extensions/` 中已安装的 Eggplant 插件；没有安装时才使用
相邻源码目录的已编译扩展。支持显式指定，与 VS Code 的 extractor override 对齐：

```sh
python3 tools/egglog_debugger/server.py --plugin /path/to/eggplant-pattern-vscode \
  --extractor /path/to/eggplant-pattern-extractor
```

DOT 支持 `pattern / action / combined`、`compact / full / recursive` 和
`tree-safe / dag-expand`，默认配置与插件的 `.egg` 规则预览一致。
没有文件路径的内存编辑器不添加插件的 Git/file graph 元数据。
插件渲染失败时会显示错误，不回退到之前的简化箭头公式或另一套 DOT。
CLI 保留原生诊断表达式，网页渲染不使用那些表达式。CLI 可独立使用：

```sh
cargo run -- debug-patterns tests/fixtures/cli_math.egg
cargo run -- debug-stream experiments/bake/increment-3.egg
cargo test --test native_debug --test native_history --test native_single_process --test fractal_comb
```

浏览器回归（安装 Python `playwright` 及 Chromium 后）：

```sh
python3 tools/egglog_debugger/test_browser.py --url http://127.0.0.1:8080
python3 tools/egglog_debugger/test_word_edit.py --url http://127.0.0.1:8080
python3 tools/egglog_debugger/test_template_edit.py --url http://127.0.0.1:8080
node tools/egglog_debugger/test_plugin_render.cjs /path/to/installed/eggplant-pattern-vscode
```

本地 HTTP 服务只监听 loopback。代码、日志和公式通过同源 API 处理；不依赖外部公式渲染服务。
原 demo 页面使用的第三方前端 CDN 仍需联网。

一致性回归直接调用插件作为对照：同一带中文注释、数学模板和显示名称的 `.egg`，
在四组 view/label/recursive 配置下比较 PatternIr、公式源码、Typst 文档及 SVG、
DOT 及 Graphviz SVG、节点 Typst 输出，要求逐项完全相等。
插件的提取器另有 `rewrite_alias` 回归，覆盖转译器生成的 `let result = pat.x` 别名。

## Typst 名称编辑

预览使用 Typst 自己的 `metadata` 测量词语区域，所以点击坐标与公式 SVG
保持一致。构造器名称（例如 `Add` 或其模板中的 `+`）以及显式 `rule` 的
binding/position 显示名称会出现透明高亮区域。点击后输入新名称并保存，服务
只更新对应的 `dsl_type` / `labels` 注释；代码 token、规则语义和运行日志不变。
源码发生并发修改时补丁会拒绝写入，避免覆盖编辑器内容。历史日志只读；下载的
`egglog-preview.egg` 可作为新的 demo 输入。

生成并验证 demo bundle：

```sh
cd ../egglog-demo
python3 examples.py --input static/examples.json > /tmp/examples.annotated.json
python3 -m unittest discover -s . -p 'test_preview_annotations.py'
```

bundle 里的 `@egg-viz-json` 注释不记录 matches、effects 或性能统计；它们是
纯显示元数据。`examples.py` 每次从上游源码重新生成注释，因此不会把当前编辑器
修改写回上游 clone。

点击构造器名称时，编辑框还会列出模板字段，可将其改成 `left/right`、`lhs/rhs`
等标识符。保存会同步更新 `fields` 及对应占位符；只改字段时保留原模板形式和
优先级，拒绝重名、非法标识符或改变字段数量。映射采用插件已有的 `typst_fields`
协议（插件提交 `55ee943`），按字段声明顺序绑定参数，独立于占位符的出现顺序。
本机已同步安装该协议的注释模块及兼容当前 extractor 的实现。

同一个编辑框可以直接编辑完整的 Typst 模板正文（例如把
`upright("Add")({left}, {right})` 改成 `frac({left}, {right})` 或
`{left} + {right}`），并调整优先级。保存前的校验会拒绝：显示名称重名、
未声明的 `{字段}` 占位符、花括号不配对，以及 **用插件自己的 Typst 指令和数学文档
包装实际编译失败** 的模板（`plugin-renderer.cjs --validate-template`）。失败时
原 `.egg` 和原公式都不变，编辑框保持打开，可直接改到通过为止。

能自动修正的失败不会直接报错：例如 `Mul {{ {left} dot {right} }}` 里的裸名称会被
改写成 `upright("Mul")`，修正后的模板填回编辑框、按钮变为“确认修正并保存”，但
**不会写入 `.egg`**；用户再按一次提交确认后才落盘。无法自动修正的错误（括号不配对、
未声明字段、重名等）仍然直接拒绝并显示原因。自动修正只做 `unknown variable` →
`upright("名称")` 这一种确定性改写，最多迭代 8 处，且忽略占位符、转义花括号和
引号内的文本。

“显示名称”只是公式里的标签文字。把整条公式（含 `{}` 占位符）误输进显示名称时，
会被当作模板处理而不是包成 `upright("{left} + {right}")`；打开旧版本已经这样包错的
注释时，模板框会自动还原成原公式，保存即可修正。服务端同样拒绝在“名称”位置写花括号。

编辑框旁的圆形 `i` 按钮会打开一个悬浮面板，列出常用数学写法
（分式、根式、幂、求和、积分、比较、`arrow.r.double`、希腊字母等）及其模板；
点击某一项会插入到模板正文的光标处。面板同时说明模板语法：`{字段}` 是占位符，
`{{` / `}}` 是字面花括号（分组），而 **裸的多字母名称不是合法 Typst 数学** ——
`Mul { {left} dot {right} }` 会被读成 `M·u·l` 三个变量并报
`unknown variable: Mul`，必须写成 `upright("Mul")` 或 `op("Mul")`，
例如 `upright("Mul") {{ {left} dot {right} }}`。校验失败时也会给出同样的中文提示。

规则里的**变量名也能改**，不只是构造器和模板。公式默认用转译器的 Rust 名字
（`num_node2`、`num_node2.arg_i64_00`），debugger 把这些名字作为可点击目标，
同时用目标列表里的 `变量 a · num_node2` 打开编辑框；保存写入该规则的
`labels.bindings`，公式立即改用新名字。显式 `rule` 和 `rewrite` 都支持；
`birewrite` 由转译器直接交给 `parse_and_run_program`，不产生 `add_rule` 作用域，
所以既不提供变量改名，也不占用 ordinal。ordinal 只统计会生成 `add_rule` 的
`rule` / `rewrite`：以前把 `birewrite` 也算进去，导致它后面每一条规则都错位到
前一条的 Rust 作用域（插件提交 `b7739b5`）。渲染时按 ordinal 索引
规则，并为 `rewrite` 补出与 `rule` 等价的结构交给插件自己的绑定重命名器，
因此带 `rewrite` 的文件不再把注解错配到后面的规则上。

绑定到基本类型字段的变量（如 `(Num a)` 里的 `a` 是 `Num` 的 `i64` 字段）保留
**节点限定的访问器**，只把字段部分改成绑定它的模式变量名：`num_node2.arg_i64_00`
显示为 `num_node2.a`，把 `num_node2` 命名为 `m` 后就是 `m.a`。这样同一个节点的
多个字段仍然可区分 —— `(Coord i64 i64)` 的两个字段是 `m.p` / `m.q`（或按声明
`m.x` / `m.y`），而不是都被压成 `m`。公式默认在提取器里生成，早于 `display_names`
存在；插件 `eggPreviewSource` 现在对 `ir.math_view` 应用同一套 `upright("…")`
替换（插件提交 `0aea01e`、`dca7cf8`），DOT 标签与公式因此一致。安装的插件若缺少
该修复，公式会退回 `m.arg_i64_00` 这类 Rust 访问器。

多字段节点上，每个字段各自是一个点击目标（`m.p`、`m.q` 各对应一条
`labels.bindings` 记录）；编辑框里填的是该变量的名字，填好后节点按它命名，
字段后缀保持变量名不变。

公式下方始终列出当前规则的可编辑目标（构造器 / 变量 / 规则条件）。命中区域
依赖插件把公式渲染成数学模式；当模板不再包含名称、或插件的公式降级为文本
（例如当前插件对带附加条件的规则）时，文字高亮区域会消失。目标列表让这些
情况仍然可以再次编辑，修复了“改过一次后面就没法改了”。

点击公式末尾 `if` 之后的区域会打开规则条件编辑框，保存的是真实匹配条件
（`rule` 的 body 附加事实或 `rewrite` 的 `:when`），写回前先用本目录原生
`debug-patterns` 解析整份源码，语法错误会被拒绝。日志回放只读。

