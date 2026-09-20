// NODE_PATH=<node_modules> node tools/egglog_debugger/test_reuse.cjs [URL] [Chrome]
const assert=require('node:assert/strict'),path=require('node:path'),fs=require('node:fs');const {chromium}=require('playwright');
(async()=>{const root=path.resolve(__dirname,'../..'),browser=await chromium.launch({headless:true,executablePath:process.argv[3]||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});try{
 const page=await browser.newPage({viewport:{width:1500,height:1100}}),errors=[];page.on('pageerror',e=>errors.push(String(e)));
 await page.goto(process.argv[2]||'http://127.0.0.1:8080/',{waitUntil:'networkidle'});await page.waitForSelector('#native-run',{timeout:90000});
 await page.evaluate(s=>document.querySelector('.CodeMirror').CodeMirror.setValue(s),fs.readFileSync(path.join(root,'tests/fixtures/reuse_coarse.egg'),'utf8'));
 await page.click('#native-run');await page.waitForFunction(()=>!document.querySelector('#native-run').disabled&&window.egglogNative.layerSnapshots.length>0,{},{timeout:60000});
 const frames=await page.evaluate(()=>window.egglogNative.layerSnapshots),last=frames.at(-1);assert(last.reuse.stats.active_uses>0);assert(last.reuse.stats.covered_events>0);
 for(const f of frames)assert.equal(fs.readFileSync(path.join(root,f.artifact_directory,'rounds',f.stem+'.reuse.dot'),'utf8'),f.dots.reuse);
 await page.selectOption('#native-layer-kind','reuse');await page.selectOption('#native-layer-scope',{index:1});await page.click('#native-layer-render');await page.waitForSelector('#native-layer-viewport[data-ready=true] svg',{timeout:60000});assert.equal(await page.locator('#native-layer-error').textContent(),'');
 await page.selectOption('#native-layer-format','typst');await page.click('#native-layer-render');await page.waitForSelector('#native-layer-viewport[data-ready=true] svg',{timeout:60000});assert.equal(await page.locator('#native-layer-error').textContent(),'');
 assert.equal(await page.locator('#native-layer-viewport [data-renderer]').first().getAttribute('data-renderer'),'eggplant-pattern-vscode');
 const pending=page.waitForEvent('download');await page.click('#native-layer-download');assert((await pending).suggestedFilename().endsWith('.reuse.dot'));
 await page.locator('#native-layer-panel').screenshot({path:path.join(root,last.artifact_directory,'reuse.png')});assert.deepEqual(errors,[]);console.log(JSON.stringify({stats:last.reuse.stats,directory:last.artifact_directory,errors}));
}finally{await browser.close();}})().catch(e=>{console.error(e);process.exit(1)});
