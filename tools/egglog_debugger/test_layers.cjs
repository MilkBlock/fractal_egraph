// NODE_PATH=<node_modules> node tools/egglog_debugger/test_layers.cjs [URL] [Chrome]
const assert=require('node:assert/strict');const path=require('node:path');const fs=require('node:fs');const {chromium}=require('playwright');
(async()=>{
 const root=path.resolve(__dirname,'../..'),url=process.argv[2]||'http://127.0.0.1:8080/';
 const browser=await chromium.launch({headless:true,...(process.argv[3]?{executablePath:process.argv[3]}:{})});
 try {
  const page=await browser.newPage({viewport:{width:1500,height:1100}});const errors=[];page.on('pageerror',e=>errors.push(String(e)));
  await page.goto(url,{waitUntil:'networkidle'});await page.waitForSelector('#native-layer-panel');
  const source=fs.readFileSync(path.join(root,process.argv[4]||'experiments/bake/increment-3.egg'),'utf8');
  await page.evaluate(s=>document.querySelector('.CodeMirror').CodeMirror.setValue(s),source);
  await page.click('#native-run');await page.waitForFunction(()=>!document.querySelector('#native-run').disabled&&window.egglogNative.layerSnapshots.length===8,{},{timeout:90000});
  const frames=await page.evaluate(()=>window.egglogNative.layerSnapshots);assert.equal(frames.length,8);
  for(const f of frames){assert.equal(f.preview_source,source);for(const kind of ['layers','fractals','coverage']){const file=path.join(root,f.artifact_directory,'rounds',f.stem+'.'+kind+'.dot');assert.equal(fs.readFileSync(file,'utf8'),f.dots[kind]);}}
  await page.selectOption('#native-layer-kind','fractals');await page.selectOption('#native-layer-scope','f0');
  await page.click('#native-layer-render');await page.waitForSelector('#native-layer-viewport[data-ready=true] svg',{timeout:60000});
  assert.equal(await page.locator('#native-layer-error').innerText(),'');
  await page.selectOption('#native-layer-format','typst');await page.click('#native-layer-render');
  await page.waitForSelector('#native-layer-viewport[data-ready=true] svg',{timeout:90000});
  assert.equal(await page.locator('#native-layer-error').innerText(),'');
  const renderer=page.locator('#native-layer-viewport [data-renderer]').first();assert((await renderer.getAttribute('data-renderer')).length>0);
  assert((await page.locator('#native-layer-code').textContent()).includes('Depth'),JSON.stringify({code:await page.locator('#native-layer-code').textContent(),iteration:await renderer.getAttribute('data-iteration')}));
  const downloadPromise=page.waitForEvent('download');await page.click('#native-layer-download');const download=await downloadPromise;assert(download.suggestedFilename().endsWith('.fractals.dot'));
  const exportPromise=page.waitForEvent('download');await page.click('#native-export');const exported=await exportPromise;const data=JSON.parse(fs.readFileSync(await exported.path(),'utf8'));assert.equal(data.version,3);assert.equal(data.layer_snapshots.length,8);
  await page.setInputFiles('#native-import',{name:'layer-history.json',mimeType:'application/json',buffer:Buffer.from(JSON.stringify(data))});assert.equal(await page.locator('#native-layer-round option').count(),8);
  await page.fill('#native-layer-directory',frames[0].artifact_directory);await page.click('#native-layer-load');await page.waitForFunction(()=>!document.querySelector('#native-layer-load').disabled&&document.querySelector('#native-layer-round').options.length===8);
  await page.selectOption('#native-layer-kind','layers');await page.selectOption('#native-layer-scope','s0');await page.selectOption('#native-layer-format','dot');await page.click('#native-layer-render');await page.waitForSelector('#native-layer-viewport[data-ready=true] svg',{timeout:60000});
  const image=path.join(root,frames[0].artifact_directory,'tools-layer-page.png');await page.screenshot({path:image});
  assert.deepEqual(errors,[]);console.log(JSON.stringify({url,frames:frames.length,artifact_directory:frames[0].artifact_directory,renderer:await renderer.count()?await renderer.getAttribute('data-renderer'): 'checked before switching',screenshot:image,errors}));
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exit(1)});
