import {installCellPanel} from './cell-egraph-panel.mjs';
async function post(path,body){const r=await fetch('/api/'+path,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)});if(!r.ok){let e;try{e=await r.json();}catch{e={error:r.statusText};}throw Error(e.error||'请求失败');}return r;}
function mountSvg(markup,host){
 const doc=new DOMParser().parseFromString(markup,'image/svg+xml');
 if(doc.querySelector('parsererror')||doc.documentElement.localName!=='svg')throw Error('无效 SVG');
 doc.querySelectorAll('script,foreignObject,style').forEach(n=>n.remove());
 for(const node of doc.querySelectorAll('*'))for(const attr of [...node.attributes])if(attr.name.toLowerCase().startsWith('on')||(attr.localName==='href'&&!attr.value.startsWith('#')&&!attr.value.startsWith('data:image/')))node.removeAttributeNode(attr);
 host.replaceChildren(document.importNode(doc.documentElement,true));
}
const view=installCellPanel(document.querySelector('#viewer'),{post,mountSvg,previewRow:async req=>(await post('preview',req)).json(),standalone:true});
const error=e=>{const text=e.message||String(e);document.querySelector('#viewer-error').textContent=text;view.panel.querySelector('[data-role=error]').textContent=text;};
const toolbar=view.panel.querySelector('[data-role=toolbar]');
const directory=document.createElement('select');directory.id='viewer-directory';directory.setAttribute('aria-label','分析目录');
const refresh=document.createElement('button');refresh.textContent='刷新';toolbar.prepend(directory,refresh);
async function load(path,round){if(!path)return;document.querySelector('#viewer-error').textContent='';view.panel.hidden=false;await view.load(path,round);const u=new URL(location.href);u.searchParams.set('layer_run',path);history.replaceState(null,'',u);}
try{
 const data=await(await post('analysis-directories',{source:''})).json();
 directory.add(new Option('选择分析目录',''));
 for(const r of data.directories)directory.add(new Option(r.path,r.path));
 const q=new URLSearchParams(location.search),path=q.get('layer_run');
 if(path){if(![...directory.options].some(o=>o.value===path))directory.add(new Option(path,path));directory.value=path;await load(path,q.get('cell_snapshot')||'');}
 else {view.panel.hidden=false;view.panel.querySelector('[data-role=status]').textContent='请选择分析目录。';}
}catch(e){error(e);}
directory.onchange=()=>load(directory.value,'').catch(error);
refresh.onclick=()=>load(directory.value).catch(error);
const channel=new BroadcastChannel('ripsat-rounds');
channel.onmessage=({data})=>{if(data.path===view.path){view.noteSnapshot({artifact_directory:data.path,stem:data.stem,label:data.stem});if(!view.snapshot)load(data.path).catch(error);}};
