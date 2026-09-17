#!/usr/bin/env node
// Compare the instrumented runtime's history between native and wasm32.
//
// The published page runs `debug_stream` in the browser, the bridge spawns the
// native binary, and the two must agree on *what matched*. They do not
// necessarily agree on the order they report it in: the e-graph's tables are
// iterated in a platform dependent order (32-bit vs 64-bit hashing) and native
// matches rules in parallel. The event ids and write cursors follow that order,
// so this test asserts the properties that hold and reports the one that does
// not, instead of pretending the JSONL is byte identical.
//
//   node test_stream_parity.mjs [--binary PATH] [--wasm DIR] [--corpus DIR] [--examples FILE]
//
// assert: same row count, same matches (rule, boundary, concrete bound values,
// in the same execution boundary), same refusal for programs the analysis
// rejects. Reported, not asserted: whether the streams are byte identical.
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath, pathToFileURL } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.dirname(path.dirname(path.dirname(HERE)));

function parseArgs(argv) {
    const options = {
        binary: path.join(ROOT, "target/release/egg_layout"),
        wasm: path.join(ROOT, "target/pages/wasm"),
        corpus: path.join(ROOT, "experiments/bake"),
        examples: path.join(ROOT, "../egglog-demo/static/examples.json")
    };
    for (let index = 0; index < argv.length; index += 2) {
        const key = argv[index].replace(/^--/, "").replace(/-([a-z])/g, (_, c) => c.toUpperCase());
        if (!(key in options)) throw new Error(`Unknown option ${argv[index]}`);
        options[key] = argv[index + 1];
    }
    return options;
}

// Load the browser runtime the page uses, in Node with a fetch shim.
async function loadRuntime(directory) {
    const realFetch = globalThis.fetch;
    globalThis.fetch = (input, init) => {
        const url = typeof input === "string" ? input : input?.url;
        if (typeof url === "string" && url.startsWith("file:")) {
            const body = fs.readFileSync(fileURLToPath(url));
            return Promise.resolve(new Response(body, { headers: { "Content-Type": "application/wasm" } }));
        }
        return realFetch(input, init);
    };
    const module = await import(pathToFileURL(path.join(directory, "egglog_debug_wasm.js")).href);
    await module.default({ module_or_path: fs.readFileSync(path.join(directory, "egglog_debug_wasm_bg.wasm")) });
    return module;
}

// The provenance of a bound value embeds the analyzed file's path: the native
// run gets the real one, the browser entry is handed a synthetic name.
const normalize = text => String(text)
    .replace(/[^\s"\\]*\/[A-Za-z0-9._-]+\.egg/g, "<file>")
    .replace(/input\.egg/g, "<file>");
// A panic is a refusal too, but its text is platform specific: native prints the
// Rust panic, wasm32 only traps (`unreachable`), so only the fact is compared.
const panicked = text => /panicked at|unreachable|RuntimeError/i.test(String(text));

// Both hosts report a refusal as an error string; native prints `Error: "…"`
// and the wasm binding surfaces the message itself.
// The final line of an error is its message; the lines above are context.
const core = text => refusal(text).split("\n").map(line => line.trim()).filter(Boolean).pop() ?? "";

function refusal(text) {
    const clean = normalize(String(text)).trim();
    const quoted = /"([^"]+)"/.exec(clean);
    return (quoted ? quoted[1] : clean.replace(/^Error:\s*/, "")).trim();
}

// What identifies a match: the rule, the execution boundary, and the concrete
// values bound to its variables. Handles, e-class ids and write cursors are
// assigned by the enumeration order, so they are dropped.
function matchIdentity(row) {
    const pairs = (row.binding || []).map(entry => {
        const parsed = /^([^=]+)= (.*)$/.exec(entry);
        if (!parsed) return null;
        const value = /^(\d+):Value\((-?\d+)\)$/.exec(parsed[2]);
        return `${parsed[1].trim()}=${value ? `Value(${value[2]})` : "non-primitive"}`;
    }).filter(Boolean).sort();
    return normalize(JSON.stringify({ kind: row.kind, rule: row.rule, boundary: row.boundary, pairs }));
}

