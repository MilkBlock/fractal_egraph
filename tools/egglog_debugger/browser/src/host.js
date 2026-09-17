// Browser host for the shared preview core in ../plugin-renderer.cjs.
//
// The bridge spawns a process for the Eggplant transpiler, the pattern
// extractor and the Typst compiler, and requires the extension's Node-only
// Graphviz wrapper. Here each of those is the same wasm module the extension
// already ships, loaded with fetch(): no second renderer implementation.
//
// Everything host independent (the DOT/Typst sources, the formula model, the
// template validation) stays in ../plugin-renderer.cjs and the plugin's own
// `out/*.js`, exactly as the bridge uses them.

import initTranspiler, { transpile_egg_to_eggplant } from "@transpiler-wasm/eggplant_transpiler_wasm_wrapper.js";
import initExtractor, { extract_pattern_json } from "@extractor-wasm/eggplant_pattern_extractor.js";
import { instance as vizInstance } from "@viz-js/viz";
import { $typst, loadFonts } from "@myriaddreamin/typst.ts";
import { renderTypstSnippetsWithRenderer } from "@plugin/shared/typstCore.js";
import { collectEditableRegions, planEditableRegions } from "../../typst-edit.cjs";

const HITBOX_PATH = "/egg-hitboxes.typ";

export function createBrowserHost({ assetBase, modules, revision }) {
    const asset = name => new URL(name, assetBase).href;
    const wasm = {
        transpiler: asset("transpiler-wasm/eggplant_transpiler_wasm_wrapper_bg.wasm"),
        extractor: asset("extractor-wasm/eggplant_pattern_extractor_bg.wasm"),
        renderer: asset("typst/typst_ts_renderer_bg.wasm"),
        compiler: asset("typst/typst_ts_web_compiler_bg.wasm"),
    };

    let initializePromise = null;
    function initialize() {
        if (!initializePromise) initializePromise = (async () => {
            await initTranspiler({ module_or_path: wasm.transpiler });
            // The extractor crate reads Rust source, so it takes its offset in
            // bytes; the shared core passes the same UTF-8 byte offset the CLI does.
            await initExtractor({ module_or_path: wasm.extractor });
            // Point both wasm halves of typst.ts at this page's assets instead
            // of the default `import.meta.url` sibling lookup.
            $typst.setCompilerInitOptions({
                getModule: () => wasm.compiler,
                // typst.ts otherwise downloads its text fonts from a CDN. The
                // debugger renders offline from the same pinned font set, which
                // also keeps the measured formula sizes stable between builds.
                beforeBuild: [loadFonts([], { assets: ["text"], assetUrlPrefix: asset("fonts/") })]
            });
            $typst.setRendererInitOptions({ getModule: () => wasm.renderer });
        })();
        return initializePromise;
    }

    async function compileTypst(document) {
        await initialize();
        // The renderer otherwise appends a `<script>` (and selection CSS) to the
        // SVG. That markup is not XML well formed, and the debugger imports the
        // SVG with DOMParser; the graph only needs the drawn document.
        return $typst.svg({
            mainContent: document,
            data_selection: { body: true, defs: true, css: false, js: false }
        });
    }

    async function queryTypst(document, selector, field) {
        await initialize();
        await $typst.addSource(HITBOX_PATH, document);
        return $typst.query({ mainFilePath: HITBOX_PATH, selector, field });
    }

    let vizPromise = null;
    async function dotToSvg(dot) {
        if (!vizPromise) vizPromise = vizInstance();
        const viz = await vizPromise;
        return viz.renderString(dot, { format: "svg", engine: "dot" });
    }

    // The plugin's own module for this name is Node only (child_process); the
    // wasm compiler is substituted under the same interface.
    const typstModule = {
        async renderTypstSnippets(sources) {
            await initialize();
            const cache = new Map();
            return renderTypstSnippetsWithRenderer(
                sources,
                { render: compileTypst },
                cache,
                error => console.warn("Typst 渲染失败，使用文本 fallback：", error)
            );
        }
    };

    const loadable = {
        ...modules,
        typst: typstModule,
        svg: { dotToSvg }
    };

    return {
        revision,
        hasEggPreviewSource: true,
        load(name) {
            const module = loadable[name];
            if (!module) throw new Error(`浏览器渲染器没有模块 ${name}`);
            return module;
        },
        transpiler: {
            async transpileEggSource(source) {
                await initialize();
                return transpile_egg_to_eggplant(source);
            }
        },
        async extract(rustSource, rustOffset) {
            await initialize();
            const bytes = new TextEncoder().encode(rustSource);
            const clamped = Math.max(0, Math.min(rustOffset, bytes.length));
            return JSON.parse(extract_pattern_json(rustSource, clamped, "2024"));
        },
        compileTypst,
        async editableRegions(source, targets, buildDocument) {
            const plan = planEditableRegions(source, targets, buildDocument);
            if (!plan) return [];
            const rows = await queryTypst(plan.document, "metadata", "value");
            return collectEditableRegions(rows, targets);
        }
    };
}
