// Offline view integration: use the real panel and local Graphviz bridge, no CDN.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
(async()=>{const b=await chromium.launch({headless:true,executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});try{
 const p=await b.newPage();
 await p.route('**/*',async r=>{const u=new URL(r.request().url());if(u.hostname!=='127.0.0.1')return r.abort();if(u.pathname==='/dependency-harness')return r.fulfill({contentType:'text/html',body:'<div id="host"></div>'});return r.continue();});
 await p.goto('http://127.0.0.1:8080/dependency-harness');
 await p.evaluate(async()=>{const {installLayerPanel}=await import('/layer-panel.js');window.panel=installLayerPanel(document.querySelector('#host'),{
 post:(path,data)=>fetch('/api/'+path,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(data)}),
 mountSvg:(text,host)=>{host.innerHTML=text;return host.querySelector('svg');},editor:{setValue:()=>{}}});});
 const root='out/cs-composition-final';const manifest=JSON.parse(fs.readFileSync(root+'/rounds/manifest.json'));
 const frame=JSON.parse(fs.readFileSync(root+'/rounds/'+manifest.at(-1).stem+'.json'));
 await p.evaluate(f=>window.panel.receive(f),frame);
 await p.selectOption('#native-layer-kind','closed');
 await p.waitForSelector('#native-closed-view[data-ready=true] svg',{timeout:30000});
 const index=frame.closed.catalog.catalog.triggers.findIndex(t=>t.origin?.candidate_kind==='CCSS');assert(index>=0);
 await p.selectOption('#native-closed-instance',String(index));
 await p.selectOption('#native-closed-diagram','cs');
 await p.waitForSelector('#native-closed-view[data-ready=true] svg',{timeout:30000});
 assert.equal(await p.locator('#native-closed-view .node').count(),3);
 assert((await p.locator('#native-closed-view').textContent()).includes('CCSS'));
 await p.locator('#native-closed-view').screenshot({path:'out/cs-composition-final/cs-reference.png'});
 console.log('Offline CS references and selected CCSS DOT rendering passed');
}finally{await b.close();}})().catch(e=>{console.error(e);process.exit(1)});
