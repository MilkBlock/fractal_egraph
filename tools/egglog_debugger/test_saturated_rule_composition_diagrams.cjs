// NODE_PATH=<node_modules> node tools/egglog_debugger/test_saturated_rule_composition_diagrams.cjs [URL] [Chrome] [CATALOG]
//
// Covers the SaturatedRuleComposition *graph selector* (source rule comb / closed e-graph / source
// Coarse-Smooth layers). test_saturated_rule_composition_catalog.cjs cannot reach that code: its fixture
// (out/saturated-rule-composition-unify/catalog) has 0 comb_groups, so renderCatalog skips closedDiagram()
// entirely and only the top-level catalog.dot is ever mounted.
//
// Regression guarded here: switching to "closed e-graph" or "source layers" while the state
// selector is still on the overview used to render an empty canvas, which reads as "the
// selector did nothing". closedDiagram() now falls back to the first SaturatedRuleComposition.
const assert = require('node:assert/strict');
const path = require('node:path');
const fs = require('node:fs');
const { chromium } = require('playwright');

(async () => {
  const root = path.resolve(__dirname, '../..');
  const url = process.argv[2] || 'http://127.0.0.1:8080/';
  const catalog = process.argv[4] || 'out/saturated-rule-composition-integrated-math6/catalog';
  if (!fs.existsSync(path.join(root, catalog, 'catalog.json'))) {
    console.error(`missing fixture ${catalog}; run the integrated analyze first`);
    process.exit(1);
  }
  const browser = await chromium.launch({
    headless: true, ...(process.argv[3] ? { executablePath: process.argv[3] } : {}),
  });
  try {
    const page = await browser.newPage({ viewport: { width: 1500, height: 1100 } });
    const errors = [];
    page.on('pageerror', e => errors.push(String(e)));

    await page.goto(`${url}?saturated_rule_composition_catalog=${catalog}`, { waitUntil: 'networkidle' });
    await page.waitForSelector('#native-saturated-rule-composition-view svg', { timeout: 60000 });

    const nodes = () => page.locator('#native-saturated-rule-composition-view .node').count();
    const caption = () => page.locator('#native-saturated-rule-composition-caption').textContent();
    const stateValue = () => page.locator('#native-saturated-rule-composition').inputValue();
    const waitCaption = text =>
      page.waitForFunction(t => document.querySelector('#native-saturated-rule-composition-caption').textContent.includes(t),
                           text, { timeout: 30000 });
    const setDiagram = async v => { await page.selectOption('#native-saturated-rule-composition-diagram', v); };

    // The selector itself must exist with the three documented graph kinds.
    assert.deepEqual(
      await page.locator('#native-saturated-rule-composition-diagram option').evaluateAll(os => os.map(o => o.value)),
      ['comb', 'egraph', 'layers']);

    // 1. default view: source rule comb
    assert.ok(await nodes() > 0, 'comb view should render nodes');
    assert.match(await caption(), /rule comb → SaturatedRuleComposition/);

    // 2. closed e-graph from the overview: must auto-select a state, not render nothing
    assert.equal(await stateValue(), '', 'precondition: overview selected');
    await setDiagram('egraph');
    await waitCaption('已自动选择');
    assert.ok(await nodes() > 0, 'e-graph must render from the overview');
    assert.equal(await stateValue(), '0', 'overview must fall back to the first state');
    assert.match(await caption(), /eclasses \/ \d+ constructor rows/);

    // 3. explicit state keeps working and is reflected in the caption
    await page.selectOption('#native-saturated-rule-composition', '1');
    await waitCaption('C1 ·');
    const c1 = await nodes();
    assert.ok(c1 > 0, 'explicit state must render');

    // 4. source layers for the selected state
    await setDiagram('layers');
    await waitCaption('原始 layer');
    assert.ok(await nodes() > 0, 'layer view must render nodes');
    assert.doesNotMatch(await caption(), /已自动选择/, 'auto note only applies to the fallback');

    // 5. back to the comb view
    await setDiagram('comb');
    await waitCaption('rule comb → SaturatedRuleComposition');
    assert.ok(await nodes() > 0, 'comb view must still render');

    assert.deepEqual(errors, []);
    console.log(`SaturatedRuleComposition diagram selector passed (comb / e-graph / layers) against ${catalog}`);
  } finally {
    await browser.close();
  }
})().catch(e => { console.error(e); process.exit(1); });
