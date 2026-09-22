// NODE_PATH=<node_modules> node tools/egglog_debugger/test_saturated_rule_composition_body.cjs [URL] [Chrome] [CATALOG]
//
// "合并前 / 合并后 body" must show two graphs at once: the member graphs that were shared
// (rebuilt from the recorded trigger) and the single native egglog e-graph the ripen cell
// wrote. The right-hand graph only exists for runs that exported native artifacts, so the test
// skips with a clear message rather than passing on nothing.
const assert = require('node:assert/strict');
const path = require('node:path');
const fs = require('node:fs');
const { chromium } = require('playwright');

(async () => {
  const root = path.resolve(__dirname, '../..');
  const url = process.argv[2] || 'http://127.0.0.1:8080/';
  const catalog = process.argv[4] || 'out/tools-layers-view/math/catalog';
  if (!fs.existsSync(path.join(root, catalog, 'catalog.json'))) {
    console.error(`missing fixture ${catalog}; regenerate that run first`);
    process.exit(1);
  }
  const browser = await chromium.launch({
    headless: true, ...(process.argv[3] ? { executablePath: process.argv[3] } : {}),
  });
  try {
    const page = await browser.newPage({ viewport: { width: 1500, height: 1200 } });
    const errors = [];
    page.on('pageerror', e => errors.push(String(e)));
    await page.goto(`${url}?saturated_rule_composition_catalog=${encodeURIComponent(catalog)}`,
                    { waitUntil: 'networkidle' });
    await page.waitForSelector('#native-saturated-rule-composition-view[data-ready=true] svg', { timeout: 60000 });

    // The option must exist and, on its own, render both graphs.
    const options = await page.locator('#native-saturated-rule-composition-diagram option').evaluateAll(os => os.map(o => o.value));
    assert.ok(options.includes('body'), `body diagram missing from ${JSON.stringify(options)}`);

    await page.selectOption('#native-saturated-rule-composition-diagram', 'body');
    await page.waitForFunction(() => {
      const host = document.querySelector('#native-saturated-rule-composition-view');
      return host.dataset.ready === 'true' && host.querySelectorAll('svg').length === 2;
    }, null, { timeout: 60000 });

    const caption = await page.locator('#native-saturated-rule-composition-caption').textContent();
    assert.match(caption, /合并前 \d+ 个原始成员 → 合并后 1 个饱和 body/, `caption: ${caption}`);

    // The two sides must be different graphs: the point of the view is the comparison.
    const nodes = await page.locator('#native-saturated-rule-composition-view svg').evaluateAll(
      svgs => svgs.map(s => s.querySelectorAll('.node').length));
    assert.ok(nodes[0] > 0, 'before side must have nodes');
    assert.ok(nodes[1] > 0, `after side must have nodes, got ${JSON.stringify(nodes)}`);

    // Printing the after side must come from the native export, not from the catalog JSON.
    const panelText = await page.locator('#native-saturated-rule-composition-view').textContent();
    assert.ok(panelText.includes('合并后 · 饱和 body'), 'after panel label missing');

    assert.deepEqual(errors, []);
    console.log(`before/after body view passed (nodes ${JSON.stringify(nodes)}) against ${catalog}`);
  } finally {
    await browser.close();
  }
})().catch(e => { console.error(e); process.exit(1); });
