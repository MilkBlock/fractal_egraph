// NODE_PATH=<node_modules> node tools/egglog_debugger/test_stale_directory_url.cjs [URL] [Chrome]
//
// analyze writes a link of the form http://127.0.0.1:8080/?layer_run=<out dir> into the
// generated fractal.html. Opening it seeds the directory box and reloads that directory, and
// because the parameter stays in the address bar it did so on every refresh -- so a stale path
// looked like a hardcoded default that no other option could dislodge.
//
// The panel should load such a link once and then stop advertising it, while leaving a path the
// user typed themselves alone.
const assert = require('node:assert/strict');
const { chromium } = require('playwright');

const STALE = 'out/tools-layers-view/math';

(async () => {
  const url = process.argv[2] || 'http://127.0.0.1:8080/';
  const browser = await chromium.launch({
    headless: true, ...(process.argv[3] ? { executablePath: process.argv[3] } : {}),
  });
  try {
    const page = await browser.newPage();
    const errors = [];
    page.on('pageerror', e => errors.push(String(e)));

    // 1. A generated link still loads its directory.
    await page.goto(`${url}?layer_run=${encodeURIComponent(STALE)}&layer_kind=saturated_rule_composition`,
                    { waitUntil: 'networkidle' });
    await page.waitForFunction(() => document.querySelector('#native-layer-round').options.length > 0,
                               null, { timeout: 60000 });
    const frames = await page.evaluate(() => window.egglogNative.layerSnapshots.length);
    assert.ok(frames > 0, 'the link must still load its directory');

    // 2. ...but it is not left in the box or the URL to look like a default.
    assert.equal(await page.locator('#native-layer-directory').inputValue(), '',
                 'the seeded path must not stay in the box');
    assert.ok(!new URL(page.url()).searchParams.has('layer_run'),
              'the seeded parameter must be dropped so a refresh cannot re-seed it');

    // 3. A path the user typed is theirs: loading it must not be cleared.
    await page.selectOption('#native-layer-kind', 'layers');
    await page.fill('#native-layer-directory', STALE);
    await page.click('#native-layer-load');
    await page.waitForFunction(() => document.querySelector('#native-layer-round').options.length > 0,
                               null, { timeout: 60000 });
    assert.equal(await page.locator('#native-layer-directory').inputValue(), STALE,
                 'a typed path must be preserved');

    assert.deepEqual(errors, []);
    console.log('stale layer_run link loads once and is not left as a default');
  } finally {
    await browser.close();
  }
})().catch(e => { console.error(e); process.exit(1); });
