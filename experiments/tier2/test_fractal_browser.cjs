// Run with Node and Playwright available through NODE_PATH; uses installed Chrome.
const {chromium}=require('playwright');
const assert=require('node:assert/strict');
const {pathToFileURL}=require('node:url');
const path=require('node:path');
(async()=>{
 const input=path.resolve(process.argv[2]||path.join(__dirname,'fractal.html'));
 const browser=await chromium.launch({channel:'chrome',headless:true});
 try{
  const page=await browser.newPage({viewport:{width:1440,height:1000}});const errors=[];
  page.on('pageerror',e=>errors.push(e.message));
  await page.goto(pathToFileURL(input).href);await page.waitForLoadState('networkidle');
  const data=await page.locator('#data').textContent().then(JSON.parse);
  assert.equal(await page.locator('.card').count(),data.lanes.length);
  assert.equal(await page.locator('.track button').count(),data.stats.applications+2*data.lanes.length);
  for(const lane of data.lanes){
   for(const key of ['trigger',...lane.events.map((_,i)=>String(i+1))]){
    await page.locator(`button[data-lane="${lane.id}"][data-step="${key}"]`).click();
    await page.waitForFunction(([id,k])=>location.hash==='#'+id+'/'+k,[lane.id,key]);
    const event=key==='trigger'?lane.trigger:lane.events[Number(key)-1];
    await page.waitForFunction(e=>document.getElementById('tags').textContent.includes('事件 '+e),event);
    assert.ok((await page.locator('#tags').textContent()).includes('事件 '+event));
    assert.equal(await page.locator('#source').textContent(),data.nodes[event].source);
   }
   await page.locator(`button[data-lane="${lane.id}"][data-step="future"]`).click();
   await page.waitForFunction(()=>!document.getElementById('future-box').hidden);
   assert.equal(await page.locator('#known-box').isVisible(),false);
  }
  await page.goto(pathToFileURL(input).href+'#chain_1/'+data.lanes[0].events.length);await page.waitForLoadState('networkidle');
  assert.ok((await page.locator('#tags').textContent()).includes(String(data.lanes[0].events.at(-1))));
  await page.screenshot({path:'/tmp/fractal-desktop.png',fullPage:true});
  await page.setViewportSize({width:390,height:844});
  assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
  await page.screenshot({path:'/tmp/fractal-mobile.png',fullPage:true});
  assert.deepEqual(errors,[]);console.log('Passed: every trigger/step, unverified tail, deep link, mobile width, no JS errors');
 }finally{await browser.close()}
})().catch(e=>{console.error(e);process.exitCode=1});
