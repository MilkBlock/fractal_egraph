#!/usr/bin/env node
// Parity check between the browser (wasm) renderer and the bridge.
//
// Both hosts run the *same* `renderPreview` from ../plugin-renderer.cjs over the
// same extension modules, so the formula sources, the pattern IR, the DOT
// structure and the edit targets must be identical. Two things legitimately
// differ, because a page cannot spawn processes:
//
//   Typst    the bridge runs the `typst` CLI with system fonts, the browser runs
//            the typst.ts wasm compiler with its bundled fonts. The same source
//            is compiled, but glyph metrics (and the SVG serialization) differ
//            slightly, so rendered sizes are compared with a tolerance.
//   Graphviz the same viz.js build renders DOT on both sides, so the graph is
//            identical once the embedded Typst images are blanked out.
//
//   node test_browser_render.mjs <bundle.js> <plugin-root> [extractor]
//
// The bundle is loaded in Node with a file:// fetch shim so the wasm modules
// load without a server; the published page itself is covered by
// test_browser_page.py.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { createRequire } from "node:module";
import { catalog } from "./src/annotations.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);
const { loadPlugin, renderPreview, validateTemplate } = require("../plugin-renderer.cjs");

const realFetch = globalThis.fetch;
globalThis.fetch = (input, init) => {
    const url = typeof input === "string" ? input : input?.url;
    if (typeof url === "string" && url.startsWith("file:")) {
        const body = fs.readFileSync(fileURLToPath(url));
        const type = url.endsWith(".wasm") ? "application/wasm" : "text/plain";
        return Promise.resolve(new Response(body, { headers: { "Content-Type": type } }));
    }
    if (typeof url === "string" && process.env.EGG_TRACE_FETCH) console.error("FETCH", url.slice(0, 120));
    return realFetch(input, init);
};

const dim = (svg, attr) => Number(new RegExp(`${attr}="([0-9.]+)`).exec(svg)?.[1] ?? 0);

// DOT that only differs in the sizes the Typst renderer measured.
function normalizeDot(dot) {
    return dot
        .replace(/image="data:[^"]*"/g, 'image="<typst>"')
        .replace(/\b(width|height)=(\d+(?:\.\d+)?)/g, (_, key, value) => `${key}=${Number(value).toFixed(0)}`);
}

// Graphviz lays the graph out around the embedded Typst images, so the final
// coordinates follow the measured glyph metrics. Compare what the graph says
// (nodes, edges, labels) instead of where it happens to put them.
function svgStructure(svg) {
    const titles = [...svg.matchAll(/<title>([^<]*)<\/title>/g)].map(m => m[1]).sort();
    const labels = [...svg.matchAll(/<text[^>]*>([^<]*)<\/text>/g)].map(m => m[1]).sort();
    const root = /<svg width="([0-9.]+)pt" height="([0-9.]+)pt"/.exec(svg);
    return { titles, labels, width: Number(root?.[1] ?? 0), height: Number(root?.[2] ?? 0) };
}

function near(actual, expected, tolerance) {
    return Math.abs(actual - expected) <= tolerance;
}

