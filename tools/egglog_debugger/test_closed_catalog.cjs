const assert=require('node:assert/strict');const{chromium}=require('playwright');
(async()=>{const b=await chromium.launch({headless:true,executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});try{
 const p=await b.newPage({viewport:{width:1400,height:1000}}),errors=[];p.on('pageerror',e=>errors.push(String(e)));
 await p.goto('http://127.0.0.1:8080/?closed_catalog=out/closed-unify/catalog',{waitUntil:'networkidle'});
 await p.waitForSelector('#native-closed-view svg',{timeout:60000});
 assert((await p.locator('#native-closed-status').textContent()).includes('2 个 Trigger → 1 个'));
 const pick=id=>p.locator('#native-closed-view .node').filter({has:p.locator('title',{hasText:new RegExp('^'+id+'$')})});
 await pick('t0').click();let d=JSON.parse(await p.locator('#native-closed-details').textContent());
 assert.equal(d.origin.use_id,24);assert.equal(d.binding_origin.parameters.length,3);assert.equal(d.value_map.length,10);
 await pick('t1').click();d=JSON.parse(await p.locator('#native-closed-details').textContent());assert.equal(d.origin.template,18);
 await pick('c0').click();d=JSON.parse(await p.locator('#native-closed-details').textContent());assert.equal(d.rows.length,15);
 await p.locator('#native-closed-view').screenshot({path:'out/closed-unify/catalog/preview.png'});
 assert.deepEqual(errors,[]);console.log('ClosedState graph and both trigger mappings passed');
}finally{await b.close();}})().catch(e=>{console.error(e);process.exit(1)});
