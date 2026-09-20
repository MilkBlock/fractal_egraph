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
  await p.selectOption('#native-layer-kind','closed');
  await p.click('#native-run');
  await p.waitForFunction(()=>!document.querySelector('#native-run').disabled&&window.egglogNative.layerSnapshots.length===5,{}, {timeout:90000});
  const frames=await p.evaluate(()=>window.egglogNative.layerSnapshots);
  assert(frames.every(f=>f.closed));
  assert(frames.at(-1).closed.counts.Closed>0);
  assert(frames.at(-1).closed.catalog.catalog.closed_states>0);
  const run=frames.at(-1).artifact_directory;
  await p.waitForSelector('#native-closed-view[data-ready=true] svg',{timeout:30000});
  // The ClosedState view is populated from the data embedded in the round snapshots, so no
  // catalog is loaded by hand. The directory box holds the user's own input, never a
  // generated run path.
  assert.equal(await p.locator('#native-closed-path').inputValue(),'');
  assert(await p.locator('#native-layer-round').isVisible());
  await p.selectOption('#native-layer-round','0');
  assert((await p.locator('#native-closed-status').textContent()).includes('边界 1'));
  await p.selectOption('#native-layer-round','4');
  await p.waitForSelector('#native-closed-view[data-ready=true] svg',{timeout:30000});
  await p.goto('http://127.0.0.1:8080/?layer_run='+encodeURIComponent(run)+'&layer_kind=closed',{waitUntil:'networkidle'});
  await p.waitForSelector('#native-closed-view[data-ready=true] svg',{timeout:30000});
  assert((await p.locator('#native-closed-status').textContent()).includes('Closed'));
  assert(await p.locator('#native-layer-source').isEnabled());
  await p.locator('#native-layer-source').click();
  assert((await p.evaluate(()=>document.querySelector('.CodeMirror').CodeMirror.getValue())).includes('(datatype Expr'));
  assert.equal(await p.locator('#native-closed-path').inputValue(),''); // no manual catalog load
  assert.deepEqual(errors,[]);
  console.log('Native run -> bounded ripen -> per-round ClosedState -> saved-run reload passed: '+run);
 } finally {await b.close();}
})().catch(e=>{console.error(e);process.exit(1)});
