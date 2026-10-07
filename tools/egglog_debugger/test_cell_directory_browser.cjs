const assert=require('node:assert/strict');
const fs=require('node:fs');
const {chromium}=require('playwright');
(async()=>{
 const browser=await chromium.launch({headless:true,executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});
 try {
  const page=await browser.newPage();
  await page.goto('http://127.0.0.1:8080/?layer_run=out/cell-egraph-view-demo&layer_kind=cells');
  await page.locator('#native-layer-directory option[value="out/cell-egraph-view-demo"]').waitFor({state:'attached',timeout:60000});
  assert.equal(await page.locator('#native-layer-kind').count(),0);
  assert.equal(await page.locator('#native-saturated-rule-composition-diagram').count(),0);
  const source=fs.readFileSync('out/cell-egraph-view-demo/source.egg','utf8');
  await page.evaluate(s=>document.querySelector('.CodeMirror').CodeMirror.setValue(s),source);
  await page.waitForFunction(()=>document.querySelector('#native-layer-status').textContent.includes('当前源码有'),null,{timeout:30000});
  const entries=await page.evaluate(async source=>(await(await fetch('/api/analysis-directories',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({source})})).json()).directories,source);
  assert.equal(await page.locator('#native-layer-directory').inputValue(),entries.find(e=>e.matches_source).path);
  await page.evaluate(()=>document.querySelector('.CodeMirror').CodeMirror.setValue('(datatype NoSavedRun20261007 (Never))\n(Never) (run 1)\n; '+Date.now()));
  await page.waitForFunction(()=>document.querySelector('#native-layer-status').textContent.includes('尚无匹配'),null,{timeout:30000});
  assert.equal(await page.locator('#native-layer-directory').inputValue(),'');
  assert.equal(await page.locator('#native-cell-egraph').isVisible(),false);
  await page.locator('#native-run').click();
  await page.waitForFunction(()=>document.querySelector('#native-layer-directory').value.startsWith('out/debugger/'),null,{timeout:60000});
  assert.match(await page.locator('#native-layer-status').innerText(),/当前源码有/);
  console.log('Directory defaults follow current source, latest match and no-result clearing passed');
 } finally {await browser.close();}
})().catch(e=>{console.error(e);process.exit(1);});
