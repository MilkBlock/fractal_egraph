// Offline view integration: use the real panel and local Graphviz bridge, no CDN.
const fs=require('node:fs'),assert=require('node:assert/strict'),{chromium}=require('playwright');
(async()=>{const b=await chromium.launch({headless:true,executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});try{
 const p=await b.newPage();
 await p.route('**/*',async r=>{const u=new URL(r.request().url());if(u.hostname!=='127.0.0.1')return r.abort();if(u.pathname==='/dependency-harness')return r.fulfill({contentType:'text/html',body:'<div id="host"></div>'});return r.continue();});
 await p.goto('http://127.0.0.1:8080/dependency-harness');
 await p.evaluate(async()=>{const {installLayerPanel}=await import('/layer-panel.js');window.panel=installLayerPanel(document.querySelector('#host'),{
 post:(path,data)=>fetch('/api/'+path,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(data)}),
 mountSvg:(text,host)=>{host.innerHTML=text;return host.querySelector('svg');},editor:{setValue:()=>{}}});});
 const root='out/dependency-candidates-math6';const manifest=JSON.parse(fs.readFileSync(root+'/rounds/manifest.json'));
 const frame=JSON.parse(fs.readFileSync(root+'/rounds/'+manifest.at(-1).stem+'.json'));
 await p.evaluate(f=>window.panel.receive(f),frame);
 await p.selectOption('#native-layer-kind','closed');
 await p.waitForSelector('#native-closed-view[data-ready=true] svg',{timeout:30000});
 assert((await p.locator('#native-closed-status').textContent()).includes('依赖候选 128'));
 assert((await p.locator('#native-closed-instance').textContent()).includes('依赖候选 D'));
 console.log('Offline dependency candidate panel and DOT rendering passed');
}finally{await b.close();}})().catch(e=>{console.error(e);process.exit(1)});
