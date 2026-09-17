// Use the VS Code extension's modules, binaries, defaults and Graphviz runtime.
// No second AST-to-formula or AST-to-DOT implementation lives in this bridge.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const { editableRegions } = require('./typst-edit.cjs');

function loadPlugin(root, extractorOverride) {
    root = path.resolve(root);
    const load = name => require(path.join(root, 'out', name + '.js'));
    const transpiler = load('eggTranspiler');
    transpiler.configureTranspilerResolution(root);
    const extractor = extractorOverride || path.join(root, 'bin', `${process.platform}-${process.arch}`,
        process.platform === 'win32' ? 'eggplant-pattern-extractor.exe' : 'eggplant-pattern-extractor');
    const modules = ['eggTranspiler', 'eggRuleMapping', 'mathView', 'dot', 'typst', 'svg', 'shared/typstCore'];
    for (const name of ['eggPreviewSource', 'eggVizAnnotation', 'rustBindingRename', 'rendererWasm']) {
        if (fs.existsSync(path.join(root, 'out', name + '.js'))) modules.push(name);
    }
    const hash = crypto.createHash('sha256');
    for (const name of modules) hash.update(fs.readFileSync(path.join(root, 'out', name + '.js')));
    hash.update(fs.readFileSync(path.join(root, 'out/previewPanel.js')));
    hash.update(fs.readFileSync(path.join(root, 'vendor/viz.cjs')));
    hash.update(fs.readFileSync(extractor));
    hash.update(fs.readFileSync(path.join(root, 'vendor/transpiler-wasm/eggplant_transpiler_wasm_wrapper_bg.wasm')));
    return { root, extractor, transpiler, load, revision: hash.digest('hex') };
}

// Names a rule binding renders under: the query node, and the whole field accessor
// for primitive fields. The editor needs both as click targets before any rename.
function bindingInfo(plugin, prepared, ir) {
    const structure = prepared.ruleStructure;
    const bindings = structure?.bindings || [];
    const nodes = (ir.nodes || []).map(node => node.id);
    const info = { nodes: {}, labels: {} };
    if (!bindings.length || !nodes.length) return info;
    const probe = { labels: { bindings: Object.fromEntries(bindings.map((name, index) => [name, `probe${index}`])) } };
    try {
        const { overrides, bindingAccessors } = plugin.load('eggVizAnnotation').buildDisplayNames(probe, structure, ir, prepared.rust);
        const byLabel = new Map(plugin.load('rustBindingRename').planBindingRenames(nodes, overrides).map(entry => [entry.to, entry.from]));
        bindings.forEach((name, index) => { const node = byLabel.get(`probe${index}`); if (node) info.nodes[name] = node; });
        // A field binding renders as `<node>.<field>`. Exposing the accessor keeps the
        // fields of one node distinct (`m.x` vs `m.y`) instead of collapsing to `m`.
        for (const [name, accessor] of bindingAccessors || []) {
            if (typeof accessor !== 'string' || !accessor.includes('.')) continue;
            info.labels[name] = (ir.display_names && ir.display_names[accessor]) || accessor;
        }
    } catch { /* variables stay read-only if the probe cannot run */ }
    return info;
}

