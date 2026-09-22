const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
(async()=>{const browser=await chromium.launch({headless:true,executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});try{
 const p=await browser.newPage({viewport:{width:1250,height:1000}});const errors=[];p.on('pageerror',e=>errors.push(e.message));
 await p.route('**/*',async r=>{const u=new URL(r.request().url());if(u.hostname!=='127.0.0.1')return r.abort();if(u.pathname==='/coverage-harness')return r.fulfill({contentType:'text/html',body:'<div id="host"></div>'});return r.continue();});
 await p.goto('http://127.0.0.1:8080/coverage-harness');
 await p.evaluate(async()=>{const {installLayerPanel}=await import('/layer-panel.js');window.panel=installLayerPanel(document.querySelector('#host'),{post:(path,data)=>fetch('/api/'+path,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(data)}),mountSvg:(text,host)=>{host.innerHTML=text;return host.querySelector('svg');},editor:{setValue:()=>{}}});});
 assert.equal(await p.locator('#native-layer-kind').inputValue(),'saturated_rule_composition');
 const run=process.argv[2]||'out/ripen-cost-profile/math4-5';const manifest=JSON.parse(fs.readFileSync(run+'/rounds/manifest.json'));const frame=JSON.parse(fs.readFileSync(run+'/rounds/'+manifest.at(-1).stem+'.json'));
 await p.evaluate(f=>window.panel.receive(f),frame);await p.selectOption('#native-layer-kind','saturated_rule_composition');
 const panel=p.locator('#native-saturated-rule-composition-coverage');await panel.locator('svg').waitFor();assert((await panel.innerText()).includes('7/77'));assert.equal(await panel.locator('[data-state]').count(),4);
 await panel.locator('[data-state="3"]').click();assert.equal(await p.locator('#native-saturated-rule-composition').inputValue(),'3');
 const download=p.waitForEvent('download');await panel.getByText('下载 CSV',{exact:true}).click();assert.equal((await download).suggestedFilename(),'ripen-top100.csv');
 await panel.screenshot({path:'out/ripen-top100/chart.png'});
 // Direct catalog loading must use its own CS context, not a stale frame.
 await p.locator('#native-saturated-rule-composition-path').evaluate((el,v)=>el.value=v,run);
 await p.locator('#native-saturated-rule-composition-load').evaluate(el=>el.click());
 await p.waitForFunction(()=>document.querySelector('#native-saturated-rule-composition-load').disabled===false);
 assert((await panel.innerText()).includes('7/77'));assert.deepEqual(errors,[]);
 await p.evaluate(f=>{window.panel.reset();f.saturated_rule_composition.catalog=null;f.saturated_rule_composition.counts={Pending:5};window.panel.receive(f);},frame);
 await p.click('#native-layer-render');assert((await p.locator('#native-saturated-rule-composition-status').innerText()).includes('Pending 5'));
 assert((await panel.innerText()).includes('已验证 0/77'));
 await p.fill('#native-layer-directory',run);await p.click('#native-layer-load');
 await p.waitForFunction(()=>!document.querySelector('#native-layer-load').disabled);
 assert.equal(await p.locator('#native-layer-round option').count(),manifest.length);
 assert((await panel.innerText()).includes('7/77'));
 console.log('Live-frame ranking, click-through, CSV and direct catalog loading passed');
}finally{await browser.close();}})().catch(e=>{console.error(e);process.exit(1)});
