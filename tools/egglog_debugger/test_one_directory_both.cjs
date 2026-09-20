// NODE_PATH=<node_modules> node tools/egglog_debugger/test_one_directory_both.cjs [URL] [Chrome] [DIR]
//
// One analysis directory must serve both views from the same address: the per-round
// layer/Fractal data and the ClosedState data live in the same round snapshots, so typing a
// single directory should never require a second path for ClosedState.
const assert = require('node:assert/strict');
const path = require('node:path');
const fs = require('node:fs');
const { chromium } = require('playwright');

(async () => {
  const root = path.resolve(__dirname, '../..');
  const url = process.argv[2] || 'http://127.0.0.1:8080/';
  const dir = process.argv[4] || 'out/tools-layers-view/math';
  if (!fs.existsSync(path.join(root, dir, 'catalog/catalog.json'))) {
    console.error(`missing fixture ${dir}/catalog/catalog.json; regenerate that run first`);
    process.exit(1);
  }
  const browser = await chromium.launch({
    headless: true, ...(process.argv[3] ? { executablePath: process.argv[3] } : {}),
  });
  try {
    const page = await browser.newPage({ viewport: { width: 1500, height: 1100 } });
    const errors = [];
    page.on('pageerror', e => errors.push(String(e)));
    await page.goto(`${url}?layer_run=${encodeURIComponent(dir)}`, { waitUntil: 'networkidle' });
    await page.waitForFunction(() => document.querySelector('#native-layer-round').options.length > 0,
                               null, { timeout: 60000 });

    // Layer / Fractal view from the same directory, with no path typed anywhere.
    await page.selectOption('#native-layer-kind', 'fractals');
    await page.click('#native-layer-render');
    await page.waitForSelector('#native-layer-viewport svg', { timeout: 60000 });
    assert.ok(await page.locator('#native-layer-viewport .node').count() > 0, 'fractal view must render');

    // ClosedState from that same directory.
    await page.selectOption('#native-layer-kind', 'closed');
    await page.waitForSelector('#native-closed-view[data-ready=true] svg', { timeout: 60000 });
    const states = await page.locator('#native-closed-state option').count();
    assert.ok(states > 1, `expected shared ClosedStates from the same directory, got ${states} options`);
    assert.match(await page.locator('#native-closed-status').textContent(), /个 Trigger → \d+ 个共享 ClosedState/);
    assert.equal(await page.locator('#native-closed-path').inputValue(), '',
                 'ClosedState must come from the loaded directory, not a second catalog path');

    assert.deepEqual(errors, []);
    console.log(`one directory serves Fractal and ClosedState: ${dir}`);
  } finally {
    await browser.close();
  }
})().catch(e => { console.error(e); process.exit(1); });
