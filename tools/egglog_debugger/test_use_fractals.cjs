// Run after producing out/use-fractal-math6 with native history replay.
const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path');
const {chromium}=require('playwright');
(async()=>{const dir=process.argv[2]||'out/use-fractal-math6',root=path.resolve(__dirname,'../..');
 const data=JSON.parse(fs.readFileSync(path.join(root,dir,'rounds/round-0006.json'),'utf8'));
 assert(data.analysis.use_fractals.families.length>0);
 const b=await chromium.launch({headless:true,executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});
 try {const p=await b.newPage({viewport:{width:1500,height:1100}}),errors=[];p.on('pageerror',e=>errors.push(String(e)));
 await p.goto('http://127.0.0.1:8080/?layer_run='+encodeURIComponent(dir)+'&layer_kind=use_fractals',{waitUntil:'networkidle'});
 await p.waitForFunction(()=>document.querySelector('#native-layer-scope')?.options.length>1,{},{timeout:60000});
 await p.selectOption('#native-layer-scope','rf0');await p.click('#native-layer-render');
 await p.waitForSelector('#native-layer-viewport[data-ready=true] svg',{timeout:60000});
 await p.locator('#native-layer-viewport .node').first().click();
 assert((await p.locator('#native-layer-details').textContent()).length>10);
 await p.selectOption('#native-layer-format','typst');await p.click('#native-layer-render');
 await p.waitForSelector('#native-layer-viewport[data-ready=true] svg',{timeout:60000});
 assert.equal(await p.locator('#native-layer-error').textContent(),'');
 assert.equal(await p.locator('#native-layer-viewport [data-renderer]').first().getAttribute('data-renderer'),'eggplant-pattern-vscode');
 assert((await p.locator('#native-layer-details').textContent()).includes('unbounded_induction_unknown'));
 const pending=p.waitForEvent('download');await p.click('#native-layer-download');const download=await pending;
 assert(download.suggestedFilename().endsWith('.use_fractals.dot'));
 const temp=path.join(root,dir,'downloaded.dot');await download.saveAs(temp);
 assert.equal(fs.readFileSync(temp,'utf8'),fs.readFileSync(path.join(root,dir,'rounds/round-0006.use_fractals.dot'),'utf8'));
 fs.unlinkSync(temp);await p.locator('#native-layer-panel').screenshot({path:path.join(root,dir,'use-fractals.png')});
 assert.deepEqual(errors,[]);console.log(JSON.stringify({directory:dir,families:data.analysis.use_fractals.families.length,errors}));
 }finally{await b.close();}
})().catch(e=>{console.error(e);process.exit(1)});
