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

Fractal 的 `Depth / context / event` 证据单独列出；它不是插件支持的一种源规则，
不伪造新的 MathView。步骤选择器展示这条实际路径上各规则的插件公式。

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
