const assert=require('node:assert/strict');
const {chromium}=require('playwright');
(async()=>{
 const b=await chromium.launch({headless:true,executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});
 try {
  const p=await b.newPage(),errors=[];p.on('pageerror',e=>errors.push(String(e)));
  await p.goto('http://127.0.0.1:8080/',{waitUntil:'networkidle'});
  let source='(datatype Expr (V String) (A Expr) (B Expr) (C Expr) (D Expr))\n(rewrite (A x) (B x))\n(rewrite (B x) (C x))\n(rewrite (C x) (D x))\n';
  for(let i=0;i<16;i++)source+=`(A (V "v${i}"))\n`;
  source+='(run 5)\n';
  await p.evaluate(s=>document.querySelector('.CodeMirror').CodeMirror.setValue(s),source);
  await p.selectOption('#native-layer-kind','saturated_rule_composition');
  await p.click('#native-run');
  await p.waitForFunction(()=>!document.querySelector('#native-run').disabled&&window.egglogNative.layerSnapshots.length===5,{}, {timeout:90000});
  const frames=await p.evaluate(()=>window.egglogNative.layerSnapshots);
  assert(frames.every(f=>f.saturated_rule_composition));
  assert(frames.at(-1).saturated_rule_composition.counts.Saturated>0);
  assert(frames.at(-1).saturated_rule_composition.catalog.catalog.saturated_rule_compositions>0);
  const run=frames.at(-1).artifact_directory;
  await p.waitForSelector('#native-saturated-rule-composition-view[data-ready=true] svg',{timeout:30000});
  // The SaturatedRuleComposition view is populated from the data embedded in the round snapshots, so no
  // catalog is loaded by hand. The directory box holds the user's own input, never a
  // generated run path.
  assert.equal(await p.locator('#native-saturated-rule-composition-path').inputValue(),'');
  assert(await p.locator('#native-layer-round').isVisible());
  await p.selectOption('#native-layer-round','0');
  assert((await p.locator('#native-saturated-rule-composition-status').textContent()).includes('边界 1'));
  await p.selectOption('#native-layer-round','4');
  await p.waitForSelector('#native-saturated-rule-composition-view[data-ready=true] svg',{timeout:30000});
  await p.goto('http://127.0.0.1:8080/?layer_run='+encodeURIComponent(run)+'&layer_kind=saturated_rule_composition',{waitUntil:'networkidle'});
  await p.waitForSelector('#native-saturated-rule-composition-view[data-ready=true] svg',{timeout:30000});
  assert((await p.locator('#native-saturated-rule-composition-status').textContent()).includes('Saturated'));
  assert(await p.locator('#native-layer-source').isEnabled());
  await p.locator('#native-layer-source').click();
  assert((await p.evaluate(()=>document.querySelector('.CodeMirror').CodeMirror.getValue())).includes('(datatype Expr'));
  assert.equal(await p.locator('#native-saturated-rule-composition-path').inputValue(),''); // no manual catalog load
  assert.deepEqual(errors,[]);
  console.log('Native run -> bounded ripen -> per-round SaturatedRuleComposition -> saved-run reload passed: '+run);
 } finally {await b.close();}
})().catch(e=>{console.error(e);process.exit(1)});