async function main() {
    const [bundle, pluginRoot, extractor] = process.argv.slice(2);
    assert.ok(bundle && pluginRoot, "usage: node test_browser_render.mjs <bundle.js> <plugin-root> [extractor]");
    const browser = await import(pathToFileURL(path.resolve(bundle)).href);
    const plugin = loadPlugin(pluginRoot, extractor);
    const source = fs.readFileSync(path.join(path.dirname(HERE), "fixtures/render-parity.egg"), "utf8");
    let checks = 0;

    for (const [mode, style, strategy] of [
        ["combined", "recursive", "dag-expand"],
        ["combined", "recursive", "tree-safe"],
        ["pattern", "full", "dag-expand"],
        ["action", "compact", "dag-expand"],
    ]) {
        // The same editable targets go to both hosts; the annotation module that
        // produces them has its own parity test against preview_annotations.py.
        const request = { source, line: 5, mode, label_style: style, recursive_strategy: strategy,
            edit_targets: catalog(source, 5) };
        const expected = await renderPreview(plugin, request);
        const actual = await browser.renderPreviewInBrowser(request);
        const where = `${mode}/${style}/${strategy}`;
        // Everything the host does not touch must be byte identical.
        for (const key of ["ir", "math_view", "typst", "typst_document", "typst_sources",
            "typst_mode", "variable_map", "variable_labels"]) {
            assert.deepEqual(actual[key], expected[key], `${where}: ${key} differs from the bridge`);
        }
        assert.equal(normalizeDot(actual.dot), normalizeDot(expected.dot), `${where}: dot structure differs from the bridge`);
        const mineSvg = svgStructure(actual.dot_svg), expectedSvg = svgStructure(expected.dot_svg);
        assert.deepEqual(mineSvg.titles, expectedSvg.titles, `${where}: graph nodes/edges differ from the bridge`);
        assert.deepEqual(mineSvg.labels, expectedSvg.labels, `${where}: graph labels differ from the bridge`);
        assert.ok(near(mineSvg.width, expectedSvg.width, expectedSvg.width * 0.03)
            && near(mineSvg.height, expectedSvg.height, expectedSvg.height * 0.03),
            `${where}: graph size ${mineSvg.width}x${mineSvg.height} vs ${expectedSvg.width}x${expectedSvg.height}`);
        assert.equal(Object.keys(actual.typst_renderings).length, Object.keys(expected.typst_renderings).length, `${where}: snippet count`);
        for (const [target, rendering] of Object.entries(expected.typst_renderings)) {
            const mine = actual.typst_renderings[target];
            assert.ok(mine, `${where}: missing rendering ${target}`);
            assert.equal(mine.mode, rendering.mode, `${where}: ${target} mode`);
            for (const attr of ["width", "height"]) {
                assert.ok(near(dim(mine.svg, attr), dim(rendering.svg, attr), Math.max(1, dim(rendering.svg, attr) * 0.05)),
                    `${where}: ${target} ${attr} ${dim(mine.svg, attr)} vs ${dim(rendering.svg, attr)}`);
            }
        }
        assert.deepEqual(actual.edit_regions.map(r => r.target_id), expected.edit_regions.map(r => r.target_id),
            `${where}: editable targets`);
        for (const region of expected.edit_regions) {
            const mine = actual.edit_regions.find(r => r.target_id === region.target_id);
            assert.equal(mine.text, region.text, `${where}: ${region.target_id} label`);
            assert.ok(near(mine.x, region.x, 1) && near(mine.y, region.y, 1), `${where}: ${region.target_id} position`);
        }
        checks++;
        process.stdout.write(`PASS browser parity: ${mode} / ${style} / ${strategy}\n`);
    }

    const rewrite = { source, line: 8 };
    const expectedRewrite = await renderPreview(plugin, rewrite);
    const actualRewrite = await browser.renderPreviewInBrowser(rewrite);
    assert.equal(actualRewrite.math_view.rule_name, expectedRewrite.math_view.rule_name);
    assert.equal(actualRewrite.typst, expectedRewrite.typst);
    assert.equal(normalizeDot(actualRewrite.dot), normalizeDot(expectedRewrite.dot));
    checks++;
    process.stdout.write("PASS browser parity: rewrite alias and second-rule selection\n");

    // Template validation runs the wasm Typst compiler instead of the CLI: the
    // verdict, and therefore the auto-repair suggestion, must match.
    const fields = ["left", "right"];
    for (const template of ["{left} dot {right}", "Mul {{ {left} dot {right} }}", "frac({left}, {right})", "{left}", "{unknown}"]) {
        const expected = await validateTemplate(plugin, template, fields);
        const actual = await browser.validateTemplateInBrowser(template, fields);
        assert.equal(actual.ok, expected.ok, `validateTemplate(${JSON.stringify(template)}) verdict`);
        assert.equal(Boolean(actual.suggestion), Boolean(expected.suggestion), `validateTemplate(${JSON.stringify(template)}) suggestion`);
        checks++;
        process.stdout.write(`PASS browser parity: template ${JSON.stringify(template)} -> ${actual.ok ? "ok" : "repair"}\n`);
    }
    process.stdout.write(`PASS ${checks} browser/bridge parity checks\n`);
}

main().catch(error => { console.error(error); process.exitCode = 1; });
