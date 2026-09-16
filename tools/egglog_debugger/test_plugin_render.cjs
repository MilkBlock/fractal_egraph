// Compare the HTTP adapter's outputs against independent calls to the exact
// installed extension pipeline, including annotations and UTF-8 cursor mapping.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { loadPlugin, renderPreview } = require('./plugin-renderer.cjs');

async function main() {
    const root = path.resolve(process.argv[2]);
    const plugin = loadPlugin(root, process.argv[3]);
    const get = name => require(path.join(root, 'out', name + '.js'));
    const source = fs.readFileSync(path.join(__dirname, 'fixtures/render-parity.egg'), 'utf8');
    get('eggTranspiler').configureTranspilerResolution(root);
    const rust = await get('eggTranspiler').transpileEggSource(source);
    const extract = async (text, offset) => {
        const result = spawnSync(plugin.extractor, ['--offset', String(Buffer.byteLength(text.slice(0, offset)))], {input:text,encoding:'utf8'});
        assert.equal(result.status, 0, result.stderr);
        return JSON.parse(result.stdout);
    };
    const { prepareEggPreviewPlan, buildEggPreview } = get('eggPreviewSource');
    const prepared = await buildEggPreview(prepareEggPreviewPlan(source, rust), source.indexOf('(rule'), extract);
    const ir = prepared.decorate(await extract(prepared.rust, prepared.extractorOffset), prepared.rust);
    assert.ok(ir.typst_templates.some(t => t.variant_name === 'Add'));
    assert.ok(Object.keys(ir.display_names).length || ir.nodes.some(n => n.id === 'left_value'));
    const formula = get('mathView').buildMathViewTypstSource(get('mathView').buildMathViewModel(ir, source));
    assert.ok(formula.startsWith('frac('));
    assert.ok(get('dot').collectTypstReplacementSources(ir,'combined','recursive','dag-expand').some(entry=>entry.source.includes('+')));
    for (const [mode, style, strategy] of [
        ['combined','recursive','dag-expand'], ['combined','recursive','tree-safe'],
        ['pattern','full','dag-expand'], ['action','compact','dag-expand'],
    ]) {
        const sources = Object.fromEntries(get('dot').collectTypstReplacementSources(ir,mode,style,strategy).map(e=>[e.targetId,e.source]));
        sources['math-view:'+ir.math_view.rule_name] = formula;
        const rendered = await get('typst').renderTypstSnippets(Object.entries(sources).map(([targetId,source])=>({targetId,source})));
        let expectedDot = get('dot').patternIrToDotWithMode(ir,mode,style,strategy,rendered);
        if(mode==='pattern' && fs.existsSync(path.join(root,'out/rendererWasm.js'))){
            try{const renderer=await get('rendererWasm').loadRendererWasm(root);expectedDot=JSON.parse(renderer.render_pattern_json(JSON.stringify(ir))).dot || expectedDot;}catch{}
        }
        const actual = await renderPreview(plugin,{source,line:5,mode,label_style:style,recursive_strategy:strategy});
        assert.deepEqual(actual.ir, ir);
        assert.equal(actual.typst, formula);
        assert.equal(actual.typst_mode, 'math');
        assert.equal(actual.typst_document,get('shared/typstCore').buildTypstMathDocument(formula));
        assert.equal(actual.typst_svg, rendered['math-view:'+ir.math_view.rule_name].svg);
        assert.equal(actual.dot,expectedDot);
        assert.equal(actual.dot_svg,await get('svg').dotToSvg(expectedDot));
        assert.deepEqual(actual.typst_renderings,rendered);
        console.log(`PASS exact plugin parity: ${mode} / ${style} / ${strategy}`);
    }
    const zero = await renderPreview(plugin,{source,line:8});
    assert.equal(zero.math_view.rule_name,'zero');
    assert.equal(zero.ir.math_view.conclusions[0].to.target_id,'x');
    assert.notEqual(zero.typst,formula);
    console.log('PASS rewrite alias and second-rule selection');
}
main().catch(error=>{console.error(error);process.exitCode=1;});
