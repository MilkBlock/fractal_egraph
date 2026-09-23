const fs = require('node:fs');
const assert = require('node:assert/strict');
const { chromium } = require('playwright');
(async () => {
  const run = process.argv[2] || 'out/core-reentry';
  const manifest = JSON.parse(fs.readFileSync(run + '/rounds/manifest.json'));
  const frame = JSON.parse(fs.readFileSync(run + '/rounds/' + manifest.at(-1).stem + '.json'));
  const browser = await chromium.launch({headless:true, executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});
  try {
    const page = await browser.newPage();
    const errors = [];
    page.on('pageerror', e => errors.push(e.message));
    await page.route('**/*', r => {
      const u = new URL(r.request().url());
      if (u.hostname !== '127.0.0.1') return r.abort();
      if (u.pathname === '/core-harness') return r.fulfill({contentType:'text/html', body:'<div id="host"></div>'});
      return r.continue();
    });
    await page.goto('http://127.0.0.1:8080/core-harness');
    await page.evaluate(async () => {
      const {installLayerPanel} = await import('/layer-panel.js');
      window.panel = installLayerPanel(document.querySelector('#host'), {
        post: (path,data) => fetch('/api/'+path, {method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(data)}),
        mountSvg: (text,host) => {host.innerHTML=text;return host.querySelector('svg');},
        editor:{setValue:()=>{}}
      });
    });
    await page.evaluate(f => window.panel.receive(f), frame);
    const weak = page.locator('#native-saturated-rule-composition-weak');
    await weak.waitFor();
    assert((await weak.innerText()).includes('未证明 infi / 耗散'));
    await page.locator('#native-saturated-rule-composition-coverage svg').waitFor();
    await page.evaluate(f => {
      const q=structuredClone(f.saturated_rule_composition);
      q.counts={Running:1};q.catalog=null;
      window.panel.progress(q);
    }, frame);
    assert((await page.locator('#native-saturated-rule-composition-status').innerText()).includes('Running 1'));
    await page.evaluate(f => window.panel.progress(f.saturated_rule_composition),frame);
    await page.locator('#native-saturated-rule-composition-coverage svg').waitFor();
    await page.fill('#native-layer-directory',run);
    await page.click('#native-layer-load');
    await page.waitForFunction(()=>!document.querySelector('#native-layer-load').disabled);
    assert.equal(await page.locator('#native-layer-round option').count(),manifest.length);
    assert.deepEqual(errors,[]);
    console.log('Core CS/ripen panel, live progress and directory loading passed');
  } finally {await browser.close();}
})().catch(e=>{console.error(e);process.exit(1)});
