# 浏览器预览包

调试器页面在没有本机 bridge 时用的预览管线。这里**不重写渲染**：页面加载插件自己编译出的
`out/*.js`，并调用与 bridge 完全相同的 `renderPreview`（`../plugin-renderer.cjs`），
只替换 host（子进程 → wasm）。

```
                         ┌── bridge（server.py）── 子进程 / Node 模块 ──┐
egg 源码 → /api/preview ─┤                                              ├→ 同一个 renderPreview
                         └── 浏览器 ── preview.js ── wasm ──────────────┘
```

## 文件

| 文件 | 作用 |
|---|---|
| `src/entry.js` | 打包入口：装配插件模块 + host，导出 `renderPreviewInBrowser` / `validateTemplateInBrowser` |
| `src/host.js` | 四个 wasm 依赖的加载与初始化（transpiler、extractor、Typst、Graphviz），以及 `typst query` 命中区域 |
| `src/annotations.js` | `preview_annotations.py` 的 JS 版本：改写行归属、`birewrite` 前向别名、可编辑目标（`catalog`）、`update_display` / `update_conditions` |
| `src/node-stub.cjs` | 只被 Node 分支引用到的 builtin 占位，保证打包无未解析 require |
| `build.mjs` | 收集 wasm/字体资源 + esbuild 打包 → `<out>/browser/{preview.js,assets/}` |
| `test_browser_render.mjs` | 浏览器 host 与 bridge 的逐项一致性（在 Node 里加载打包结果） |
| `test_annotations_parity.mjs` | 与 `preview_annotations.py` 逐行对比（54 个示例程序） |
| `test_browser_page.mjs` | 起静态服务器 + 死 bridge，验证页面真的用 wasm 渲染公式和 DOT，并把 constructor / binding 改名写回 `.egg` |
| `test_stream_parity.mjs` | native 与 wasm 的 match 历史对比（命中集合/轮次/拒绝信息），并报告顺序差异 |
| `annotation_reference.py` | 上面那个对比用的 Python 侧回答器 |

## 构建

```sh
# 一次性：插件的 extractor crate → wasm（RUSTFLAGS 不能省）
cd ../eggplant_pattern_view_plugin/eggplant-pattern-extractor
RUSTFLAGS='--cfg no_salsa_async_drops' wasm-pack build --release --target web \
  --out-dir ../../egg_layout/target/extractor-wasm --out-name eggplant_pattern_extractor

# 打包（通常由 build_site.py 调用）
node tools/egglog_debugger/browser/build.mjs \
  --plugin ~/.vscode/extensions/milkblock.eggplant-pattern-vscode-* \
  --webdeps dpsk_workspace/viz-web-editor/node_modules \
  --out target/pages
```

`--webdeps` 只需要 `@myriaddreamin/typst.ts`、`@myriaddreamin/typst-ts-*`、`@viz-js/viz`
和 `esbuild`；`@viz-js/viz` 的版本会与插件 `vendor/viz.cjs` 的版本号比对，不一致直接报错。

## match 历史

`test_stream_parity.mjs` 在 62 个程序上断言两边“命中集合 + boundary 分布 + 拒绝信息”一致，
并报告顺序：9 个能出事件的程序里 5 个逐字节一致，4 个命中相同但枚举顺序不同
（`id`/`event`/`0:write:N` 随顺序变）。两端各自可重复。需要权威 event id 时用本机 bridge。

## 与 bridge 的一致性

必须逐字节相同：PatternIr、MathView、公式源码、Typst 文档、DOT 结构（除 Typst 图片尺寸）、
Graphviz 图（节点/边/标签/整体尺寸 3% 内）、可点击目标、模板校验结论。

允许不同：Typst SVG 的序列化与字形度量。bridge 用 `typst` CLI + 系统字体，浏览器用
`typst.ts` wasm + `build.mjs` 固定下来的字体集；同一份 Typst 源码，不同字体后端会带来
约 1–5% 的尺寸偏差，进而改变 Graphviz 的坐标。为减少噪声，浏览器侧关掉了 typst.ts 注入的
`<script>`/选择 CSS（那段 HTML 不是合法 XML，`DOMParser` 会拒绝）。
