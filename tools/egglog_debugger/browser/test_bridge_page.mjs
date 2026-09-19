#!/usr/bin/env node
// The local workflow: `server.py` plus the page it serves, no static build and no
// network. The bridge does the rendering; the browser bundle is still needed to
// generate a fractal lane's `.egg`, so this also checks that the server serves it.
//
//   python3 tools/egglog_debugger/server.py --port 8080
//   node browser/test_bridge_page.mjs [--url http://127.0.0.1:8080/] [--chromium PATH]
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.dirname(path.dirname(path.dirname(HERE)));

function parseArgs(argv) {
    const options = { url: "http://127.0.0.1:8080/" };
    for (let index = 0; index < argv.length; index += 2) {
        const key = argv[index].replace(/^--/, "").replace(/-([a-z])/g, (_, c) => c.toUpperCase());
        options[key] = argv[index + 1];
    }
    return options;
}

async function main() {
    const options = parseArgs(process.argv.slice(2));
    const webdeps = options.webdeps || path.join(ROOT, "dpsk_workspace/viz-web-editor/node_modules");
    const require = createRequire(path.join(webdeps, "noop.cjs"));
    const { chromium } = require("playwright");
    const source = await readFile(options.source || path.join(ROOT, "experiments/bake/increment-3.egg"), "utf8");
    // Fail early with the command to run rather than with a page timeout.
    const probe = await fetch(new URL("/plugin-overlay.js", options.url)).catch(() => null);
    assert.ok(probe?.ok, `no bridge at ${options.url}; run: python3 tools/egglog_debugger/server.py --port 8080`);

    const browser = await chromium.launch({ headless: true, ...(options.chromium ? { executablePath: options.chromium } : {}) });
    try {
        const page = await browser.newPage();
        const errors = [];
        page.on("pageerror", error => errors.push(String(error)));
        await page.goto(options.url, { waitUntil: "networkidle" });
        await page.waitForSelector("#native-run");
        await page.waitForTimeout(1500);
        // A page served by the bridge is same-origin, so no bridge override is set
        // and the previews come from the bridge rather than the wasm bundle.
        assert.equal(await page.evaluate(() => window.__EGGLOG_BRIDGE__), undefined);
        assert.equal(await page.evaluate(() => fetch("browser/preview.js").then(r => r.status)), 200,
            "the server must serve the browser bundle: the page imports it to generate a fractal lane");

        await page.evaluate(text => { document.querySelector(".CodeMirror").CodeMirror.setValue(text); }, source);
        await page.click("#native-run");
        await page.waitForFunction(() => document.querySelector("#native-status").textContent.includes("完成"), null, { timeout: 180000 });
        assert.match(await page.locator("#native-status").innerText(), /4 个 Fractal 证据/);

        await page.selectOption("#native-filter", "fractal");
        await page.locator("#native-trace button").last().click();
        await page.waitForSelector('#native-preview[data-ready="true"]', { timeout: 180000 });
        assert.equal(await page.locator("#native-step option").count(), 1);
        // The generated `.egg` is shown, not hidden inside the request.
        assert.equal(await page.locator("#native-generated-panel").isVisible(), true);
        const generated = await page.locator("#native-generated").textContent();
        assert.match(generated, /\(rule \(\(= node \(A n limit\)\) \(< n limit\)\)/, generated);
        assert.match(generated, /:name "fractal:advance"/, generated);
        assert.match(await page.locator("#native-generated-title").textContent(), /只展开前几步/);

        const formula = await page.locator("#native-source").textContent();
        assert.match(formula, /underbrace\(/, formula);
        assert.match(formula, /upright\("trigger/, formula);
        assert.match(formula, /apply 5 times/, formula);
        // Each state carries the runtime's verdict for that step, and a coarse step
        // names the outside input it consumes. This lane's trigger reads the initial
        // `advance` fact, so the legend has to say so.
        assert.match(formula, /upright\("trigger · coarse"\)/, formula);
        assert.match(formula, /upright\("apply once · smooth"\)/, formula);
        assert.match(formula, /needs external: limit, n, L\d+/, formula);
        // native_lower refuses this datatype, so the fused rule is reported as the
        // reason instead of rendered; the panel still opens and explains itself.
        assert.equal(await page.locator("#native-coarse-panel").isVisible(), true);
        assert.match(await page.locator("#native-coarse-note").innerText(), /不能融合这条 lane：unsupported endpoint operator/, "coarse refusal");
        assert.equal(await page.locator("#native-coarse-render svg").count(), 0);

        // The layer panel renders each member from the *current* editor text, so an
        // edited annotation template refreshes it without re-running recognition.
        const annotated = await readFile(path.join(HERE, "../fixtures/fractal-math.egg"), "utf8");
        await page.evaluate(text => { document.querySelector(".CodeMirror").CodeMirror.setValue(text); }, annotated);
        await page.click("#native-run");
        await page.waitForFunction(() => document.querySelector("#native-status").textContent.includes("完成"), null, { timeout: 180000 });
        const traceRows = await page.locator("#native-trace button").count();
        const logged = await page.evaluate(() => window.egglogNative.rows.length);
        await page.selectOption("#native-layer-kind", "fractals");
        await page.selectOption("#native-layer-format", "typst");
        await page.click("#native-layer-render");
        await page.waitForFunction(() => document.querySelector("#native-layer-viewport").dataset.ready === "true", null, { timeout: 180000 });
        assert.equal(await page.locator("#native-layer-error").innerText(), "");
        const panelCode = () => page.locator("#native-layer-code").textContent();
        assert.match(await panelCode(), /integral \(a \* b\)/, "the annotated Typst template should be used");
        assert.doesNotMatch(await panelCode(), /upright\("Integral"\)/, "the un-annotated fallback must not appear");
        assert.equal(await page.locator("#native-layer-viewport [data-provenance]").first().getAttribute("data-provenance"), "editor");
        assert.equal(await page.locator("#native-layer-viewport .native-layer-source").count(), 0, "the live source needs no provenance note");
        // Only the annotation line changes: no rerun, but the panel must re-render.
        const edited = annotated.replace('"typst": "integral {integrand} quad d {variable}"', '"typst": "upright(\\"INT\\") {integrand} quad d {variable}"');
        assert.notEqual(edited, annotated, "the fixture should carry the Integral template");
        await page.evaluate(text => { document.querySelector(".CodeMirror").CodeMirror.setValue(text); }, edited);
        await page.waitForFunction(() => document.querySelector("#native-layer-code").textContent.includes("INT"), null, { timeout: 180000 });
        assert.match(await page.locator("#native-status").innerText(), /源码已修改/, "an edit only marks the source as modified");
        assert.equal(await page.locator("#native-trace button").count(), traceRows, "no rerun, so the trace list must not grow");
        assert.equal(await page.evaluate(() => window.egglogNative.rows.length), logged, "no rerun, so the log must not be rebuilt");
        // A rule that is gone from the editor falls back to the run-time snapshot.
        await page.evaluate(() => {
            const cm = document.querySelector(".CodeMirror").CodeMirror;
            cm.setValue(cm.getValue().split("\n").filter(line =>
                !line.startsWith("(rewrite (Integral (Mul a b) x)") &&
                !line.startsWith("(Sub (Mul a (Integral b x))") &&
                !line.startsWith("    (Integral (Mul (Diff x a)")).join("\n"));
        });
        await page.click("#native-layer-render");
        await page.waitForSelector("#native-layer-viewport [data-provenance=snapshot]", { timeout: 180000 });
        assert.match(await page.locator("#native-layer-viewport .native-layer-source").first().innerText(), /运行快照/);

        // The `math` example does fuse: show the lane both as the repetition (smooth
        // chain) and as the one-step coarse rule the plugin renders. The fixture is
        // that example's integration-by-parts lane trimmed to what the lane needs;
        // the full example runs to saturation and streams hundreds of MB.
        const mathSource = await readFile(path.join(HERE, "../fixtures/fractal-math.egg"), "utf8");
        await page.evaluate(text => { document.querySelector(".CodeMirror").CodeMirror.setValue(text); }, mathSource);
        await page.click("#native-run");
        await page.waitForFunction(() => document.querySelector("#native-status").textContent.includes("完成"), null, { timeout: 180000 });
        await page.selectOption("#native-filter", "fractal");
        const laneCount = await page.locator("#native-trace button").count();
        assert.ok(laneCount > 0, "the math lane fixture should produce a fractal lane");
        await page.locator("#native-trace button").last().click();
        await page.waitForSelector('#native-preview[data-ready="true"]', { timeout: 180000 });
        const fusedCode = await page.locator("#native-coarse").textContent();
        assert.match(fusedCode, /^\(rule \(/, fusedCode);
        assert.match(fusedCode, /:name "native_comb_\d+"/, fusedCode);
        assert.match(fusedCode, /Integral/, fusedCode);
        assert.equal(await page.locator("#native-coarse-panel").isVisible(), true);
        assert.ok((await page.locator("#native-coarse-render svg").boundingBox()).width > 0, "the fused rule should render");
        assert.match(await page.locator("#native-coarse-title").innerText(), /融合 \d+ 步/);
        // The chain and the fused rule are two views of the same lane, so the smooth
        // chain keeps its per-step verdicts and the coarse rule is the fused one.
        const laneFormula = await page.locator("#native-source").textContent();
        assert.match(laneFormula, /upright\("trigger"\)/, laneFormula);
        assert.match(laneFormula, /upright\("apply once · smooth"\)/, laneFormula);
        assert.match(laneFormula, /upright\("apply twice · smooth"\)/, laneFormula);
        await page.evaluate(text => { document.querySelector(".CodeMirror").CodeMirror.setValue(text); }, source);
        const renderer = await page.locator("#native-renderer").innerText();
        assert.match(renderer, /eggplant-pattern-vscode/);
        assert.doesNotMatch(renderer, /browser-/, `the bridge should render, not the wasm bundle: ${renderer}`);
        // A row without a generated rule hides the panel again.
        await page.selectOption("#native-filter", "application");
        await page.locator("#native-trace button").first().click();
        await page.waitForFunction(() => document.querySelector('#native-preview[data-ready="true"]')
            && document.querySelector("#native-generated-panel").hidden, null, { timeout: 180000 });
        assert.equal(await page.locator("#native-generated-panel").isVisible(), false);
        // The runtime has to stream: rows must appear while the run is still going.
        // A synchronous wasm call froze the page, so the whole log appeared at once
        // when the program finished.
        const longSource = await readFile(path.join(ROOT, "experiments/bake/heldout-increment.egg"), "utf8");
        await page.evaluate(text => { document.querySelector(".CodeMirror").CodeMirror.setValue(text); }, longSource);
        await page.click("#native-run");
        await page.waitForFunction(
            () => document.querySelectorAll("#native-trace button").length > 0
                && !document.querySelector("#native-status").textContent.includes("完成"),
            null, { timeout: 180000 });
        const midRun = await page.locator("#native-trace button").count();
        await page.waitForFunction(() => document.querySelector("#native-status").textContent.includes("完成"), null, { timeout: 180000 });
        const finished = await page.locator("#native-trace button").count();
        assert.ok(midRun < finished, `only ${midRun} of ${finished} rows streamed before the run finished`);
        await page.evaluate(text => { document.querySelector(".CodeMirror").CodeMirror.setValue(text); }, source);

        assert.deepEqual(errors, []);
        process.stdout.write("PASS local bridge page: run, identify and preview a fractal lane served by server.py\n");
    } finally {
        await browser.close();
    }
}

main().catch(error => { console.error(error); process.exitCode = 1; });
