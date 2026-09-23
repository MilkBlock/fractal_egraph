const assert=require('node:assert/strict');
const {chromium}=require('playwright');
(async()=>{
 const browser=await chromium.launch({headless:true,executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});
 try{
  const page=await browser.newPage();const errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.route('**/*',r=>{const u=new URL(r.request().url());if(u.hostname!=='127.0.0.1')return r.abort();if(u.pathname==='/reentry-harness')return r.fulfill({contentType:'text/html',body:'<div id="host"></div>'});return r.continue();});
  await page.goto('http://127.0.0.1:8080/reentry-harness');
  await page.evaluate(async()=>{const {installLayerPanel}=await import('/layer-panel.js');window.panel=installLayerPanel(document.querySelector('#host'),{post:(path,data)=>fetch('/api/'+path,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(data)}),mountSvg:(text,host)=>{host.innerHTML=text;return host.querySelector('svg');},editor:{setValue:()=>{}}});});
  await page.evaluate(path=>window.panel.load(path),process.argv[2]||'out/reentry-bucket-length');
  const bucket=page.locator('#native-reentry-buckets');await bucket.waitFor();
  assert((await bucket.locator('summary').innerText()).includes('1 个成分桶'));
  await bucket.locator('summary').click();await bucket.getByRole('button',{name:'条件 0',exact:true}).click();
  const data=JSON.parse(await bucket.locator('pre').innerText());
  if(process.argv[3]==='growth')assert(data.condition.observations.some(o=>o.admission==='growth_observation'&&o.starting_condition_resolved===false));
  else assert(data.original_conditions[0].condition.values.some(v=>v.literal==='3'));
  assert(data.condition.abstract_condition.values.every(v=>v.literal===null));
  assert.deepEqual(errors,[]);console.log('Reentry bucket, minimal condition and original literal evidence passed');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exit(1)});
