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
        case "catalog": return annotations.catalog(request.source, request.line);
        case "default_template": return annotations.defaultTemplate(request.name, request.arity);
        case "update_display":
            return annotations.updateDisplay(request.source, request.line, request.target_id,
                request.value, request.fields, request.template, request.precedence);
        case "update_conditions":
            return annotations.updateConditions(request.source, request.line, request.conditions);
        default: throw new Error(`unknown op ${request.op}`);
    }
}

// The reference reports thrown `ValueError`s as `{error}`; mirror that so the
// two implementations are compared on their messages too.
function jsOutcome(request) {
    try { return { ok: jsAnswer(request) }; } catch (error) { return { error: String(error.message) }; }
}

function reference(requests, demo) {
    const result = spawnSync("python3",
        [path.join(HERE, "annotation_reference.py"), "--demo", demo],
        { input: JSON.stringify(requests), encoding: "utf8", maxBuffer: 256 * 1024 * 1024 });
    assert.equal(result.status, 0, result.stderr);
    return JSON.parse(result.stdout);
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

    const expected = reference(requests, demo);

    let checked = 0;
    requests.forEach((request, index) => {
        assert.deepEqual(jsOutcome(request), expected[index],
            `${request.name} line ${request.line}: ${request.op} differs from preview_annotations.py`);
        checked++;
    });
    process.stdout.write(`PASS ${checked} annotation parity checks over ${Object.keys(examples).length} programs\n`);

    // Editing: derived from the Python catalog, so every request names a target
    // that really exists on that line.
    const programs = Object.entries(examples).filter(([, text]) => typeof text === "string");
    const edits = [];
    for (const [name, source] of programs) {
        // Three rule lines per program keep the reference payload small; the
        // per-line protocol is the same everywhere else.
        for (const line of linesWithRules(source).slice(0, 3)) {
            edits.push({ op: "catalog", source, line, name });
        }
    }
    const catalogs = reference(edits, demo);
    const requests2 = [];
    const label = [];
    edits.forEach((request, index) => {
        const targets = catalogs[index].ok;
        if (!Array.isArray(targets) || !targets.length) return;
        const constructors = targets.filter(target => target.kind === "constructor");
        const bindings = targets.filter(target => target.kind === "binding");
        const conditions = targets.find(target => target.kind === "conditions");
        for (const target of constructors.slice(0, 2)) {
            // Rename, rename with an explicit template, and rename with fields
            // reordered — the three shapes the editor submits.
            requests2.push({ op: "update_display", source: request.source, line: request.line,
                target_id: target.id, value: "RenamedOp", name: `${request.name}#${target.id}` });
            requests2.push({ op: "update_display", source: request.source, line: request.line,
                target_id: target.id, value: "RenamedOp", template: "upright(\"R\")({arg0})",
                fields: target.fields, name: `${request.name}#${target.id}/template` });
            requests2.push({ op: "update_display", source: request.source, line: request.line,
                target_id: target.id, value: target.words?.[0] ?? target.name, fields: [...target.fields].reverse(),
                name: `${request.name}#${target.id}/fields` });
            // A colliding name must be refused with the same message.
            requests2.push({ op: "update_display", source: request.source, line: request.line,
                target_id: target.id, value: targets.find(other => other.id !== target.id)?.words?.[0] ?? target.name,
                name: `${request.name}#${target.id}/collision` });
        }
        for (const target of bindings.slice(0, 2)) {
            requests2.push({ op: "update_display", source: request.source, line: request.line,
                target_id: target.id, value: "renamed_binding", name: `${request.name}#${target.id}` });
        }
        if (conditions) {
            requests2.push({ op: "update_conditions", source: request.source, line: request.line,
                conditions: "(> a 0)", name: `${request.name}#conditions` });
            requests2.push({ op: "update_conditions", source: request.source, line: request.line,
                conditions: "", name: `${request.name}#conditions/clear` });
            requests2.push({ op: "update_conditions", source: request.source, line: request.line,
                conditions: "(rule (foo))", name: `${request.name}#conditions/forbidden` });
        }
    });
    assert.ok(requests2.length, "no editable targets found");
    const expected2 = reference(requests2, demo);
    requests2.forEach((request, index) => {
        assert.deepEqual(jsOutcome(request), expected2[index],
            `${request.name}: ${request.op} differs from preview_annotations.py`);
        checked++;
    });
    const accepted = expected2.filter(entry => "ok" in entry).length;
    process.stdout.write(`PASS ${requests2.length} annotation edit checks against preview_annotations.py`
        + ` (${accepted} accepted, ${requests2.length - accepted} refused)\n`);
}

try { main(); } catch (error) { console.error(error); process.exitCode = 1; }
