#!/usr/bin/env node
// Build the browser preview bundle for the static debugger page.
//
// The page must work with no local bridge, so the four pieces the bridge gets
// from processes or Node-only modules are bundled as wasm:
//
//   Eggplant transpiler  <plugin>/vendor/transpiler-wasm            (extension vendor)
//   pattern extractor    wasm-pack build of the extension's crate   (same crate as the CLI)
//   Graphviz             <plugin>/vendor/viz.cjs                    (same module svg.js requires)
//   Typst                @myriaddreamin/typst.ts + its two .wasm    (web editor dependency)
//
// Everything else is the extension's own compiled `out/*.js`, reached through
// the same host-agnostic `renderPreview` the bridge uses.
import { createHash } from "node:crypto";
import { access, cp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const DEBUGGER = path.dirname(HERE);

function parseArgs(argv) {
    const options = {
        plugin: undefined,
        webdeps: undefined,
        out: undefined,
        extractorWasm: undefined,
        fontCache: undefined,
        esbuild: undefined
    };
    for (let index = 0; index < argv.length; index += 2) {
        const key = argv[index].replace(/^--/, "").replace(/-([a-z])/g, (_, c) => c.toUpperCase());
        if (!(key in options)) throw new Error(`Unknown option ${argv[index]}`);
        options[key] = argv[index + 1];
    }
    for (const required of ["plugin", "webdeps", "out"]) {
        if (!options[required]) throw new Error(`--${required} is required`);
    }
    return options;
}

async function copyFiles(pairs) {
    for (const [from, to] of pairs) {
        await mkdir(path.dirname(to), { recursive: true });
        await cp(from, to, { recursive: false });
    }
}

function buildExtractor(crate, outDir) {
    // rust-analyzer's syntax tree drops itself on a worker thread, which wasm
    // does not have; its own build expects this cfg on wasm targets.
    const result = spawnSync("wasm-pack", ["build", "--release", "--target", "web",
        "--out-dir", outDir, "--out-name", "eggplant_pattern_extractor"],
        { cwd: crate, stdio: "inherit", env: { ...process.env, RUSTFLAGS: "--cfg no_salsa_async_drops" } });
    if (result.status !== 0) throw new Error(`wasm-pack failed in ${crate}`);
}

// typst.ts would otherwise download this font set from jsDelivr on first
// render. Vendoring it keeps the page offline capable and the measured formula
// sizes reproducible; the list is `_textFonts` of @myriaddreamin/typst.ts.
const TYPST_FONT_URL = "https://cdn.jsdelivr.net/gh/typst/typst-assets@v0.13.1/files/fonts/";
const TYPST_TEXT_FONTS = [
    "DejaVuSansMono-Bold.ttf", "DejaVuSansMono-BoldOblique.ttf", "DejaVuSansMono-Oblique.ttf",
    "DejaVuSansMono.ttf", "LibertinusSerif-Bold.otf", "LibertinusSerif-BoldItalic.otf",
    "LibertinusSerif-Italic.otf", "LibertinusSerif-Regular.otf", "LibertinusSerif-Semibold.otf",
    "LibertinusSerif-SemiboldItalic.otf", "NewCM10-Bold.otf", "NewCM10-BoldItalic.otf",
    "NewCM10-Italic.otf", "NewCM10-Regular.otf", "NewCMMath-Bold.otf", "NewCMMath-Book.otf",
    "NewCMMath-Regular.otf"
];

async function download(url, target) {
    try {
        await access(target);
        return;
    } catch { /* not cached yet */ }
    const response = await fetch(url);
    if (!response.ok) throw new Error(`cannot fetch ${url}: ${response.status}`);
    await writeFile(target, Buffer.from(await response.arrayBuffer()));
}

async function fetchFonts(cache) {
    await mkdir(cache, { recursive: true });
    for (const name of TYPST_TEXT_FONTS) {
        await download(TYPST_FONT_URL + name, path.join(cache, name));
    }
    return TYPST_TEXT_FONTS.map(name => path.join(cache, name));
}

async function revisionOf(files) {
    const hash = createHash("sha256");
    for (const file of files.slice().sort()) {
        hash.update(await readFile(file));
    }
    return hash.digest("hex");
}

async function main() {
    const options = parseArgs(process.argv.slice(2));
    const plugin = path.resolve(options.plugin);
    const webdeps = path.resolve(options.webdeps);
    const out = path.resolve(options.out);
    const site = path.join(out, "browser");
    const assets = path.join(site, "assets");
    const require = createRequire(path.join(webdeps, "noop.cjs"));
    const esbuild = options.esbuild ? require(path.resolve(options.esbuild)) : require("esbuild");

    await rm(site, { recursive: true, force: true });

    // 1. The transpiler wasm bundle the extension ships, verbatim.
    const transpilerSource = path.join(plugin, "vendor", "transpiler-wasm");
    const transpilerTarget = path.join(assets, "transpiler-wasm");
    await copyFiles([
        [path.join(transpilerSource, "eggplant_transpiler_wasm_wrapper.js"), path.join(transpilerTarget, "eggplant_transpiler_wasm_wrapper.js")],
        [path.join(transpilerSource, "eggplant_transpiler_wasm_wrapper_bg.wasm"), path.join(transpilerTarget, "eggplant_transpiler_wasm_wrapper_bg.wasm")]
    ]);

    // 2. The pattern extractor: the same crate the extension's CLI binary is
    //    built from, compiled for wasm32 (`extract_pattern_json`).
    const extractorTarget = path.join(assets, "extractor-wasm");
    if (options.extractorWasm) {
        const prebuilt = path.resolve(options.extractorWasm);
        await copyFiles([
            [path.join(prebuilt, "eggplant_pattern_extractor.js"), path.join(extractorTarget, "eggplant_pattern_extractor.js")],
            [path.join(prebuilt, "eggplant_pattern_extractor_bg.wasm"), path.join(extractorTarget, "eggplant_pattern_extractor_bg.wasm")]
        ]);
    } else {
        buildExtractor(path.join(path.dirname(plugin), "eggplant-pattern-extractor"), extractorTarget);
    }

    // 3. The pinned Typst text font set the wasm compiler renders with.
    const fontCache = path.resolve(options.fontCache || path.join(path.dirname(HERE), "..", "..", "target", "typst-fonts"));
    const fonts = await fetchFonts(fontCache);
    await mkdir(path.join(assets, "fonts"), { recursive: true });
    await copyFiles(fonts.map(font => [font, path.join(assets, "fonts", path.basename(font))]));

    // 4. Graphviz: the same @viz-js/viz build the extension vendors, so DOT
    //    renders identically. Fail loudly if the two ever drift apart.
    const vizVersion = JSON.parse(await readFile(path.join(webdeps, "@viz-js/viz/package.json"), "utf8")).version;
    const vendorBanner = (await readFile(path.join(plugin, "vendor/viz.cjs"), "utf8")).slice(0, 200);
    if (!vendorBanner.includes(`Viz.js ${vizVersion}`)) {
        throw new Error(`extension vendor/viz.cjs is not Viz.js ${vizVersion}; browser DOT would differ from the bridge`);
    }

    // 5. Typst: the compiler and renderer wasm the web editor already uses.
    const typstTarget = path.join(assets, "typst");
    await copyFiles([
        [path.join(webdeps, "@myriaddreamin/typst-ts-web-compiler/pkg/typst_ts_web_compiler_bg.wasm"), path.join(typstTarget, "typst_ts_web_compiler_bg.wasm")],
        [path.join(webdeps, "@myriaddreamin/typst-ts-renderer/pkg/typst_ts_renderer_bg.wasm"), path.join(typstTarget, "typst_ts_renderer_bg.wasm")]
    ]);

    // The revision identifies the exact plugin build the page renders with, the
    // same way the bridge reports a hash of the modules it loaded.
    const pluginModules = ["dot", "mathView", "ir", "eggPreviewSource", "eggVizAnnotation",
        "eggRuleMapping", "rustBindingRename", "shared/typstCore"]
        .map(name => path.join(plugin, "out", `${name}.js`));
    const revision = "browser-" + (await revisionOf([
        ...pluginModules,
        // The shared core and this bundle's own sources are part of the renderer
        // too: they own what the plugin's modules do not know about (the fractal
        // repetition badge and the generated lane rule).
        path.join(DEBUGGER, "plugin-renderer.cjs"),
        path.join(DEBUGGER, "typst-edit.cjs"),
        path.join(HERE, "src/entry.js"),
        path.join(HERE, "src/host.js"),
        path.join(HERE, "src/annotations.js"),
        path.join(plugin, "vendor/viz.cjs"),
        path.join(transpilerTarget, "eggplant_transpiler_wasm_wrapper_bg.wasm"),
        path.join(extractorTarget, "eggplant_pattern_extractor_bg.wasm"),
        ...fonts
    ])).slice(0, 32);

    const stub = path.join(HERE, "src", "node-stub.cjs");
    const nodeBuiltins = ["fs", "path", "crypto", "child_process", "os", "url", "module"];
    const alias = {
        "@plugin": path.join(plugin, "out"),
        "@transpiler-wasm": transpilerTarget,
        "@extractor-wasm": extractorTarget,
    };
    for (const name of nodeBuiltins) {
        alias[`node:${name}`] = stub;
        alias[name] = stub;
    }

    await esbuild.build({
        entryPoints: [path.join(HERE, "src", "entry.js")],
        outfile: path.join(site, "preview.js"),
        bundle: true,
        format: "esm",
        platform: "browser",
        target: ["es2022"],
        sourcemap: false,
        nodePaths: [webdeps],
        alias,
        define: { __EGGLOG_BROWSER_REVISION__: JSON.stringify(revision) },
        logLevel: "warning"
    });

    await writeFile(path.join(site, "revision.txt"), revision + "\n");
    const size = (await readFile(path.join(site, "preview.js"))).length;
    process.stdout.write(`${site} (preview.js ${(size / 1_000_000).toFixed(2)} MB, ${revision})\n`);
}

await main();
