const assert=require('node:assert/strict');
const {chromium}=require('playwright');
(async()=>{
 const browser=await chromium.launch({headless:true,executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});
 try {
  const page=await browser.newPage({viewport:{width:1450,height:1100}}),errors=[];
  page.on('pageerror',e=>errors.push(String(e)));
  await page.goto('http://127.0.0.1:8080/?layer_run=out/cell-egraph-view-demo&layer_kind=cells&cell_unit=1');
  const panel=page.locator('#native-cell-egraph');
  await panel.locator('[data-role=body][data-ready=true] svg').waitFor({timeout:60000});
  assert.match(await panel.locator('[data-role=guarantee]').innerText(),/回放验证通过/);
  assert.match(await panel.locator('[data-role=body]').innerHTML(),/#ffc477/);
  await panel.locator('[data-role=typst-button]').click();
  await panel.locator('[data-role=typst][data-ready=true] svg').waitFor({timeout:30000});
  assert.equal(await panel.locator('[data-role=error]').innerText(),'');
  await panel.locator('[data-role=rule-preview]').click();
  await panel.locator('[data-role=rule-svg][data-ready=true] svg').waitFor({timeout:60000});
  await panel.screenshot({path:'out/cell-egraph-view-demo/path-viewer.png'});
  await panel.locator('[data-role=overview]').locator('..').locator('summary').click();
  await panel.locator('[data-role=overview][data-ready=true] svg').waitFor({timeout:30000});
  await panel.locator('[data-role=overview] a').filter({hasText:'Cell e0'}).first().click();
  await page.waitForFunction(()=>document.querySelector('#native-cell-egraph [data-role=unit]').value==='0');
  await panel.locator('[data-role=body][data-ready=true] svg').waitFor();
  await panel.screenshot({path:'out/cell-egraph-view-demo/viewer.png'});
  await page.goto('http://127.0.0.1:8080/?layer_run=out/eggcc-full-replay-20261004-stack16m/run&layer_kind=cells&cell_unit=1');
  await panel.locator('[data-role=body][data-ready=true] svg').waitFor({timeout:60000});
  assert.match(await panel.locator('[data-role=guarantee]').innerText(),/观察性快照/);
  assert.match(await panel.locator('[data-role=legend]').innerText(),/没有逐路径 LHS/);
  assert.deepEqual(errors,[]);
  console.log('Cell UI: selection, Typst, SVG, exact highlight and legacy guarantee labels passed');
 } finally {await browser.close();}
})().catch(e=>{console.error(e);process.exit(1);});
