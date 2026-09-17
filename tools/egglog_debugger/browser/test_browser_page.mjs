#!/usr/bin/env node
// Serve the static debugger page, point it at a dead bridge, and check that a
// trace row previews its formula and DOT from wasm alone.
//
//   node browser/test_browser_page.mjs --dir <static build> --webdeps <node_modules> [--chromium <path>]
//   node browser/test_browser_page.mjs --url https://milkblock.github.io/fractal_egraph/   # post-deploy check
//
// `--webdeps` is only used to resolve playwright; the same node_modules the
// browser build takes its typst.ts and viz.js from.
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const DEBUGGER = path.dirname(HERE);
const ROOT = path.dirname(path.dirname(DEBUGGER));

const TYPES = {
    ".html": "text/html", ".js": "text/javascript", ".mjs": "text/javascript",
    ".css": "text/css", ".json": "application/json", ".wasm": "application/wasm",
    ".svg": "image/svg+xml", ".ttf": "font/ttf", ".otf": "font/otf"
};

function parseArgs(argv) {
    const options = {};
    for (let index = 0; index < argv.length; index += 2) {
        const key = argv[index].replace(/^--/, "").replace(/-([a-z])/g, (_, c) => c.toUpperCase());
        options[key] = argv[index + 1];
    }
    if (!options.dir && !options.url) throw new Error("--dir or --url is required");
    return options;
}

function serve(folder) {
    const server = createServer(async (request, response) => {
        const url = new URL(request.url, "http://localhost");
        const target = path.join(folder, decodeURIComponent(url.pathname === "/" ? "/index.html" : url.pathname));
        try {
            const body = await readFile(target);
            response.writeHead(200, { "Content-Type": TYPES[path.extname(target)] || "application/octet-stream" });
            response.end(body);
        } catch {
            response.writeHead(404).end("not found");
        }
    });
    return new Promise(resolve => server.listen(0, "127.0.0.1", () => resolve({
        server,
        url: `http://127.0.0.1:${server.address().port}/`
    })));
}

async function main() {
    const options = parseArgs(process.argv.slice(2));
    const require = createRequire(path.join(options.webdeps || path.join(ROOT, "dpsk_workspace/viz-web-editor/node_modules"), "noop.cjs"));
    const { chromium } = require("playwright");
    const source = (await readFile(options.source || path.join(ROOT, "experiments/bake/increment-3.egg"), "utf8"));
    const local = options.url ? null : await serve(path.resolve(options.dir));
    const url = options.url || local.url;
    const browser = await chromium.launch({ headless: true, ...(options.chromium ? { executablePath: options.chromium } : {}) });
    try {
        const page = await browser.newPage();
        const errors = [];
        page.on("pageerror", error => errors.push(String(error)));
        // A dead port forces the page onto the wasm path even on loopback.
        await page.addInitScript("window.__EGGLOG_BRIDGE__='http://127.0.0.1:8199'");
        await page.goto(url, { waitUntil: "networkidle" });
        await page.waitForSelector("#native-run");
        await page.waitForTimeout(1500);
        await page.evaluate(text => { document.querySelector(".CodeMirror").CodeMirror.setValue(text); }, source);
        await page.click("#native-run");
        await page.waitForFunction(() => document.querySelector("#native-status").textContent.includes("完成"), null, { timeout: 180000 });
        const rows = page.locator("#native-trace button");
        assert.ok(await rows.count(), "the run produced no trace rows");
        await rows.first().click();
        await page.waitForSelector('#native-preview[data-ready="true"]', { timeout: 180000 });
        const renderer = await page.locator("#native-renderer").innerText();
        assert.match(renderer, /browser-/, `preview did not come from the browser bundle: ${renderer}`);
        assert.match(renderer, /eggplant-pattern-vscode/);
        assert.ok(await page.locator("#native-preview svg.typst-doc").count(), "no Typst formula was mounted");
        assert.ok(await page.evaluate(() => document.querySelectorAll("#native-preview svg path").length) > 5);
        await page.selectOption("#native-format", "dot");
        await page.waitForFunction(() => document.querySelectorAll("#native-preview svg g.node").length > 0, null, { timeout: 180000 });
        assert.equal(await page.locator("#native-render-error").innerText(), "");

        // Editing belongs to source previews: put the cursor on a rule line, like
        // a user clicking it. (Trace rows are event snapshots and stay read only.)
        await page.selectOption("#native-format", "typst");
        await page.evaluate(() => {
            document.querySelector(".CodeMirror").CodeMirror.setCursor({ line: 2, ch: 0 });
        });
        await page.waitForFunction(
            () => document.querySelectorAll('#native-preview .native-edit-hit[data-target^="constructor:"]').length > 0,
            null, { timeout: 180000 });
        // Parse the annotation lines the edits wrote, instead of pattern matching
        // escaped JSON inside the source.
        const annotations = () => page.evaluate(() => document.querySelector(".CodeMirror").CodeMirror.getValue()
            .split("\n").filter(line => line.includes("@egg-viz-json"))
            .map(line => JSON.parse(line.slice(line.indexOf("@egg-viz-json") + "@egg-viz-json ".length))));

        // A constructor rename writes a `dsl_type` template, which the browser
        // Typst compiler has to accept before anything is written.
        await page.locator('#native-preview .native-edit-hit[data-target^="constructor:"]').first().click();
        await page.waitForSelector("#native-name-editor:not([hidden])");
        await page.fill("#native-name-input", "RenamedCtor");
        await page.click("#native-name-save");
        await page.waitForFunction(() => document.querySelector(".CodeMirror").CodeMirror.getValue().includes("RenamedCtor"),
            null, { timeout: 180000 });
        assert.equal(await page.locator("#native-edit-error").innerText(), "");
        const afterConstructor = await annotations();
        const dslType = afterConstructor.find(entry => entry.kind === "dsl_type");
        assert.equal(dslType.variants.A.typst, 'upright("RenamedCtor")({arg0}, {arg1})', JSON.stringify(dslType));

        // A binding rename writes the rule's `labels.bindings` instead.
        await page.waitForFunction(
            () => document.querySelectorAll('#native-preview .native-edit-hit[data-target^="binding:"]').length > 0,
            null, { timeout: 180000 });
        await page.locator('#native-preview .native-edit-hit[data-target^="binding:"]').first().click();
        await page.waitForSelector("#native-name-editor:not([hidden])");
        await page.fill("#native-name-input", "renamed_node");
        await page.click("#native-name-save");
        await page.waitForFunction(() => document.querySelector(".CodeMirror").CodeMirror.getValue().includes("renamed_node"),
            null, { timeout: 180000 });
        const afterBinding = await annotations();
        const rule = afterBinding.find(entry => entry.kind === "original_rule");
        assert.ok(rule, JSON.stringify(afterBinding));
        assert.ok(Object.values(rule.labels.bindings).includes("renamed_node"), JSON.stringify(rule));

        assert.ok(await page.evaluate(() => window.egglogNative.rendered) >= 2);
        assert.deepEqual(errors, []);
        const line = await page.evaluate(() => window.egglogNative.selected.source_line);
        assert.equal(await page.evaluate(() => window.egglogNative.selected.kind), undefined);
        process.stdout.write(`PASS wasm preview page: Typst formula, DOT and a .egg edit written back`
            + ` entirely from wasm (L${line}, ${renderer.split(" · ")[1]})\n`);
    } finally {
        await browser.close();
        local?.server.close();
    }
}

main().catch(error => { console.error(error); process.exitCode = 1; });
