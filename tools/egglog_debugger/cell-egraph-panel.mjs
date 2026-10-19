import {renderDiscovery} from './cell-discovery.mjs';
// AND/OR construction choices, not an enumeration of all possible rewrite histories.
export function model(data) {
  const units=new Map(data.nodes.map(n=>[n.unit,n])),classes=new Map(),routes=new Map();
  for(const n of data.nodes){if(n.eclass==null)continue;if(!classes.has(n.eclass))classes.set(n.eclass,[]);classes.get(n.eclass).push(n);}
  for(const e of data.graft_interfaces||[]){if(!units.has(e.target)||e.parents.some(p=>!units.has(p)))continue;if(!routes.has(e.target))routes.set(e.target,[]);routes.get(e.target).push(e);}
  const degree=c=>classes.get(c)?.length||0;
  const rank=parents=>parents.reduce((sum,p)=>sum+degree(units.get(p)?.eclass),0);
  for(const es of routes.values())es.sort((a,b)=>rank(b.parents)-rank(a.parents)||JSON.stringify(a.parents).localeCompare(JSON.stringify(b.parents)));
  for(const ns of classes.values())ns.sort((a,b)=>rank(b.parents||[])-rank(a.parents||[])||a.unit-b.unit);
  const reuse=new Map((data.reuse_value?.cells||[]).map(r=>[r.eclass,r]));
  const families=new Map((data.repetition_families?.families||[]).map(f=>[f.id,f]));
  const progressions=data.repetition_families?.port_progressions||{};
  const portFamilies=new Map((progressions.families||[]).map(f=>[f.id,f]));
  const portInstances=new Map((progressions.instances||[]).map(i=>[i.id,i]));
  const m={units,classes,degree,reuse,routes,families,portFamilies,portInstances,mode:'interfaces',routeChoices:new Map()};
  m.route=n=>routes.get(n.unit)?.[m.routeChoices.get(n.unit)||0];
  m.parents=n=>[...(m.mode==='history'?(n.history_parents||n.parents||[]):(m.route(n)?.parents||[]))].sort((a,b)=>degree(units.get(b)?.eclass)-degree(units.get(a)?.eclass)||a-b);
  return m;
}
const q=JSON.stringify;
export function overviewDot(m){
  const lines=['digraph Cells {rankdir=TB; node [shape=box,style="rounded,filled",fillcolor="#e8eef9"];'];
  for(const [c,ns] of m.classes){const r=m.reuse.get(c);lines.push(`"c${c}" [label=${q(`Cell e${c}\n${ns.length} constructions${r?`\n${r.interface_instances} interface bindings`:''}`)},URL=${q(`#cell-class-${c}`)}];`);}
  for(const [c,ns] of m.classes)for(const n of ns){
    const window=(n.history_windows||[]).some(w=>w.new_entry);
    lines.push(`"u${n.unit}" [shape=ellipse,fillcolor=${q(window?"#e5def5":"#f8edcf")},label=${q(`CoarseApply ${n.contract}${window?"\n历史窗口":""}`)},URL=${q(`#cell-unit-${n.unit}`)}];`);
    lines.push(`"u${n.unit}" -> "c${c}";`);
    for(const p of n.history_parents||n.parents||[]){const parent=m.units.get(p);if(parent){const producer=(n.producer_parents||[]).includes(p);lines.push(`"c${parent.eclass}" -> "u${n.unit}" [style=dashed,color="#9299a6",label=${q(producer?'producer history':(n.context_parents||[]).includes(p)?'context history':'history')}];`);}}
    for(const [i,e] of (m.routes.get(n.unit)||[]).entries()){
      const id=`g${n.unit}_${i}`;
      lines.push(`"${id}" [shape=diamond,fillcolor="#dceee6",label="认证接口",tooltip=${q(`unit ${e.parents.join('+')} → ${n.unit}; see typed binding and obligations`)},URL=${q(`#cell-interface-${n.unit}-${i}`)}];`);
      for(const p of e.parents)lines.push(`"c${m.units.get(p).eclass}" -> "${id}" [color="#237450",label=${q(`unit ${p}`)}];`);
      lines.push(`"${id}" -> "c${c}" [color="#237450"];`);
    }
  }
  for(const f of m.families.values()){
    lines.push(`"family${f.id}" [shape=hexagon,fillcolor="#eee2ff",color="#8054ad",label=${q(`观察重复族 F${f.id}\n${f.occurrences.length} 次有来源的再入`)},URL=${q(`#cell-family-${f.id}`)}];`);
    for(const u of f.units||[])if(m.units.has(u))lines.push(`"family${f.id}" -> "u${u}" [style=dotted,arrowhead=none,color="#8054ad",constraint=false];`);
  }
  for(const f of m.portFamilies.values()){
    lines.push(`"ports${f.id}" [shape=hexagon,fillcolor="#d8f0f5",color="#23788c",label=${q(`闭包端口族 P${f.id}\n${f.witnesses.length} 次真实交互`)},URL=${q(`#cell-port-family-${f.id}`)}];`);
    for(const u of f.units||[])if(m.units.has(u))lines.push(`"ports${f.id}" -> "u${u}" [style=dotted,arrowhead=none,color="#23788c",constraint=false];`);
  }
  return lines.join('\n')+'\n}';
}
export function familyDot(m,id){
  const f=m.families.get(id);if(!f)return overviewDot(m);
  const units=new Set(f.units);
  return overviewDot(model({nodes:[...m.units.values()].filter(n=>units.has(n.unit)),
    graft_interfaces:[...m.routes.values()].flat().filter(e=>units.has(e.target)&&e.parents.every(p=>units.has(p))),
    repetition_families:{families:[f]}}));
}
export function portFamilyDot(m,id){
  const f=m.portFamilies.get(id);if(!f)return overviewDot(m);
  const lines=['digraph Ports {rankdir=TB; node [shape=box,style="rounded,filled",fillcolor="#e1f1f6"];'];
  for(const [ci,c] of f.components.entries()){
    if(c.status!=='observed-indexed-chain')continue;
    for(const p of c.coordinates){const i=m.portInstances.get(p.instance),unit=i?.units?.[0];if(unit==null)continue;const cell=m.units.get(unit);
      lines.push(`"w${ci}_${p.instance}" [label=${q(`窗口 i=${p.i} · Cell e${cell?.eclass}\n`+i.ports.map(x=>`port ${x.port} = X[${p.base+x.port}]`).join('\n'))},URL=${q(`#cell-port-unit-${unit}`)}];`);}
    for(const w of f.witnesses.filter(w=>c.instances.includes(w.from))){
      const nid=`e${ci}_${w.event}`;
      lines.push(`"${nid}" [shape=diamond,fillcolor="#f8edcf",label=${q(`event ${w.event}\n${w.rule}`)},URL=${q(`#cell-unit-${w.target_unit}`)}];`);
      lines.push(`"w${ci}_${w.from}" -> "${nid}"; "w${ci}_${w.to}" -> "${nid}";`);
      lines.push(`"w${ci}_${w.from}" -> "w${ci}_${w.to}" [style=dashed,arrowhead=none,color="#23788c",label=${q(`重叠 ${f.contract.width-f.contract.shift} 个端口`)}];`);
    }
  }
  return lines.join('\n')+'\n}';
}
export function bodyDot(state,highlight=[]){
  const selected=new Set(highlight),hidden=new Set(state.subsumed_rows||[]);
  const lines=['digraph Body {rankdir=TB; node [fontname="Helvetica",fontsize=11];'];
  for(const [i,v] of (state.values||[]).entries())lines.push(`"v${i}" [shape=ellipse,label=${q(v.literal??`${v.sort} · ${i}`)},color="#8893a8"];`);
  for(const [i,r] of (state.rows||[]).entries()){
    const color=selected.has(i)?'#ffc477':hidden.has(i)?'#eeeeee':'#dceee6';
    lines.push(`"r${i}" [shape=box,style=filled,fillcolor=${q(color)},label=${q(r.op)},tooltip=${q(selected.has(i)?'Selected coarse LHS':'Closure fact; per-node producer not inferred')}];`);
    lines.push(`"v${r.result}" -> "r${i}" [style=dashed,arrowhead=none];`);
    r.args.forEach((a,j)=>lines.push(`"r${i}" -> "v${a}" [label=${q(String(j))}];`));
  }
  return lines.join('\n')+'\n}';
}
export function pathTypst(m,root,choices){
  const seen=new Set(),rows=[];
  function visit(n){
    if(seen.has(n.unit))return;seen.add(n.unit);
    const parents=m.parents(n).map(id=>m.units.get(id)).filter(Boolean);
    for(const p of parents)visit(p);
    rows.push([parents.length?parents.map(p=>`C${p.eclass}@${p.unit}`).join(' + '):'NoParents',m.mode==='history'?`History ${n.contract}`:m.route(n)?`Graft ${n.contract}`:`Seed ${n.contract}`,`C${n.eclass}@${n.unit}`,n.unit]);
  }
  visit(root);
  const items=rows.map(([p,t,c,u])=>`[#box[$ upright(${q(p)}) arrow.r upright(${q(t)}) arrow.r $ #link(${q(`#cell-unit-${u}`)})[#text(${q(c)})]]]`);
  // Same local Typst renderer and upright/arrow convention as existing fractal lanes.
  return '#set page(width: auto, height: auto, margin: 12pt)\n#set text(size: 11pt)\n#stack(dir: ttb, spacing: 8pt,\n'+items.join(',\n')+'\n)';
}
export function installCellPanel(parent,{post,mountSvg,previewRow,standalone=false}){
  const panel=document.createElement('section');panel.id='native-cell-egraph';panel.hidden=true;
  panel.innerHTML=`<header data-role="toolbar"><button data-role="fullscreen">打开独立看图页</button><label>快照 <select data-role="snapshot"><option value="">最新（自动跟随）</option></select></label></header><p><b>RipSatCell egraph</b> · 点击状态查看内部图与生成方式</p>
    <div data-role="status"></div><label>Cell e-class <select data-role="class"></select></label>
    <details open><summary>复用价值 · 按去重接口绑定数排序</summary><p>绑定数保留历史版本，不消除对称性与后续 union 别名。不同 Cell 的覆盖可能重叠，不能直接相加。参考收益是假设实例各自存储完整闭包的描述模型，不是实测内存收益。</p><div data-role="reuse-table" style="overflow:auto"></div></details>
    <details open data-role="family-panel"><summary>Fractal 候选族 · 仅 analyze 历史证据</summary><p>紫色关系表示已观察到的重复依赖路径，不表示 Cell 判等、已证明无限展开或饱和。点击族可查看原始 binding、参数表达式及外部义务。</p><div data-role="families"></div><button data-role="family-focus" hidden>图中只看本族成员</button><button data-role="family-all" hidden>恢复完整图</button><div data-role="family-members"></div><div data-role="family-samples"></div><details><summary>完整事件、binding 与 effect 证据</summary><pre data-role="family-evidence" style="white-space:pre-wrap"></pre></details></details>
    <details open data-role="port-panel"><summary>闭包端口族 · 从接线恢复坐标</summary><p>仅使用已认证 RipSat 闭包和实际 producer 读取。坐标描述已观察到的有限结构，不声明任意长度数组或内存布局。</p><div data-role="port-families"></div><div data-role="port-summary"></div><button data-role="port-focus" hidden>查看端口推进图</button><button data-role="port-all" hidden>恢复完整 Cell 图</button><div data-role="port-elements"></div><details><summary>闭包证据、版本化读取与仿射访问</summary><pre data-role="port-evidence" style="white-space:pre-wrap"></pre></details></details>
    <details open data-role="discovery-panel"><summary>端口推进发现 · 为什么识别／为什么没识别</summary><div data-role="discovery"></div></details>
    <label>当前构造路径 <select data-role="unit"></select></label>
    <p data-role="guarantee" style="padding:10px;background:#edf1f7;color:#182338;border-radius:6px"></p>
    <details><summary>整个 Cell egraph（点击节点跳转）</summary><div data-role="overview"></div></details>
    <p>灰色虚线只表示观测历史；绿色菱形表示有条件的认证接口。端口绑定不同的前驱不会自动互换。</p>
    <label>路径含义 <select data-role="path-mode"><option value="interfaces">认证接口</option><option value="history">观测历史</option></select></label>
    <label>此 Cell 的认证接口 <select data-role="route"></select></label>
    <details data-role="route-proof"><summary>接口绑定、外部注入与有序检查</summary><pre data-role="route-evidence" style="white-space:pre-wrap"></pre></details>
    <div data-role="paths"></div><button data-role="typst-button">渲染当前路径 · Typst</button>
    <div data-role="typst"></div>
    <label>路径中的 apply <select data-role="member"></select></label><button data-role="rule-preview">规则公式 · 现有 Typst 模板</button><div data-role="rule-svg"></div>
    <details open><summary>展开当前 RipSat egraph</summary>
      <select data-role="body-kind"><option value="state">局部闭包</option><option value="initial">起始状态</option></select>
      <p data-role="legend"></p><div data-role="body"></div></details>
    <details><summary>阶段、coarse / smooth 应用与保证证据</summary><pre data-role="evidence" style="white-space:pre-wrap"></pre></details>
    <details><summary>当前 Typst / DOT 源码</summary><pre data-role="source" style="white-space:pre-wrap"></pre></details>
    <p data-role="error" role="alert"></p>`;
  parent.append(panel);
  const el=r=>panel.querySelector(`[data-role="${r}"]`);
  let data,m,path='',snapshot='',selected=null,detail=null,epoch=0;
  const graphPane=el('overview').parentElement,workspace=document.createElement('div'),inspector=document.createElement('div');
  workspace.className='cell-workspace';inspector.className='cell-inspector';graphPane.classList.add('cell-graph-pane');
  for(const child of [...panel.children])if(child!==el('toolbar')&&child!==graphPane)inspector.append(child);
  workspace.append(graphPane,inspector);panel.append(workspace);
  el('body').closest('details').dataset.section='body';
  if(standalone){panel.classList.add('cell-fullscreen');graphPane.open=true;el('fullscreen').hidden=true;}
  el('fullscreen').onclick=()=>{
    if(!path)return;
    const url=new URL('/cell-viewer.html',location.href);url.searchParams.set('layer_run',path);
    if(snapshot)url.searchParams.set('cell_snapshot',snapshot);
    if(selected)url.searchParams.set('cell_unit',selected.unit);
    window.open(url,'_blank','noopener');
  };
  const choices=new Map();
  function fail(e){el('error').textContent=e.message||String(e);}
  async function svg(kind,source,host,token=epoch){
    const text=await(await post('render',{kind,source})).text();if(token!==epoch)return;
    host.replaceChildren();mountSvg(text,host);host.dataset.ready='true';
    host.onclick=e=>{const link=e.target.closest('a');if(!link)return;const href=link.getAttribute('href')||link.getAttribute('xlink:href')||'';const portUnit=href.match(/#cell-port-unit-(\d+)$/);if(portUnit){e.preventDefault();panel.dataset.focus='body';el('body').closest('details').open=true;selectUnit(Number(portUnit[1]));return;}const ports=href.match(/#cell-port-family-(\d+)$/);if(ports){e.preventDefault();selectPortFamily(Number(ports[1]));return;}const family=href.match(/#cell-family-(\d+)$/);if(family){e.preventDefault();selectFamily(Number(family[1]));return;}const route=href.match(/#cell-interface-(\d+)-(\d+)$/);if(route){e.preventDefault();panel.dataset.focus='body';m.routeChoices.set(Number(route[1]),Number(route[2]));m.mode='interfaces';el('path-mode').value='interfaces';el('route-proof').open=true;selectUnit(Number(route[1]));return;}const match=href.match(/#cell-(class|unit)-(\d+)$/);if(match){e.preventDefault();if(match[1]==='class'){panel.dataset.focus='body';el('body').closest('details').open=true;selectClass(Number(match[2]));}else{panel.dataset.focus='rule';selectUnit(Number(match[2])).then(()=>{if(selected?.unit===Number(match[2]))renderRule();});}}};
  }
  function paths(){
    el('paths').replaceChildren();
    const draw=(node,host,ancestors)=>{
      const item=document.createElement('details'),label=document.createElement('summary');
      label.textContent=`e${node.eclass} · unit ${node.unit} · CoarseApply ${node.contract} · 入度 ${m.degree(node.eclass)}`;item.append(label);host.append(item);
      const button=document.createElement('button');button.textContent='查看这个 RipSat';button.onclick=()=>selectUnit(node.unit);item.append(button);
      if(ancestors.has(node.unit)){item.append(document.createTextNode(' 递归／共享引用，不重复展开'));return;}
      const selector=document.createElement('select');selector.setAttribute('aria-label',`e${node.eclass} 的构造选择`);
      // Only the root may choose another construction. A certificate names
      // concrete parent units and their wiring, not arbitrary eclass members.
      for(const n of (host===el('paths')?m.classes.get(node.eclass)||[]:[node]))selector.add(new Option(`unit ${n.unit} · 前驱 ${m.parents(n).join(', ')||'无'}`,n.unit));
      selector.disabled=host!==el('paths');
      selector.value=String(node.unit);item.append(selector);
      const nested=document.createElement('div');nested.style.marginLeft='18px';item.append(nested);
      const expand=()=>{nested.replaceChildren();const n=m.units.get(Number(selector.value));if(host===el('paths'))choices.set(node.eclass,n);for(const id of m.parents(n)){const p=m.units.get(id);if(p)draw(p,nested,new Set([...ancestors,node.unit]));}};
      selector.onchange=()=>{if(host===el('paths')){selectUnit(Number(selector.value));return;}expand();el('typst').replaceChildren();};item.ontoggle=()=>{if(item.open&&!nested.childNodes.length)expand();};
      if(host===el('paths'))item.open=true;
    };
    if(selected)draw(selected,el('paths'),new Set());
  }
  function routeEvidence(){
    const routes=m.routes.get(selected.unit)||[];el('route').replaceChildren();
    for(const [i,r] of routes.entries())el('route').add(new Option(`接口 ${i+1} · unit ${r.parents.join(' + ')} → ${selected.unit}`,i));
    el('route').disabled=!routes.length;el('route').value=String(m.routeChoices.get(selected.unit)||0);
    const route=m.route(selected);
    el('route-evidence').textContent=route?JSON.stringify(route.proof,null,2):'尚无认证接口。这个 Cell 仍可作为独立起始状态；不会把历史上下文当作接口。';
  }
  async function body(){
    const token=++epoch;el('body').replaceChildren();el('body').dataset.ready='false';
    const state=el('body-kind').value==='initial'?detail?.initial:detail?.state;
    if(!state){el('body').textContent='这个构造尚无对应图：'+(detail?.reason||detail?.status||'未导出');return;}
    const route=m.route(selected);
    const colors=m.mode==='interfaces'&&route?(detail.graft_interfaces||[]).find(e=>JSON.stringify(e.parents)===JSON.stringify(route.parents))||detail:detail;
    const highlights=el('body-kind').value==='state'?colors.highlight_rows:colors.initial_highlight_rows;
    const src=bodyDot(state,highlights);el('source').textContent=src;
    el('legend').textContent=colors.has_lhs_evidence?detail.color_note+`；${colors.unresolved_lhs.length} 个 LHS 项未能在当前快照定位。`:'该旧记录没有逐路径 LHS 绑定证据，暂不标橙色。绿色仅表示闭包事实，不能当作 smooth 来源证明。';
    await svg('dot',src,el('body'),token);
  }
  async function selectUnit(id){
    const n=m?.units.get(id);if(!n)return;
    const token=++epoch;selected=n;detail=null;choices.set(n.eclass,n);el('error').textContent='';el('typst').replaceChildren();el('body').replaceChildren();
    el('class').value=String(n.eclass);el('unit').replaceChildren();
    for(const v of m.classes.get(n.eclass))el('unit').add(new Option(`unit ${v.unit} · ${v.state} · 历史前驱 ${v.parents.join(', ')||'无'}`,v.unit));el('unit').value=String(id);
    routeEvidence();paths();el('guarantee').textContent='正在读取此路径的证据…';
    try{
      const d=await(await post('cell-egraph-state',{path,unit:id,snapshot})).json();if(token!==epoch)return;detail=d;el('member').replaceChildren();el('rule-svg').replaceChildren();for(const member of d.members)el('member').add(new Option(`${member.use_kind==='CoarseRuleComposition'?'Coarse':'Smooth'} · record ${member.record}`,member.record));
      if(d.root_record!=null)el('member').value=String(d.root_record);
      const g=d.guarantee;
      el('guarantee').textContent=[g.replay?'✓ 原历史回放验证通过':'? 无回放验证记录',g.saturated?'✓ 局部推理完成':`未完成：${d.status}`,g.certified_body?'✓ 导出认证闭包':g.observation_only?'观察性快照，不是认证闭包':'无认证闭包',g.symbolic?.schema?'符号路径条件下的闭包；不是全数值分支饱和':'',g.runtime_adapter].filter(Boolean).join(' ｜ ');
      el('evidence').textContent=JSON.stringify({guarantee:g,repetition_families:d.repetition_families,port_progressions:d.port_progressions,graft_interfaces:d.graft_interfaces,reuse_value:d.reuse_value,history_windows:d.history_windows,members:d.members,lhs_evidence:d.lhs_evidence},null,2);
      const url=new URL(location.href);url.searchParams.set('cell_eclass',n.eclass);url.searchParams.set('cell_unit',id);history.replaceState(null,'',url);
      await body();
    }catch(e){if(token===epoch)fail(e);}
  }
  function selectClass(c){const n=choices.get(c)||m.classes.get(c)?.[0];if(n)return selectUnit(n.unit);}
  function selectFamily(id){
    const f=m.families.get(id);if(!f)return;
    el('family-panel').open=true;const host=el('family-members');host.replaceChildren();
    for(const u of f.units||[]){const n=m.units.get(u);if(!n)continue;const b=document.createElement('button');b.textContent=`e${n.eclass} · unit ${u}`;b.onclick=()=>{panel.dataset.focus='body';selectUnit(u);};host.append(b);}
    const evidence=data.repetition_families||{},records=new Set(f.records),bindings=new Set(f.binding_cycle);
    const events=(evidence.events||[]).filter(e=>records.has(e.record));const contracts=new Set(events.map(e=>e.contract));
    el('family-evidence').textContent=JSON.stringify({family:f,events,contracts:[...contracts].map(i=>({id:i,...evidence.contracts[i]})),binding_contracts:[...bindings].map(i=>({id:i,...evidence.binding_contracts[i]}))},null,2);
    const samples=el('family-samples');samples.replaceChildren();
    for(const o of f.occurrences){const line=document.createElement('p');const a=events.find(e=>e.record===o.start_record),b=events.find(e=>e.record===o.end_record);const values=e=>(e?.values?.inputs||[]).filter(v=>v.literal!=null).map(v=>v.literal).join(', ');line.textContent=`event ${o.start_event} [${values(a)}] → event ${o.end_event} [${values(b)}] · ${o.steps.length} 条真实依赖`;samples.append(line);}
    el('family-focus').hidden=false;el('family-all').hidden=false;
    el('family-focus').onclick=async()=>{try{graphPane.open=true;const src=familyDot(m,id);el('source').textContent=src;await svg('dot',src,el('overview'));const graph=el('overview').querySelector('svg');if(graph){graph.style.maxWidth='100%';graph.style.height='auto';}}catch(e){fail(e);}};
    el('family-all').onclick=()=>svg('dot',overviewDot(m),el('overview')).catch(fail);
  }
  function familyTable(){
    el('families').replaceChildren();el('family-members').replaceChildren();el('family-evidence').replaceChildren();el('family-samples').replaceChildren();el('family-focus').hidden=true;el('family-all').hidden=true;
    const evidence=data.repetition_families;
    if(!evidence){el('families').textContent='旧快照没有分析证据；不从源码补猜重复族。';return;}
    for(const f of m.families.values()){const b=document.createElement('button');b.dataset.family=String(f.id);b.textContent=`F${f.id} · ${f.rules.join(' / ')} · ${f.occurrences.length} 次再入`;b.onclick=()=>selectFamily(f.id);el('families').append(b);}
    if(!m.families.size)el('families').textContent='本轮尚无满足真实 producer 接线的再入。';
  }
  function selectPortFamily(id){
    const f=m.portFamilies.get(id);if(!f)return;el('port-panel').open=true;
    el('port-summary').textContent=`${f.historical_producer_spellings} 种来源历史由闭包判等归为同一端口交互；宽度 ${f.contract.width}，推进 ${f.contract.shift}。`+(f.source_body_maps?.length===2?' 两份父闭包的事实均保留。':' 旧报告缺少父闭包保留证明。');
    const host=el('port-elements');host.replaceChildren();
    for(const c of f.components){const title=document.createElement('p');title.textContent=c.status==='observed-indexed-chain'?`${c.window_count} 个窗口，${c.element_count} 个位置；交互域 i=${c.domain.interaction_i.join('…')}`:`${c.status}：不生成数组坐标`;host.append(title);
      for(const p of c.elements||[]){const s=document.createElement('span');s.textContent=`X[${p.index}] = ${p.sort}#${p.token}　`;host.append(s);}
      const text=document.createElement('p');text.textContent=(c.interactions||[]).map(w=>`event ${w.event} → i=${w.i}`).join('；');host.append(text);
    }
    el('port-evidence').textContent=JSON.stringify(f,null,2);el('port-focus').hidden=false;el('port-all').hidden=false;
    el('port-focus').onclick=async()=>{try{graphPane.open=true;const src=portFamilyDot(m,id);el('source').textContent=src;await svg('dot',src,el('overview'));const graph=el('overview').querySelector('svg');if(graph){graph.style.maxWidth='100%';graph.style.height='auto';}}catch(e){fail(e);}};
    el('port-all').onclick=()=>svg('dot',overviewDot(m),el('overview')).catch(fail);
  }
  function portTable(){
    for(const role of ['port-families','port-summary','port-elements','port-evidence'])el(role).replaceChildren();
    el('port-focus').hidden=true;el('port-all').hidden=true;
    for(const f of m.portFamilies.values()){const b=document.createElement('button');b.dataset.portFamily=String(f.id);b.textContent=`P${f.id} · 宽 ${f.contract.width} / 步进 ${f.contract.shift} · ${f.witnesses.length} 次交互`;b.onclick=()=>selectPortFamily(f.id);el('port-families').append(b);}
    if(!m.portFamilies.size)el('port-families').textContent='本轮没有足够的认证闭包与端口接线证据。';
    renderDiscovery(el('discovery'),data.repetition_families?.port_progressions?.discovery,{
      selectUnit:u=>{panel.dataset.focus='body';el('body').closest('details').open=true;selectUnit(u);},
      selectFamily:selectPortFamily});
  }
  function reuseTable(){
    const host=el('reuse-table');host.replaceChildren();
    if(!data.reuse_value){host.textContent='旧目录没有复用统计，请重新 analyze。';return;}
    const table=document.createElement('table');table.style.borderSpacing='12px 5px';
    const head=table.insertRow();for(const label of ['Cell','构造','接口绑定','窗口读取','历史 effect 去重','外部 fact 去重','闭包行','重复覆盖引用','描述收益（槽）']){const th=document.createElement('th');th.textContent=label;head.append(th);}
    for(const r of data.reuse_value.cells){
      const row=table.insertRow(),button=document.createElement('button');button.textContent=`e${r.eclass}`;button.onclick=()=>selectClass(r.eclass);row.insertCell().append(button);
      for(const value of [r.registered_constructions,r.interface_instances,r.window_readings,r.historical_effect_fact_coverage,r.external_fact_coverage,r.closure_rows,r.overlapping_fact_references,r.isolated_description_gain??'未知'])row.insertCell().textContent=String(value);
    }
    host.append(table);
    if(!data.reuse_value.cells.length)host.append(document.createTextNode('暂无已认证闭包；Pending、Rejected 和 unfinished 不计为可复用 Cell。'));
  }
  async function renderRule(){const token=epoch;try{const member=detail?.members.find(m=>String(m.record)===el('member').value);if(!member||!detail.preview_source)throw Error('这条路径缺少原始规则源码，无法调用现有模板。');const source=detail.preview_source+'\n'+member.rule;const line=detail.preview_source.split('\n').length+1;const rendered=await previewRow({source,line,mode:'combined',label_style:'recursive',recursive_strategy:'tree-safe'});if(token!==epoch)return;el('source').textContent=rendered.typst;mountSvg(rendered.typst_svg,el('rule-svg'));el('rule-svg').dataset.ready='true';}catch(e){if(token===epoch)fail(e);}};
  el('rule-preview').onclick=renderRule;
  el('path-mode').onchange=()=>{m.mode=el('path-mode').value;paths();el('typst').replaceChildren();if(detail)body().catch(fail);};
  el('route').onchange=()=>{m.routeChoices.set(selected.unit,Number(el('route').value));routeEvidence();paths();el('typst').replaceChildren();if(detail)body().catch(fail);};
  el('snapshot').onchange=()=>api.load(path,el('snapshot').value).catch(fail);
  el('class').onchange=()=>selectClass(Number(el('class').value));el('unit').onchange=()=>selectUnit(Number(el('unit').value));el('body-kind').onchange=()=>body().catch(fail);
  el('typst-button').onclick=async()=>{try{if(!selected)return;const src=pathTypst(m,selected,choices);el('source').textContent=src;await svg('typst',src,el('typst'));}catch(e){fail(e);}};
  const all=el('overview').parentElement;all.ontoggle=()=>{if(all.open&&m&&!el('overview').childNodes.length)svg('dot',overviewDot(m),el('overview')).catch(fail);};
  const api={panel,noteSnapshot(frame){if(frame.artifact_directory===path&&frame.stem&&![...el('snapshot').options].some(o=>o.value===frame.stem))el('snapshot').add(new Option(frame.label||frame.stem,frame.stem));},get snapshot(){return snapshot;},get path(){return path;},clear(){epoch++;selected=null;detail=null;m=null;panel.hidden=true;for(const name of ['body','paths','typst','rule-svg','evidence','overview','status','guarantee','error','route-evidence'])el(name).replaceChildren();},get source(){return detail?.preview_source||'';},download(){if(!m)return;const a=document.createElement('a'),u=URL.createObjectURL(new Blob([overviewDot(m)],{type:'text/vnd.graphviz'}));a.href=u;a.download='ripsat-cell-egraph.dot';a.click();setTimeout(()=>URL.revokeObjectURL(u),1000);},async load(p,round){if(round!==undefined)snapshot=round;else if(p!==path)snapshot='';path=p;const token=++epoch;const received=await(await post('cell-egraph',{path,snapshot})).json();if(token!==epoch)return;data=received;el('snapshot').replaceChildren(new Option('最新（自动跟随）',''));for(const f of data.snapshots||[])el('snapshot').add(new Option(f.label||`Round ${f.round??f.stem}`,f.stem));el('snapshot').value=snapshot;m=model(data);m.mode=el('path-mode').value;choices.clear();el('overview').replaceChildren();el('class').replaceChildren();
    reuseTable();familyTable();portTable();
    for(const [c,ns] of m.classes)el('class').add(new Option(`e${c} · ${ns.length} 条构造 · ${m.reuse.has(c)?`${m.reuse.get(c).interface_instances} 个接口绑定`:'未认证'}`,c));
    el('status').textContent=`${m.classes.size} 个 e-class，${data.nodes.length} 条已登记构造；认证合并 ${data.certified_unions}。${data.note}`;
    if(!m.classes.size){el('guarantee').textContent='此分析目录没有 Cell egraph 登记数据。';for(const name of ['body','paths','typst','evidence'])el(name).replaceChildren();selected=null;detail=null;return;}
    const query=new URLSearchParams(location.search),unit=Number(query.get('cell_unit'));
    if(query.has('cell_unit')&&m.units.has(unit))await selectUnit(unit);else await selectClass(query.has('cell_eclass')&&m.classes.has(Number(query.get('cell_eclass')))?Number(query.get('cell_eclass')):m.classes.keys().next().value);
    if(graphPane.open)await svg('dot',overviewDot(m),el('overview'));
  },refresh(){return selected?selectUnit(selected.unit):Promise.resolve();}};
  return api;
}