async function renderPreview(plugin, request) {
    const { source, line = 1, mode = 'combined', label_style = 'recursive', recursive_strategy = 'dag-expand' } = request;
    if (typeof source !== 'string' || !Number.isInteger(line) || line < 1) throw Error('Expected source and a positive source line');
    if (!['pattern', 'action', 'combined'].includes(mode)) throw Error('Invalid DOT view mode');
    if (!['compact', 'full', 'recursive'].includes(label_style)) throw Error('Invalid label style');
    if (!['tree-safe', 'dag-expand'].includes(recursive_strategy)) throw Error('Invalid recursive strategy');
    const rust = await plugin.transpiler.transpileEggSource(source);
    let offset = source.split('\n').slice(0, line - 1).join('\n').length + (line > 1 ? 1 : 0);
    // The caller supplies the native parser's start line, which can precede the
    // opening parenthesis by indentation. The plugin mapper expects the form itself.
    offset += source.slice(offset).search(/\S|$/);
    const extract = (rustSource, rustOffset) => new Promise((resolve, reject) => {
        const child = require('node:child_process').spawn(plugin.extractor,
            ['--offset', String(Buffer.byteLength(rustSource.slice(0, rustOffset), 'utf8'))],
            { cwd: path.dirname(plugin.root), stdio: ['pipe', 'pipe', 'pipe'] });
        let stdout = '', stderr = '';
        child.stdout.on('data', chunk => { stdout += chunk; });
        child.stderr.on('data', chunk => { stderr += chunk; });
        child.on('error', reject);
        child.on('close', code => {
            if (code) return reject(Error(stderr || `Extractor exited with ${code}`));
            try { resolve(JSON.parse(stdout)); } catch (error) { reject(error); }
        });
        child.stdin.end(rustSource);
    });
    let ir, variableMap = {}, variableLabels = {}, rustSource = '', rustStartLine = 0;
    if (fs.existsSync(path.join(plugin.root, 'out/eggPreviewSource.js'))) {
        const { prepareEggPreviewPlan, buildEggPreview } = plugin.load('eggPreviewSource');
        const plan = prepareEggPreviewPlan(source, rust);
        // The plugin drops `rewrite` forms from program.rules, which both ignores their
        // annotations and shifts every later rule onto the wrong annotation. Index the
        // entries by ordinal and fill the selected rewrite from the .egg source.
        const ordinal = plugin.load('eggRuleMapping').eggRuleOrdinalAtOffset(source, offset);
        const entries = new Map((plan.program.rules || []).map(entry => [entry.ordinal, entry]));
        if (request.rule_entry?.structure) entries.set(ordinal, { ordinal, structure: request.rule_entry.structure, annotation: request.rule_entry.annotation || {}, ruleStart: offset });
        plan.program.rules = [...entries.keys()].reduce((list, key) => { list[key] = entries.get(key); return list; }, []);
        const prepared = await buildEggPreview(plan, offset, extract);
        ir = prepared.decorate(await extract(prepared.rust, prepared.extractorOffset), prepared.rust);
        const info = bindingInfo(plugin, prepared, ir);
        variableMap = info.nodes;
        variableLabels = info.labels;
        // The exact Rust handed to the extractor for this rule, for the source panel.
        const span = plugin.load('rustBindingRename').findMatchingParenRange(prepared.rust, prepared.extractorOffset, 'rust');
        if (span) {
            rustSource = prepared.rust.slice(span.start, span.end);
            rustStartLine = prepared.rust.slice(0, span.start).split('\n').length;
        }
    } else {
        const rustOffset = plugin.load('eggRuleMapping').resolveEggPreviewOffset(source, offset, rust);
        ir = await extract(rust, rustOffset);
        rustSource = rust;
        rustStartLine = 1;
    }
    const { buildMathViewModel, buildMathViewTypstSource } = plugin.load('mathView');
    const { collectTypstReplacementSources, patternIrToDotWithMode } = plugin.load('dot');
    const mathView = buildMathViewModel(ir, source);
    const formulaSource = buildMathViewTypstSource(mathView);
    const formulaTarget = `math-view:${mathView.rule_name}`;
    const typstSources = Object.fromEntries(collectTypstReplacementSources(ir, mode, label_style, recursive_strategy)
        .map(({ targetId, source }) => [targetId, source]));
    typstSources[formulaTarget] = formulaSource;
    const typstRenderings = await plugin.load('typst').renderTypstSnippets(
        Object.entries(typstSources).map(([targetId, source]) => ({ targetId, source })));
    if (!typstRenderings[formulaTarget]) throw Error('Plugin Typst renderer could not render the rule formula');
    let dot = patternIrToDotWithMode(ir, mode, label_style, recursive_strategy, typstRenderings);
    let patternRenderer = 'typescript';
    // Match the extension's optional pattern-only renderer override exactly.
    if (mode === 'pattern') {
        try {
            const renderer = await plugin.load('rendererWasm').loadRendererWasm(plugin.root);
            const rendered = JSON.parse(renderer.render_pattern_json(JSON.stringify(ir)));
            if (rendered.dot) { dot = rendered.dot; patternRenderer = 'wasm'; }
        } catch { /* The extension uses the TypeScript renderer if this bundle is absent. */ }
    }
    const graphSvg = await plugin.load('svg').dotToSvg(dot);
    const core = plugin.load('shared/typstCore');
    const formula = typstRenderings[formulaTarget];
    // A binding renders as its node name and, for a primitive field, as the whole
    // accessor (`m.a`, `m.b`); offer both so each field stays separately clickable.
    const targets = (request.edit_targets || []).map(target => ({ ...target }));
    for (const target of targets) {
        if (target.kind !== 'binding') continue;
        const node = variableMap[target.name];
        const label = variableLabels[target.name];
        if (node) {
            target.rendered = node;
            if (!target.words.includes(node)) target.words.push(node);
        }
        if (label && !target.words.includes(label)) target.words.push(label);
    }
    let editRegions = [], editError = null;
    if(formula.mode === 'math'){
        try { editRegions = editableRegions(formulaSource, targets, core.buildTypstMathDocument); }
        catch(error){editError=String(error.message || error);}
    }
    return {
        edit_targets: targets, edit_regions: editRegions, edit_error: editError,
        renderer: 'eggplant-pattern-vscode', renderer_revision: plugin.revision,
        config: { mode, label_style, recursive_strategy, pattern_renderer: patternRenderer },
        ir, math_view: mathView, typst: formulaSource,
        typst_document: formula.mode === 'math' ? core.buildTypstMathDocument(formulaSource) : core.buildTypstTextDocument(formulaSource),
        typst_svg: formula.svg, typst_mode: formula.mode,
        dot, dot_svg: graphSvg, typst_sources: typstSources, typst_renderings: typstRenderings,
        variable_map: variableMap, variable_labels: variableLabels,
        rust_source: rustSource, rust_start_line: rustStartLine,
    };
}

