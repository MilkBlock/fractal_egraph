const {chromium}=require('playwright');const {pathToFileURL}=require('url');const path=require('path');
(async()=>{const browser=await chromium.launch({channel:'chrome',headless:true});const page=await browser.newPage();const errors=[];page.on('pageerror',e=>errors.push(e.message));
for(const [file,units,applies,pending] of [[process.argv[2],15,45,0],[process.argv[3],7,13,8]]){
 await page.goto(pathToFileURL(path.resolve(file)).href);
 if(await page.locator('.unit').count()!==units)throw Error('wrong observed unit count');
 if(await page.locator('button[data-event]').count()!==applies)throw Error('invented or missing apply');
 if(await page.locator('.pending').count()!==pending)throw Error('missing sparse frontier');
 await page.locator('button[data-event]').first().click();const detail=JSON.parse(await page.locator('#detail').textContent());if(!detail.outputs||!detail.row_effects)throw Error('missing fact evidence');
 await page.setViewportSize({width:390,height:844});if(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth))throw Error('mobile overflow');
}
for(const file of process.argv.slice(4)){
 await page.goto(pathToFileURL(path.resolve(file)).href);
 const count=await page.locator('#pattern option').count();
 for(let i=0;i<count;i++){
  await page.locator('#pattern').selectOption(String(i));
  if(!(await page.locator('#ranking').textContent()).includes('语义 dominance 尚未证明'))throw Error('missing dominance scope');
  await page.locator('button[data-event]').first().click();
  if(!JSON.parse(await page.locator('#detail').textContent()).outputs)throw Error('shared witness missing');
 }
}
if(errors.length)throw Error(errors.join('\n'));await browser.close();console.log('Passed: complete/partial extents, actual apply counts, fact inspector, mobile layout');
})().catch(e=>{console.error(e);process.exit(1)});
