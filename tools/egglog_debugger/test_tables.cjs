const assert=require('node:assert/strict'),{chromium}=require('playwright');
(async()=>{const b=await chromium.launch({headless:true,executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});try{
 const p=await b.newPage();await p.goto('http://127.0.0.1:8080/',{waitUntil:'networkidle'});
 let relation='(datatype E (Num i64) (Add E E) (Seed i64))\n(relation Marked (E))\n(rule ((= s (Seed n))) ((Add (Num n) (Num n))))\n(rule ((= e (Add a b))) ((Marked e)))\n';
 for(let i=1;i<=8;i++)relation+=`(Seed ${i})\n`;relation+='(run 3)';
 const scalar='(relation P (i64))\n(relation Q (i64))\n(function score (i64) i64 :merge (max old new))\n(P 2)\n(rule ((P x)) ((Q x) (set (score x) (+ x 1))))\n(run 2)\n(check (Q 2))\n(check (= (score 2) 3))';
 for(const [source,rounds] of [[relation,3],[scalar,2]]){
  await p.evaluate(s=>document.querySelector('.CodeMirror').CodeMirror.setValue(s),source);
  await p.click('#native-run');await p.waitForFunction(()=>!document.querySelector('#native-run').disabled,{}, {timeout:60000});
  const status=await p.locator('#native-status').textContent();assert(!status.includes('失败'),status);
  const frames=await p.evaluate(()=>window.egglogNative.layerSnapshots);assert.equal(frames.length,rounds);
  if(source===relation){assert(frames.at(-1).saturated_rule_composition.counts.Saturated>0);assert(frames.at(-1).saturated_rule_composition.catalog.states.some(s=>s.rows.some(r=>r.op==='Marked')));}
 }
 console.log('Relation closures and pure relation/function execution passed in browser');
}finally{await b.close();}})().catch(e=>{console.error(e);process.exit(1)});
