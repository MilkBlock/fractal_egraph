const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path');
const {chromium}=require('playwright');
(async()=>{
 const folder='out/cell-layout-test-'+process.pid;
 fs.mkdirSync(folder,{recursive:true});
 const data=JSON.parse(fs.readFileSync('out/cell-egraph-view-demo/analysis.json'));
 for(const j of data.saturated_rule_composition.jobs){const dest=path.join(folder,'saturated-rule-composition/cells',path.basename(j.folder));fs.cpSync(j.folder,dest,{recursive:true});j.folder=path.resolve(dest);}
 fs.copyFileSync('out/cell-egraph-view-demo/source.egg',path.join(folder,'source.egg'));
 fs.writeFileSync(path.join(folder,'analysis.json'),JSON.stringify(data));fs.mkdirSync(path.join(folder,'rounds'));
 const early=structuredClone(data.saturated_rule_composition);early.cell_egraph.nodes=early.cell_egraph.nodes.filter(n=>n.unit===0);early.cell_egraph.units=early.cell_egraph.units.filter(n=>n.unit===0);early.jobs=early.jobs.filter(j=>j.cell_unit===0);early.jobs.forEach(j=>j.state='Pending');
 fs.writeFileSync(path.join(folder,'rounds/manifest.json'),JSON.stringify([{stem:'round-0001',label:'test round 1'}]));
 fs.writeFileSync(path.join(folder,'rounds/round-0001.json'),JSON.stringify({saturated_rule_composition:early}));
 const browser=await chromium.launch({headless:true,executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});
 try{
 const page=await browser.newPage({viewport:{width:1400,height:1000}});
 const errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.goto(`http://127.0.0.1:8080/?layer_run=${folder}&cell_unit=1`);
 const panel=page.locator('#native-cell-egraph');await panel.locator('[data-role=body][data-ready=true] svg').waitFor({timeout:60000});
 await panel.locator('.cell-graph-pane > summary').click();await panel.locator('[data-role=overview][data-ready=true] svg').waitFor();
 const svg=await panel.locator('[data-role=overview]').innerHTML();
 await panel.locator('[data-role=fullscreen]').click();
 assert.equal(await panel.locator('[data-role=overview]').innerHTML(),svg,'fullscreen must reuse existing SVG');
 const box=await panel.boundingBox();assert.ok(box.x<=1&&box.y<=1&&box.width>=1390&&box.height>=990);
 await panel.locator('[data-role=overview] a').filter({hasText:'CoarseApply 1'}).click();
 await panel.locator('[data-role=rule-svg][data-ready=true] svg').waitFor({timeout:60000});
 assert.equal(await panel.getAttribute('data-focus'),'rule');
 await panel.locator('[data-role=overview] a').filter({hasText:'Cell e0'}).click();
 await page.waitForFunction(()=>document.querySelector('#native-cell-egraph').dataset.focus==='body');
 await panel.locator('[data-role=body][data-ready=true] svg').waitFor();
 await panel.locator('[data-role=snapshot]').selectOption('round-0001');
 await page.waitForFunction(()=>document.querySelector('#native-cell-egraph [data-role=guarantee]').textContent.includes('Pending'));
 assert.equal(await panel.locator('[data-role=class] option').count(),1);
 assert.equal(await panel.locator('[data-role=body] svg').count(),0,'pending snapshot must not show future saturated body');
 await page.keyboard.press('Escape');assert.ok(!(await panel.getAttribute('class')).includes('cell-fullscreen'));
 assert.deepEqual(errors,[]);console.log('Same SVG fullscreen, rule/cell clicks and historical snapshot isolation passed');
 }finally{await browser.close();fs.rmSync(folder,{recursive:true,force:true});}
})().catch(e=>{console.error(e);process.exit(1);});
