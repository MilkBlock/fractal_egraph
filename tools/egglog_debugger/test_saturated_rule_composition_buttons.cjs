// NODE_PATH=<node_modules> node tools/egglog_debugger/test_saturated_rule_composition_buttons.cjs [URL] [Chrome] [CATALOG]
//
// "显示" and "下载当前 SaturatedRuleComposition DOT" used to return silently in SaturatedRuleComposition mode when no
// catalog was loaded, and pressing 显示 overwrote whatever load error was on screen -- so a
// failed load looked like a dead button. These are the behaviours guarded here.
const assert = require('node:assert/strict');
const path = require('node:path');
const fs = require('node:fs');
const { chromium } = require('playwright');

// A directory that exists but has no catalog/ and no `closed` key in any round snapshot:
// it was produced before the SaturatedRuleComposition pipeline. out/tools-layers-view/math was regenerated
// with the current build, so the preserved pre-pipeline copy is used here.
const NO_CATALOG = 'out/tools-layers-view/math-prepipeline';

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
    const errors = [], downloads = [];
    page.on('pageerror', e => errors.push(String(e)));
    page.on('download', d => downloads.push(d.suggestedFilename()));
    const status = () => page.locator('#native-saturated-rule-composition-status').textContent();
    const settle = (ms = 700) => page.waitForTimeout(ms);

    await page.goto(url, { waitUntil: 'networkidle' });
    await page.selectOption('#native-layer-kind', 'saturated_rule_composition');
    await settle(400);

    // 1. SaturatedRuleComposition mode with nothing loaded explains itself, and both buttons keep saying it.
    assert.match(await status(), /尚无 SaturatedRuleComposition/);
    await page.click('#native-layer-render'); await settle();
    assert.match(await status(), /尚无 SaturatedRuleComposition/, '显示 must not go silent without a catalog');
    await page.click('#native-layer-download'); await settle();
    assert.match(await status(), /尚无 SaturatedRuleComposition/, '下载 must not go silent without a catalog');
    assert.deepEqual(downloads, [], '下载 must not fire without a catalog');

    // 2. A directory without catalog/ reports a clear reason...
    await page.fill('#native-layer-directory', NO_CATALOG);
    await page.click('#native-layer-load'); await settle(1500);
    const failure = await status();
    assert.match(failure, /没有 catalog\.json/, `expected a clear load failure, got: ${failure}`);
    assert.doesNotMatch(failure, /Errno/, 'raw errno text must not reach the user');

    // ...and 显示/下载 must not erase it: that is what made the buttons look dead.
    await page.click('#native-layer-render'); await settle();
    assert.match(await status(), /没有 catalog\.json/, '显示 erased the load error');
    await page.click('#native-layer-download'); await settle();
    assert.match(await status(), /没有 catalog\.json/, '下载 erased the load error');

    // 3. A real catalog renders and downloads.
    await page.fill('#native-layer-directory', catalog);
    await page.click('#native-layer-load'); await settle(2500);
    assert.match(await status(), /个 Trigger/);
    assert.ok(await page.locator('#native-saturated-rule-composition-view .node').count() > 0, 'catalog must render');
    await page.click('#native-layer-download'); await settle(900);
    assert.deepEqual(downloads, ['saturated-rule-composition.dot']);

    // 4. An analysis directory whose per-round snapshots predate the SaturatedRuleComposition pipeline
    //    must say so, not tell the user to run something they already ran. This is the exact
    //    path: load a layer directory, then switch the kind to SaturatedRuleComposition.
    await page.selectOption('#native-layer-kind', 'layers');
    await page.fill('#native-layer-directory', NO_CATALOG);
    await page.click('#native-layer-load'); await settle(2000);
    await page.waitForFunction(() => document.querySelector('#native-layer-round').options.length > 0,
                               null, { timeout: 30000 });
    await page.selectOption('#native-layer-kind', 'saturated_rule_composition'); await settle(600);
    const stale = await status();
    assert.match(stale, /每轮快照里没有 SaturatedRuleComposition 数据/, `expected the stale-snapshot hint, got: ${stale}`);
    assert.doesNotMatch(stale, /请先「运行并识别」/, 'a loaded directory must not be told to run first');

    assert.deepEqual(errors, []);
    console.log(`SaturatedRuleComposition buttons passed (no catalog / bad directory / stale snapshots / real catalog) against ${catalog}`);
  } finally {
    await browser.close();
  }
})().catch(e => { console.error(e); process.exit(1); });