// Expand the plugin's documented `{field}` / `{{` / `}}` template protocol with a
// neutral atomic math atom, then compile it with the same Typst binary and the
// same math document wrapper the plugin uses. This rejects template syntax
// errors before they are written to .egg.
function expandTemplate(template, fields, atom) {
    const chars = [...template];
    let rendered = '';
    for (let index = 0; index < chars.length;) {
        if (chars[index] === '{' && chars[index + 1] === '{') { rendered += '{'; index += 2; continue; }
        if (chars[index] === '}' && chars[index + 1] === '}') { rendered += '}'; index += 2; continue; }
        if (chars[index] === '{') {
            const end = chars.indexOf('}', index + 1);
            if (end < 0) throw Error('模板占位符缺少右花括号 }');
            const name = chars.slice(index + 1, end).join('');
            if (!fields.includes(name)) throw Error(`未声明的模板字段：{${name}}`);
            rendered += atom; index = end + 1; continue;
        }
        rendered += chars[index]; index += 1;
    }
    return rendered;
}

function runTypst(document, timeout = 20000) {
    return new Promise((resolve, reject) => {
        const child = require('node:child_process').spawn(
            process.env.EGGPLANT_PATTERN_TYPST_PATH || 'typst',
            ['compile', '-', '-', '--format', 'svg'], { stdio: ['pipe', 'pipe', 'pipe'] });
        let stdout = '', stderr = '', timer = null;
        child.stdout.on('data', chunk => { stdout += chunk; });
        child.stderr.on('data', chunk => { stderr += chunk; });
        child.on('error', reject);
        child.on('close', code => {
            if (timer) clearTimeout(timer);
            if (code === 0) return resolve(stdout);
            reject(Error((stderr || `typst exited with code ${code}`).trim()));
        });
        timer = setTimeout(() => { child.kill(); reject(Error('Typst 模板编译超时')); }, timeout);
        child.stdin.end(document);
    });
}

// Typst math reads a bare multi-letter word as one unknown identifier, so
// `Mul { … }` fails with `unknown variable: Mul` while `upright("Mul") { … }`
// works. Turn that into an actionable hint instead of a bare Typst dump.
function literalNameHint(template, message) {
    const unknown = /unknown variable: ([A-Za-z_][A-Za-z0-9_]*)/.exec(message);
    if (!unknown) return '';
    const token = unknown[1];
    const words = template.match(/[A-Za-z_][A-Za-z0-9_]*/g) || [];
    const candidate = token.length > 1 ? token : words.find(word => word.length > 1 && word.startsWith(token));
    if (!candidate) return '';
    return `\n提示：Typst 数学模式把 ${candidate} 当作多个单字母变量，而不是一个名称。`
        + `要显示这个字面名称请写 upright("${candidate}") 或 op("${candidate}")；`
        + `要调用函数请检查拼写。需要花括号分组时用 {{ 和 }}。`;
}

