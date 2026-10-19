const assert=require('node:assert/strict');
const {chromium}=require('playwright');
(async()=>{
 const root=process.argv[2]||'out/discovery-contract-verified';
 const browser=await chromium.launch({headless:true,executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});
 try{
  const page=await browser.newPage(),errors=[];
  page.on('pageerror',e=>errors.push(String(e)));
  async function open(name){
   await page.goto('http://127.0.0.1:8080/cell-viewer.html?layer_run='+encodeURIComponent(root+'/'+name));
   await page.waitForFunction(()=>document.querySelector('[data-role=discovery-accounting]')?.textContent.includes('✓'));
   return page.locator('[data-role=discovery-records]');
  }
  let records=await open('different-closures');
  assert.equal(await records.locator(':scope > details').count(),2);
  await records.locator('summary').first().click();
  assert.match(await records.textContent(),/两个来源没有认证为同一闭包/);
  assert.match(await records.textContent(),/查看两份来源闭包的差异/);
  assert.match(await records.textContent(),/未检查 共享端口/);
  await records.getByRole('button',{name:/查看目标 Cell/}).first().click();
  await page.locator('[data-role=body][data-ready=true] svg').waitFor();
  records=await open('disconnected');
  assert.equal(await records.locator(':scope > details').count(),0);
  assert.match(await records.textContent(),/不能从独立 Cell 推断相邻关系/);
  await page.getByLabel('发现诊断筛选').selectOption('all');
  assert.equal(await records.locator(':scope > details').count(),12);
  records=await open('branch');
  assert.equal(await records.locator(':scope > details').count(),3);
  await records.locator('summary').first().click();
  assert.match(await records.textContent(),/交互形成分叉/);
  await records.getByRole('button',{name:/查看端口族/}).first().click();
  assert.match(await page.locator('[data-role=port-elements]').textContent(),/branching：不生成数组坐标/);
  await page.route('**/api/cell-egraph',async route=>{
   const response=await route.fetch(),data=await response.json();
   delete data.repetition_families.port_progressions.discovery;
   await route.fulfill({response,json:data});
  });
  await page.goto('http://127.0.0.1:8080/cell-viewer.html?layer_run='+encodeURIComponent(root+'/positive'));
  await page.waitForFunction(()=>document.querySelector('[data-role=discovery-accounting]')?.textContent.includes('旧快照'));
  assert.equal(await page.locator('[data-role=discovery-records]').count(),0,'old report cannot invent diagnostics');
  assert.deepEqual(errors,[]);
  console.log('Negative-control guidance, filters, target links, branching and legacy reports passed');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exit(1);});