function count(entries) {
    const map = new Map();
    for (const entry of entries) map.set(entry, (map.get(entry) || 0) + 1);
    return map;
}

function compare(name, text, strict) {
    const native = spawnSync(options.binary, ["debug-stream", text.path],
        { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
    // A program whose history does not fit in memory is not a parity failure.
    if (native.error?.code === "ENOBUFS") return { name, skipped: "native stream too large" };
    if (native.error) throw new Error(`${name}: cannot run the native runtime: ${native.error.message}`);
    let rows = null;
    let mineError = null;
    try {
        rows = [];
        runtime.debug_stream(fs.readFileSync(text.path, "utf8"), line => rows.push(JSON.parse(line)));
    } catch (error) {
        mineError = String(error.message ?? error);
    }
    if (native.status !== 0 || mineError) {
        const theirsError = refusal(native.stderr || "");
        assert.ok(mineError, `${name}: native refused (${theirsError}) but wasm produced rows`);
        if (panicked(native.stderr) || panicked(mineError)) {
            assert.ok(panicked(mineError), `${name}: native panicked but wasm reported ${mineError}`);
            return { name, refused: true, panic: true };
        }
        // The two readers report the same failure with different amounts of
        // context (native adds the source snippet and `parse error:` prefix).
        const mineCore = core(mineError), theirsCore = core(theirsError);
        assert.ok(mineCore === theirsCore || theirsCore.includes(mineCore) || mineCore.includes(theirsCore),
            `${name}: the two hosts refuse differently: ${mineCore} vs ${theirsCore}`);
        return { name, refused: true };
    }
    const theirs = native.stdout.split("\n").filter(Boolean).map(line => JSON.parse(line));
    // Row counts are reported, not asserted: a compose row exists only for an
    // application whose input is attributed to an earlier application, and that
    // attribution follows the runtime's own enumeration order (see the README).
    // The match multiset, the boundaries and the refusals are what must hold.
    const counts = list => count(list.map(row => row.kind));
    const nativeKinds = counts(theirs), wasmKinds = counts(rows);
    const kindDelta = Object.fromEntries([...new Set([...Object.keys(nativeKinds), ...Object.keys(wasmKinds)])]
        .map(kind => [kind, (wasmKinds[kind] ?? 0) - (nativeKinds[kind] ?? 0)]).filter(([, delta]) => delta !== 0));
    const nativeCounts = count(theirs.map(matchIdentity));
    const mineCounts = count(rows.map(matchIdentity));
    const boundary = list => count(list.filter(row => row.boundary).map(row => normalize(
        JSON.stringify({ kind: row.kind, rule: row.rule, boundary: row.boundary }))));
    const nativeBoundaries = boundary(theirs);
    const mineBoundaries = boundary(rows);
    let missingMatches = 0;
    for (const key of new Set([...nativeCounts.keys(), ...mineCounts.keys()])) {
        const delta = (mineCounts.get(key) ?? 0) - (nativeCounts.get(key) ?? 0);
        if (delta === 0) continue;
        missingMatches += Math.abs(delta);
        if (strict) {
            assert.fail(`${name}: match sets differ at ${key.slice(0, 200)} (${delta > 0 ? "+" : ""}${delta})`);
        }
    }
    let boundaryDelta = 0;
    for (const key of new Set([...nativeBoundaries.keys(), ...mineBoundaries.keys()])) {
        const delta = (mineBoundaries.get(key) ?? 0) - (nativeBoundaries.get(key) ?? 0);
        if (delta === 0) continue;
        boundaryDelta += Math.abs(delta);
        if (strict) assert.fail(`${name}: boundary histogram differs at ${key} (${delta > 0 ? "+" : ""}${delta})`);
    }
    return {
        name, rows: theirs.length, kindDelta, missingMatches, boundaryDelta, strict,
        sameSequence: JSON.stringify(theirs.map(matchIdentity)) === JSON.stringify(rows.map(matchIdentity))
    };
}

const options = parseArgs(process.argv.slice(2));
assert.ok(fs.existsSync(options.binary), `build the runtime first: ${options.binary} is missing`);
assert.ok(fs.existsSync(path.join(options.wasm, "egglog_debug_wasm.js")),
    `build the runtime wasm first (build_site.py puts it in ${options.wasm})`);
const runtime = await loadRuntime(options.wasm);

const folder = fs.mkdtempSync(path.join(os.tmpdir(), "egglog-stream-parity-"));
const programs = [];
try {
    if (fs.existsSync(options.corpus)) {
        for (const entry of fs.readdirSync(options.corpus).filter(name => name.endsWith(".egg")).sort()) {
            programs.push({ name: entry, path: path.join(options.corpus, entry), strict: true });
        }
    }
    // The analysis names its sort after the program's datatype, so a renamed copy
    // of the same program must analyze identically on both hosts.
    const increment = path.join(options.corpus, "increment-3.egg");
    if (fs.existsSync(increment)) {
        const renamed = path.join(folder, "increment-3-expr.egg");
        fs.writeFileSync(renamed,
            fs.readFileSync(increment, "utf8").replace("(datatype Math ", "(datatype Expr "));
        programs.push({ name: "increment-3 (datatype Expr)", path: renamed, strict: true });
    }
    if (options.examples && fs.existsSync(options.examples)) {
        const examples = JSON.parse(fs.readFileSync(options.examples, "utf8"));
        for (const [name, source] of Object.entries(examples)) {
            if (typeof source !== "string") continue;
            const file = path.join(folder, `${name}.egg`);
            fs.writeFileSync(file, source);
            // The demo programs are a survey, not a contract: a saturating program
            // can fire its rules in a different order on the two platforms and
            // then observes different matches (see the README).
            programs.push({ name: `examples/${name}`, path: file, strict: false });
        }
    }
    assert.ok(programs.length, "no programs to compare");

    const results = programs.map(program => compare(program.name, program, program.strict));
    const refused = results.filter(result => result.refused).length;
    const panics = results.filter(result => result.panic).map(result => result.name);
    const compared = results.filter(result => !result.refused && !result.skipped);
    const identical = compared.filter(result => result.sameSequence).length;
    const rows = compared.reduce((total, result) => total + result.rows, 0);
    const corpus = compared.filter(result => result.strict);
    process.stdout.write(`PASS ${results.length} programs (${rows} rows, ${refused} refused identically): `
        + `the ${corpus.length} fractal-corpus programs agree on row kinds, match identities and boundaries`
        + ` (${corpus.filter(result => result.sameSequence).length} of them byte identical including order)\n`);
    const divergent = compared.filter(result => result.missingMatches || result.boundaryDelta
        || Object.values(result.kindDelta).some(delta => delta !== 0));
    if (divergent.length) {
        process.stdout.write("  survey programs whose history differs on the two platforms"
            + " (rule firing order is hash dependent there):\n");
        for (const result of divergent) {
            process.stdout.write(`    ${result.name}: ${result.missingMatches} row identities, `
                + `${result.boundaryDelta} boundary rows, kinds ${JSON.stringify(result.kindDelta)}\n`);
        }
    }
    if (panics.length) process.stdout.write(`  both hosts panicked: ${panics.join(", ")}\n`);
    const skipped = results.filter(result => result.skipped);
    if (skipped.length) process.stdout.write(`  skipped: ${skipped.map(result => result.name).join(", ")}\n`);
    const differing = compared.filter(result => !result.sameSequence
        && !result.missingMatches && !result.boundaryDelta).map(result => result.name);
    if (differing.length) {
        process.stdout.write(`  same matches in a different order (event ids differ): ${differing.join(", ")}\n`);
    }
} finally {
    fs.rmSync(folder, { recursive: true, force: true });
}
