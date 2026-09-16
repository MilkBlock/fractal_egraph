// Use the VS Code extension's modules, binaries, defaults and Graphviz runtime.
// No second AST-to-formula or AST-to-DOT implementation lives in this bridge.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');

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
    let ir;
    if (fs.existsSync(path.join(plugin.root, 'out/eggPreviewSource.js'))) {
        const { prepareEggPreviewPlan, buildEggPreview } = plugin.load('eggPreviewSource');
        const plan = prepareEggPreviewPlan(source, rust);
        const prepared = await buildEggPreview(plan, offset, extract);
        ir = prepared.decorate(await extract(prepared.rust, prepared.extractorOffset), prepared.rust);
    } else {
        const rustOffset = plugin.load('eggRuleMapping').resolveEggPreviewOffset(source, offset, rust);
        ir = await extract(rust, rustOffset);
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
    return {
        renderer: 'eggplant-pattern-vscode', renderer_revision: plugin.revision,
        config: { mode, label_style, recursive_strategy, pattern_renderer: patternRenderer },
        ir, math_view: mathView, typst: formulaSource,
        typst_document: formula.mode === 'math' ? core.buildTypstMathDocument(formulaSource) : core.buildTypstTextDocument(formulaSource),
        typst_svg: formula.svg, typst_mode: formula.mode,
        dot, dot_svg: graphSvg, typst_sources: typstSources, typst_renderings: typstRenderings,
    };
}

module.exports = { loadPlugin, renderPreview };
if (require.main === module) {
    (async () => {
        let input = ''; for await (const chunk of process.stdin) input += chunk;
        const request = JSON.parse(input);
        const plugin = loadPlugin(process.argv[2], process.argv[3]);
        const response = await renderPreview(plugin, request);
        process.stdout.write(JSON.stringify(response));
    })().catch(error => { console.error(error.stack || String(error)); process.exitCode = 1; });
}
