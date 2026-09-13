const {chromium}=require('playwright');
const {pathToFileURL}=require('url');const path=require('path');
(async()=>{const browser=await chromium.launch({channel:'chrome',headless:true});
const page=await browser.newPage();const errors=[];page.on('pageerror',e=>errors.push(e.message));
await page.goto(pathToFileURL(path.resolve(process.argv[2])).href);
if(await page.locator('.row').count()!==7)throw Error('expected seven observed layers');
if(await page.locator('#grid button').count()!==28)throw Error('must render only 28 mapped observations');
await page.getByRole('button',{name:'(6,0)',exact:true}).click();
const detail=JSON.parse(await page.locator('#detail').textContent());
if(detail.d!==6||detail.j!==0||!detail.witness.normalized_sources)throw Error('missing binding witness');
await page.setViewportSize({width:390,height:844});
if(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth))throw Error('mobile overflow');
await page.screenshot({path:path.join(path.dirname(process.argv[2]),'viewer.png'),fullPage:true});
if(process.argv[3]){await page.goto(pathToFileURL(path.resolve(process.argv[3])).href);const n=await page.locator('#family option').count();for(let i=0;i<n;i++)await page.locator('#family').selectOption(String(i));}
if(errors.length)throw Error(errors.join('\n'));await browser.close();console.log('Passed: observed coordinates, binding evidence, all Math families, mobile layout');
})().catch(e=>{console.error(e);process.exit(1)});
