const assert=require('node:assert/strict');
const path=require('node:path');
const {chromium}=require('playwright');
(async()=>{
 const run=process.argv[2]||'out/closure-port-progressions-final';
 const browser=await chromium.launch({headless:true,executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});
 try{
  const page=await browser.newPage({viewport:{width:1500,height:1050}}),errors=[];
  page.on('pageerror',e=>errors.push(String(e)));
  await page.goto('http://127.0.0.1:8080/cell-viewer.html?layer_run='+encodeURIComponent(run));
  const panel=page.locator('#native-cell-egraph');
  await panel.locator('[data-port-family="0"]').waitFor({timeout:60000});
  await panel.locator('[data-port-family="0"]').click();
  assert.match(await panel.locator('[data-role=port-summary]').textContent(),/2 种来源历史/);
  assert.match(await panel.locator('[data-role=port-summary]').textContent(),/两份父闭包的事实均保留/);
  assert.match(await panel.locator('[data-role=port-elements]').textContent(),/3 个窗口，4 个位置/);
  await panel.locator('[data-role=port-focus]').click();
  await page.waitForFunction(()=>[...document.querySelectorAll('[data-role=overview] svg g.node title')].filter(x=>/^w[0-9]+_/.test(x.textContent)).length===3);
  await panel.locator('[data-role=overview] a').filter({hasText:'窗口 i=0'}).click();
  await panel.locator('[data-role=body][data-ready=true] svg').waitFor({timeout:30000});
  assert.match(await panel.locator('[data-role=guarantee]').textContent(),/导出认证闭包/);
  const reuse=panel.locator('[data-role=reuse-table]').locator('..');
  await reuse.locator('summary').click();
  await panel.locator('[data-role=family-panel] > summary').click();
  assert.equal(await panel.locator('[data-role=error]').textContent(),'');
  await page.screenshot({path:path.join(run,'ports-viewer.jpg'),type:'jpeg'});
  await panel.locator('[data-role=snapshot]').selectOption('round-0001');
  await page.waitForFunction(()=>document.querySelector('[data-role=port-families]').textContent.includes('没有足够'));
  assert.equal(await panel.locator('[data-port-family]').count(),0);
  assert.deepEqual(errors,[]);
  console.log('Closure-backed coordinates, Cell inspection and snapshot isolation passed');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exit(1);});