// Blank out placeholders, escaped braces and quoted strings so only bare Typst
// words remain; masking keeps indices aligned with the original template.
function maskTemplate(template) {
    const chars = [...template];
    const out = chars.slice();
    for (let index = 0; index < chars.length;) {
        if (chars[index] === '{' && chars[index + 1] === '{') { out[index] = ' '; out[index + 1] = ' '; index += 2; continue; }
        if (chars[index] === '}' && chars[index + 1] === '}') { out[index] = ' '; out[index + 1] = ' '; index += 2; continue; }
        if (chars[index] === '{') {
            const end = chars.indexOf('}', index + 1);
            if (end < 0) break;
            for (let at = index; at <= end; at++) out[at] = ' ';
            index = end + 1; continue;
        }
        if (chars[index] === '"') {
            out[index] = ' ';
            let at = index + 1;
            while (at < chars.length && chars[at] !== '"') {
                if (chars[at] === '\\' && at + 1 < chars.length) { out[at] = ' '; out[at + 1] = ' '; at += 2; }
                else { out[at] = ' '; at += 1; }
            }
            if (at < chars.length) { out[at] = ' '; index = at + 1; } else index = at;
            continue;
        }
        index += 1;
    }
    return out.join('');
}

// `unknown variable: Mul` means a bare word Typst does not know; showing it as
// text needs upright("Mul"). Returns the rewritten template, or null when the
// failing identifier is not a bare word the user actually wrote.
function wrapBareName(template, message) {
    const unknown = /unknown variable: ([A-Za-z_][A-Za-z0-9_]*)/.exec(message);
    if (!unknown) return null;
    const hit = [...maskTemplate(template).matchAll(/[A-Za-z_][A-Za-z0-9_]*/g)].find(match => match[0] === unknown[1]);
    if (!hit) return null;
    const word = hit[0];
    return { template: template.slice(0, hit.index) + `upright(${JSON.stringify(word)})` + template.slice(hit.index + word.length), word };
}

function buildTemplateDocument(plugin, template, fields) {
    return plugin.load('shared/typstCore').buildTypstMathDocument(expandTemplate(template, fields || [], 'upright("x")'));
}

async function validateTemplate(plugin, template, fields) {
    if (typeof template !== 'string' || !template.trim()) return { ok: false, error: 'Typst 模板不能为空' };
    let document;
    try { document = buildTemplateDocument(plugin, template, fields); }
    catch (error) { return { ok: false, error: String(error.message || error) }; }
    try { await runTypst(document); return { ok: true }; }
    catch (error) { return repairTemplate(plugin, template, fields, String(error.message || error)); }
}

// Best-effort automatic fix: wrap each unknown bare name and recompile. The caller
// only writes the suggestion after the user confirms it, never on the first submit.
async function repairTemplate(plugin, template, fields, message) {
    let current = template;
    const notes = [];
    let failure = message;
    for (let round = 0; round < 8; round++) {
        const fixed = wrapBareName(current, failure);
        if (!fixed) break;
        current = fixed.template;
        notes.push(`${fixed.word} → upright("${fixed.word}")`);
        let document;
        try { document = buildTemplateDocument(plugin, current, fields); }
        catch (error) { return { ok: false, error: String(error.message || error), notes }; }
        try {
            await runTypst(document);
            return { ok: false, error: failure + literalNameHint(template, failure), suggestion: current, notes };
        } catch (error) { failure = String(error.message || error); }
    }
    return { ok: false, error: message + literalNameHint(template, message), notes };
}

module.exports = { loadPlugin, renderPreview, validateTemplate };
if (require.main === module) {
    (async () => {
        let input = ''; for await (const chunk of process.stdin) input += chunk;
        const request = JSON.parse(input);
        if (process.argv[2] === '--validate-template') {
            const plugin = loadPlugin(process.argv[3], process.argv[4]);
            const result = await validateTemplate(plugin, request.template, request.fields || []);
            process.stdout.write(JSON.stringify(result));
            if (!result.ok) process.exitCode = 1;
            return;
        }
        const plugin = loadPlugin(process.argv[2], process.argv[3]);
        const response = await renderPreview(plugin, request);
        process.stdout.write(JSON.stringify(response));
    })().catch(error => { console.error(error.stack || String(error)); process.exitCode = 1; });
}
