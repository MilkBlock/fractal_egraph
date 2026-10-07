import {installCellPanel} from './cell-egraph-panel.mjs';
// Keep the public stream hooks; retired CS/Layer controls are no longer UI concepts.
export function installLayerPanel(host,{post,previewRow,mountSvg,editor}) {
  const panel=document.createElement('section');panel.id='native-layer-panel';
  panel.innerHTML=`<h3>RipSatCell</h3>
    <div><label>分析结果 <select id="native-layer-directory" aria-label="分析目录"><option value="">查找当前 .egg 的结果…</option></select></label>
    <button id="native-layer-load">刷新目录</button><button id="native-layer-render">显示</button><button id="native-layer-source">载入该次源码</button><button id="native-layer-download">下载图</button></div>
    <p id="native-layer-status" role="status"></p><p id="native-layer-error" role="alert"></p>`;
  host.append(panel);
  const $=name=>panel.querySelector('#native-layer-'+name);
  const view=installCellPanel(panel,{post,previewRow,mountSvg});
  const roundChannel=typeof BroadcastChannel==='undefined'?null:new BroadcastChannel('ripsat-rounds');
  let frames=[],source=null,refreshEpoch=0,lastDirectory='',running=false;
  let initial=new URLSearchParams(location.search).get('layer_run');
  if(new URLSearchParams(location.search).has('example'))initial=null;
  function clearSelection(){view.clear();$('source').disabled=true;$('render').disabled=true;$('download').disabled=true;}
  async function load(path){
    if(!path){clearSelection();return;}
    if(![...$('directory').options].some(o=>o.value===path))$('directory').add(new Option(path,path));
    $('directory').value=path;$('error').textContent='';
    view.panel.hidden=false;
    await view.load(path);lastDirectory=path;
    $('source').disabled=!view.source;$('render').disabled=false;$('download').disabled=false;
    const url=new URL(location.href);url.searchParams.set('layer_run',path);url.searchParams.set('layer_kind','cells');history.replaceState(null,'',url);
  }
  async function refresh(force=false,preferLatest=false){
    if(running)return;
    const current=editor.getValue();
    if(!force&&current===source)return;
    const token=++refreshEpoch,changed=current!==source;source=current;if(changed){lastDirectory='';clearSelection();}
    try{
      const data=await(await post('analysis-directories',{source:current})).json();if(token!==refreshEpoch)return;
      const previous=$('directory').value;
      $('directory').replaceChildren(new Option('请选择分析结果',''));
      const matching=document.createElement('optgroup');matching.label='当前 .egg';
      const other=document.createElement('optgroup');other.label='其他源码（手动选择）';
      for(const row of data.directories){const date=new Date(row.modified*1000).toLocaleString();(row.matches_source?matching:other).append(new Option(`${row.path} · ${date}`,row.path));}
      if(matching.children.length)$('directory').append(matching);if(other.children.length)$('directory').append(other);
      const matched=data.directories.filter(r=>r.matches_source);
      const preserveLink=Boolean(initial);
      const selected=initial||(!preferLatest&&!changed&&data.directories.some(r=>r.path===previous)?previous:matched[0]?.path)||'';initial=null;
      $('status').textContent=matched.length?`当前源码有 ${matched.length} 次分析，默认选择最近一次。`:'当前 .egg 尚无匹配的分析结果。点击「运行并识别」生成；也可手动查看其他源码的结果。';
      if(!selected){$('directory').value='';lastDirectory='';clearSelection();const u=new URL(location.href);u.searchParams.delete('layer_run');u.searchParams.delete('cell_unit');u.searchParams.delete('cell_eclass');history.replaceState(null,'',u);return;}
      if(selected!==lastDirectory||force){if(selected!==lastDirectory&&!preserveLink){const u=new URL(location.href);u.searchParams.delete('cell_unit');u.searchParams.delete('cell_eclass');history.replaceState(null,'',u);}await load(selected);}
      else $('directory').value=selected;
    }catch(e){if(token===refreshEpoch)$('error').textContent=e.message;}
  }
  $('directory').onchange=()=>{const url=new URL(location.href);url.searchParams.delete('cell_unit');url.searchParams.delete('cell_eclass');history.replaceState(null,'',url);load($('directory').value).catch(e=>$('error').textContent=e.message);};
  $('load').onclick=()=>refresh(true);
  $('render').onclick=()=>load($('directory').value).catch(e=>$('error').textContent=e.message);
  $('source').onclick=()=>{if(view.source)editor.setValue(view.source);};
  $('download').onclick=()=>view.download();
  function reset(){frames=[];running=true;refreshEpoch++;lastDirectory='';clearSelection();$('status').textContent='正在分析当前 .egg，结果生成后自动选中。';}
  function receive(frame){frames.push(frame);if(frame.artifact_directory)roundChannel?.postMessage({path:frame.artifact_directory,stem:frame.stem});if(frame.artifact_directory){const path=frame.artifact_directory;lastDirectory=path;view.noteSnapshot(frame);if(!view.snapshot||view.path!==path)load(path).catch(e=>$('error').textContent=e.message);}}
  function progress(q){$('status').textContent='Ripen · '+Object.entries(q.counts||{}).map(([k,v])=>`${k} ${v}`).join(' / ');}
  async function complete(){running=false;await refresh(true,true);}
  clearSelection();queueMicrotask(()=>refresh(true));
  return {receive,progress,reset,load,refresh,complete,get snapshots(){return structuredClone(frames);}};
}
