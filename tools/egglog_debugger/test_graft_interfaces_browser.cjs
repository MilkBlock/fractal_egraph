// Uses the actual native probe output, not a hand-built graph or certificate.
// First run experiments/cell_spine/graft_diamond/probe.py --output <run>.
const assert=require('node:assert/strict'),fs=require('node:fs');
const {chromium}=require('playwright');
(async()=>{
 const run=process.argv[2]||'out/graft-interface-repair/left-right';
 const browser=await chromium.launch({headless:true,executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});
 try{
  const page=await browser.newPage({viewport:{width:1450,height:1050}}),errors=[];
  page.on('pageerror',e=>errors.push(e.message));
  const q=JSON.parse(fs.readFileSync(run+'/analysis.json')).saturated_rule_composition;
  const window=q.cs.windows.find(w=>JSON.stringify(w.members)==='[0,2]');
  assert.ok(window,'real independent window exists');
  await page.goto(`http://127.0.0.1:8080/cell-viewer.html?layer_run=${encodeURIComponent(run)}&cell_unit=${window.unit}`);
  const panel=page.locator('#native-cell-egraph');
  await panel.locator('[data-role=overview][data-ready=true] svg').waitFor({timeout:60000});
  await panel.locator(`[data-role=overview] a[*|href="#cell-interface-${window.unit}-0"]`).click();
  await panel.locator('[data-role=body][data-ready=true] svg').waitFor();
  const proof=JSON.parse(await panel.locator('[data-role=route-evidence]').textContent());
  assert.equal(proof.parent_bindings[0].parent,0);
  assert.ok(proof.external_injections.some(s=>s.facts.includes('(GateR (RipenInput 1))')));
  assert.ok(!proof.ordered_replay.some(s=>s.includes('(Left ')));
  assert.ok(await panel.locator('[data-role=route-proof]').evaluate(e=>e.open));
  const response=await page.request.post('http://127.0.0.1:8080/api/cell-egraph-state',{data:{path:run,unit:window.unit}});
  const detail=await response.json();
  assert.deepEqual(detail.graft_interfaces[0].lhs_evidence.map(e=>e.record),[2]);
  fs.mkdirSync('output/playwright',{recursive:true});
  await page.screenshot({path:'output/playwright/graft-interfaces.png'});
  await page.goto(`http://127.0.0.1:8080/?layer_run=${encodeURIComponent(run)}&layer_kind=cells&cell_unit=${window.unit}`);
  await panel.locator('[data-role=body][data-ready=true] svg').waitFor();
  await panel.locator('[data-role=typst-button]').click();
  await panel.locator('[data-role=typst][data-ready=true] svg').waitFor();
  assert.match(await panel.locator('[data-role=source]').textContent(),/Graft/);
  await panel.locator('[data-role=path-mode]').selectOption('history');
  await panel.locator('[data-role=typst-button]').click();
  await panel.locator('[data-role=typst][data-ready=true] svg').waitFor();
  assert.match(await panel.locator('[data-role=source]').textContent(),/History/);
  await panel.locator('[data-role=path-mode]').selectOption('interfaces');
  await panel.locator('[data-role=body][data-ready=true] svg').waitFor();
  assert.equal(await panel.locator('[data-role=error]').innerText(),'');assert.deepEqual(errors,[]);
  console.log('Native graft: base/right edge, typed binding, no left leakage, history mode and Typst passed');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exit(1);});
