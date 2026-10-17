const assert=require('node:assert/strict');
const path=require('node:path');
const {chromium}=require('playwright');
(async()=>{
 const run=process.argv[2]||'out/analyze-cell-families-verified';
 const browser=await chromium.launch({headless:true,executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});
 try {
  const page=await browser.newPage({viewport:{width:1500,height:1050}}),errors=[];
  page.on('pageerror',e=>errors.push(String(e)));
  await page.goto('http://127.0.0.1:8080/cell-viewer.html?layer_run='+encodeURIComponent(run));
  const panel=page.locator('#native-cell-egraph');
  await panel.locator('[data-family="0"]').waitFor({timeout:60000});
  assert.equal(await panel.locator('[data-family]').count(),2);
  await panel.locator('[data-family="0"]').click();
  assert.match(await panel.locator('[data-role=family-evidence]').textContent(),/observed-reentry/);
  assert.match(await panel.locator('[data-role=family-samples]').textContent(),/event/);
  const member=panel.locator('[data-role=family-members] button').first();
  const label=await member.textContent();const unit=label.match(/unit (\d+)/)[1];
  await member.click();
  await page.waitForFunction(u=>document.querySelector('[data-role=unit]').value===u,unit);
  await panel.locator('[data-role=body][data-ready=true] svg').waitFor({timeout:60000});
  await panel.locator('[data-role=family-focus]').click();
  const members=await panel.locator('[data-role=family-members] button').count();
  await page.waitForFunction(n=>[...document.querySelectorAll('[data-role=overview] svg g.node title')]
    .filter(x=>/^u[0-9]+$/.test(x.textContent)).length===n,members);
  await panel.locator('[data-role=overview] a').filter({hasText:'观察重复族 F0'}).waitFor({timeout:30000});
  await panel.locator('[data-role=overview] a').filter({hasText:'观察重复族 F0'}).click();
  assert.equal(await panel.locator('[data-role=error]').textContent(),'');
  await page.screenshot({path:path.join(run,'viewer.jpg'),type:'jpeg'});
  await panel.locator('[data-role=snapshot]').selectOption('round-0001');
  await page.waitForFunction(()=>document.querySelector('[data-role=families]').textContent.includes('尚无'));
  assert.equal(await panel.locator('[data-family]').count(),0,'early snapshot must not borrow future families');
  assert.deepEqual(errors,[]);
  console.log('Family membership, graph links, Cell body selection and snapshot isolation passed');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exit(1);});
