#!/usr/bin/env node
// Compare this bundle's annotation protocol against egglog-demo's Python one.
//
// The static page has no bridge, so it can no longer ask `preview_annotations.py`
// which rewrite a line belongs to or what the rule structure of a birewrite is.
// This walks every example program in `static/examples.json` and asserts the two
// implementations agree, byte for byte, on every rule/rewrite/birewrite line.
//
//   node test_annotations_parity.mjs <egglog-demo> [examples.json]
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import * as annotations from "./src/annotations.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));

function linesWithRules(source) {
    const forms = [...annotations.walk(annotations.parse(source))];
    const lines = new Set();
    for (const form of forms) {
        if (!["rule", "rewrite", "birewrite"].includes(form.op)) continue;
        lines.add(source.slice(0, form.start).split("\n").length);
    }
    return [...lines].sort((a, b) => a - b);
}

function jsAnswer(request) {
    switch (request.op) {
        case "rewrite_preview_entry": return annotations.rewritePreviewEntry(request.source, request.line);
        case "preview_source": return annotations.previewSource(request.source, request.line);
        default: throw new Error(`unknown op ${request.op}`);
    }
}

function main() {
    const demo = process.argv[2];
    if (!demo) throw new Error("usage: node test_annotations_parity.mjs <egglog-demo> [examples.json]");
    const examplesPath = process.argv[3] || path.join(demo, "static/examples.json");
    const examples = JSON.parse(fs.readFileSync(examplesPath, "utf8"));

    const requests = [];
    for (const [name, source] of Object.entries(examples)) {
        if (typeof source !== "string") continue;
        for (const line of linesWithRules(source)) {
            requests.push({ op: "rewrite_preview_entry", source, line, name });
            requests.push({ op: "preview_source", source, line, name });
        }
    }
    assert.ok(requests.length, `no rule lines found in ${examplesPath}`);

    const reference = spawnSync("python3",
        [path.join(HERE, "annotation_reference.py"), "--demo", demo],
        { input: JSON.stringify(requests), encoding: "utf8", maxBuffer: 256 * 1024 * 1024 });
    assert.equal(reference.status, 0, reference.stderr);
    const expected = JSON.parse(reference.stdout);

    let checked = 0;
    requests.forEach((request, index) => {
        const mine = { ok: jsAnswer(request) };
        assert.deepEqual(mine, expected[index],
            `${request.name} line ${request.line}: ${request.op} differs from preview_annotations.py`);
        checked++;
    });
    process.stdout.write(`PASS ${checked} annotation parity checks over ${Object.keys(examples).length} programs\n`);
}

try { main(); } catch (error) { console.error(error); process.exitCode = 1; }
