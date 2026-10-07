// AND/OR construction choices, not an enumeration of all possible rewrite histories.
export function model(data) {
  const units=new Map(data.nodes.map(n=>[n.unit,n]));
  const classes=new Map();
  for(const n of data.nodes){if(n.eclass==null)continue;if(!classes.has(n.eclass))classes.set(n.eclass,[]);classes.get(n.eclass).push(n);}
  // In the displayed egraph, each CoarseApply alternative has an edge into its eclass.
  const degree=c=>classes.get(c)?.length||0;
  const rank=n=>(n.parents||[]).reduce((sum,p)=>sum+degree(units.get(p)?.eclass),0);
  for(const ns of classes.values())ns.sort((a,b)=>rank(b)-rank(a)||a.unit-b.unit);
  return {units,classes,degree,parents:n=>[...(n.parents||[])].sort((a,b)=>degree(units.get(b)?.eclass)-degree(units.get(a)?.eclass)||a-b)};
}
const q=JSON.stringify;
export function overviewDot(m){
  const lines=['digraph Cells {rankdir=TB; node [shape=box,style="rounded,filled",fillcolor="#e8eef9"];'];
  for(const [c,ns] of m.classes)lines.push(`"c${c}" [label=${q(`Cell e${c}\n${ns.length} constructions`)},URL=${q(`#cell-class-${c}`)}];`);
  for(const [c,ns] of m.classes)for(const n of ns){lines.push(`"u${n.unit}" [shape=ellipse,fillcolor="#f8edcf",label=${q(`CoarseApply ${n.contract}`)},URL=${q(`#cell-unit-${n.unit}`)}];`);lines.push(`"u${n.unit}" -> "c${c}";`);for(const p of n.parents||[]){const parent=m.units.get(p);if(parent)lines.push(`"c${parent.eclass}" -> "u${n.unit}";`);}}
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
    if(seen.has(n.eclass))return;seen.add(n.eclass);
    const parents=m.parents(n).map(id=>m.units.get(id)).filter(Boolean).map(p=>choices.get(p.eclass)??m.classes.get(p.eclass)?.[0]??p);
    for(const p of parents)visit(p);
    rows.push([parents.length?parents.map(p=>`C${p.eclass}`).join(' + '):'NoParents',`CoarseApply ${n.contract}`,`C${n.eclass}`,n.unit]);
  }
  visit(root);
  const items=rows.map(([p,t,c,u])=>`[#box[$ upright(${q(p)}) arrow.r upright(${q(t)}) arrow.r $ #link(${q(`#cell-unit-${u}`)})[#text(${q(c)})]]]`);
  // Same local Typst renderer and upright/arrow convention as existing fractal lanes.
  return '#set page(width: auto, height: auto, margin: 12pt)\n#set text(size: 11pt)\n#stack(dir: ttb, spacing: 8pt,\n'+items.join(',\n')+'\n)';
}
export function installCellPanel(parent,{post,mountSvg,previewRow}){
  const panel=document.createElement('section');panel.id='native-cell-egraph';panel.hidden=true;
  panel.innerHTML=`<header data-role="toolbar"><button data-role="fullscreen">全屏看图</button><label>快照 <select data-role="snapshot"><option value="">最新（自动跟随）</option></select></label></header><p><b>RipSatCell egraph</b> · 点击状态查看内部图与生成方式</p>
    <div data-role="status"></div><label>Cell e-class <select data-role="class"></select></label>
    <label>当前构造路径 <select data-role="unit"></select></label>
    <p data-role="guarantee" style="padding:10px;background:#edf1f7;color:#182338;border-radius:6px"></p>
    <details><summary>整个 Cell egraph（点击节点跳转）</summary><div data-role="overview"></div></details>
    <p>已登记的 AND/OR 路径：前驱按入度降序排列；展开可选择其他构造，重复状态以引用表示。</p>
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
  let oldOverflow='';
  function fullscreen(on){panel.classList.toggle('cell-fullscreen',on);el('fullscreen').textContent=on?'退出全屏（Esc）':'全屏看图';if(on){oldOverflow=document.body.style.overflow;document.body.style.overflow='hidden';graphPane.open=true;panel.setAttribute('role','dialog');panel.setAttribute('aria-modal','true');if(m&&!el('overview').childNodes.length)svg('dot',overviewDot(m),el('overview')).catch(fail);}else{document.body.style.overflow=oldOverflow;panel.removeAttribute('role');panel.removeAttribute('aria-modal');}}
  el('fullscreen').onclick=()=>fullscreen(!panel.classList.contains('cell-fullscreen'));
  document.addEventListener('keydown',e=>{if(e.key==='Escape'&&panel.classList.contains('cell-fullscreen'))fullscreen(false);});
  const choices=new Map();
  function fail(e){el('error').textContent=e.message||String(e);}
  async function svg(kind,source,host,token=epoch){
    const text=await(await post('render',{kind,source})).text();if(token!==epoch)return;
    host.replaceChildren();mountSvg(text,host);host.dataset.ready='true';
    host.onclick=e=>{const link=e.target.closest('a');if(!link)return;const href=link.getAttribute('href')||link.getAttribute('xlink:href')||'';const match=href.match(/#cell-(class|unit)-(\d+)$/);if(match){e.preventDefault();if(match[1]==='class'){panel.dataset.focus='body';el('body').closest('details').open=true;selectClass(Number(match[2]));}else{panel.dataset.focus='rule';selectUnit(Number(match[2])).then(()=>{if(selected?.unit===Number(match[2]))renderRule();});}}};
  }
  function paths(){
    el('paths').replaceChildren();
    const draw=(node,host,ancestors)=>{
      const item=document.createElement('details'),label=document.createElement('summary');
      label.textContent=`e${node.eclass} · unit ${node.unit} · CoarseApply ${node.contract} · 入度 ${m.degree(node.eclass)}`;item.append(label);host.append(item);
      const button=document.createElement('button');button.textContent='查看这个 RipSat';button.onclick=()=>selectUnit(node.unit);item.append(button);
      if(ancestors.has(node.eclass)){item.append(document.createTextNode(' 递归／共享引用，不重复展开'));return;}
      const selector=document.createElement('select');selector.setAttribute('aria-label',`e${node.eclass} 的构造选择`);
      for(const n of m.classes.get(node.eclass)||[])selector.add(new Option(`unit ${n.unit} · 前驱 ${n.parents.join(', ')||'无'}`,n.unit));
      selector.value=String(node.unit);item.append(selector);
      const nested=document.createElement('div');nested.style.marginLeft='18px';item.append(nested);
      const expand=()=>{nested.replaceChildren();const n=m.units.get(Number(selector.value));choices.set(node.eclass,n);for(const id of m.parents(n)){const p=m.units.get(id);if(p)draw(choices.get(p.eclass)||m.classes.get(p.eclass)[0],nested,new Set([...ancestors,node.eclass]));}};
      selector.onchange=()=>{if(node.eclass===selected.eclass){selectUnit(Number(selector.value));return;}expand();el('typst').replaceChildren();};item.ontoggle=()=>{if(item.open&&!nested.childNodes.length)expand();};
      if(host===el('paths'))item.open=true;
    };
    if(selected)draw(selected,el('paths'),new Set());
  }
  async function body(){
    const token=epoch;el('body').replaceChildren();el('body').dataset.ready='false';
    const state=el('body-kind').value==='initial'?detail?.initial:detail?.state;
    if(!state){el('body').textContent='这个构造尚无对应图：'+(detail?.reason||detail?.status||'未导出');return;}
    const highlights=el('body-kind').value==='state'?detail.highlight_rows:detail.initial_highlight_rows;
    const src=bodyDot(state,highlights);el('source').textContent=src;
    el('legend').textContent=detail.has_lhs_evidence?detail.color_note+`；${detail.unresolved_lhs.length} 个 LHS 项未能在当前快照定位。`:'该旧记录没有逐路径 LHS 绑定证据，暂不标橙色。绿色仅表示闭包事实，不能当作 smooth 来源证明。';
    await svg('dot',src,el('body'),token);
  }
  async function selectUnit(id){
    const n=m?.units.get(id);if(!n)return;
    const token=++epoch;selected=n;detail=null;choices.set(n.eclass,n);el('error').textContent='';el('typst').replaceChildren();el('body').replaceChildren();
    el('class').value=String(n.eclass);el('unit').replaceChildren();
    for(const v of m.classes.get(n.eclass))el('unit').add(new Option(`unit ${v.unit} · ${v.state} · 前驱 ${v.parents.join(', ')||'无'}`,v.unit));el('unit').value=String(id);
    paths();el('guarantee').textContent='正在读取此路径的证据…';
    try{
      const d=await(await post('cell-egraph-state',{path,unit:id,snapshot})).json();if(token!==epoch)return;detail=d;el('member').replaceChildren();el('rule-svg').replaceChildren();for(const member of d.members)el('member').add(new Option(`${member.use_kind==='CoarseRuleComposition'?'Coarse':'Smooth'} · record ${member.record}`,member.record));
      if(d.root_record!=null)el('member').value=String(d.root_record);
      const g=d.guarantee;
      el('guarantee').textContent=[g.replay?'✓ 原历史回放验证通过':'? 无回放验证记录',g.saturated?'✓ 局部推理完成':`未完成：${d.status}`,g.certified_body?'✓ 导出认证闭包':g.observation_only?'观察性快照，不是认证闭包':'无认证闭包',g.symbolic?.schema?'符号路径条件下的闭包；不是全数值分支饱和':'',g.runtime_adapter].filter(Boolean).join(' ｜ ');
      el('evidence').textContent=JSON.stringify({guarantee:g,members:d.members,lhs_evidence:d.lhs_evidence},null,2);
      const url=new URL(location.href);url.searchParams.set('cell_eclass',n.eclass);url.searchParams.set('cell_unit',id);history.replaceState(null,'',url);
      await body();
    }catch(e){if(token===epoch)fail(e);}
  }
  function selectClass(c){const n=choices.get(c)||m.classes.get(c)?.[0];if(n)return selectUnit(n.unit);}
  async function renderRule(){const token=epoch;try{const member=detail?.members.find(m=>String(m.record)===el('member').value);if(!member||!detail.preview_source)throw Error('这条路径缺少原始规则源码，无法调用现有模板。');const source=detail.preview_source+'\n'+member.rule;const line=detail.preview_source.split('\n').length+1;const rendered=await previewRow({source,line,mode:'combined',label_style:'recursive',recursive_strategy:'tree-safe'});if(token!==epoch)return;el('source').textContent=rendered.typst;mountSvg(rendered.typst_svg,el('rule-svg'));el('rule-svg').dataset.ready='true';}catch(e){if(token===epoch)fail(e);}};
  el('rule-preview').onclick=renderRule;
  el('snapshot').onchange=()=>api.load(path,el('snapshot').value).catch(fail);
  el('class').onchange=()=>selectClass(Number(el('class').value));el('unit').onchange=()=>selectUnit(Number(el('unit').value));el('body-kind').onchange=()=>body().catch(fail);
  el('typst-button').onclick=async()=>{try{if(!selected)return;const root=choices.get(selected.eclass)||selected;const src=pathTypst(m,root,choices);el('source').textContent=src;await svg('typst',src,el('typst'));}catch(e){fail(e);}};
  const all=el('overview').parentElement;all.ontoggle=()=>{if(all.open&&m&&!el('overview').childNodes.length)svg('dot',overviewDot(m),el('overview')).catch(fail);};
  const api={panel,noteSnapshot(frame){if(frame.artifact_directory===path&&frame.stem&&![...el('snapshot').options].some(o=>o.value===frame.stem))el('snapshot').add(new Option(frame.label||frame.stem,frame.stem));},get snapshot(){return snapshot;},get path(){return path;},clear(){if(panel.classList.contains('cell-fullscreen'))fullscreen(false);epoch++;selected=null;detail=null;m=null;panel.hidden=true;for(const name of ['body','paths','typst','rule-svg','evidence','overview','status','guarantee','error'])el(name).replaceChildren();},get source(){return detail?.preview_source||'';},download(){if(!m)return;const a=document.createElement('a'),u=URL.createObjectURL(new Blob([overviewDot(m)],{type:'text/vnd.graphviz'}));a.href=u;a.download='ripsat-cell-egraph.dot';a.click();setTimeout(()=>URL.revokeObjectURL(u),1000);},async load(p,round){if(round!==undefined)snapshot=round;else if(p!==path)snapshot='';path=p;const token=++epoch;const received=await(await post('cell-egraph',{path,snapshot})).json();if(token!==epoch)return;data=received;el('snapshot').replaceChildren(new Option('最新（自动跟随）',''));for(const f of data.snapshots||[])el('snapshot').add(new Option(f.label||`Round ${f.round??f.stem}`,f.stem));el('snapshot').value=snapshot;m=model(data);choices.clear();el('overview').replaceChildren();el('class').replaceChildren();
    for(const [c,ns] of m.classes)el('class').add(new Option(`e${c} · ${ns.length} 条构造 · 入度 ${m.degree(c)}`,c));
    el('status').textContent=`${m.classes.size} 个 e-class，${data.nodes.length} 条已登记构造；认证合并 ${data.certified_unions}。${data.note}`;
    if(!m.classes.size){el('guarantee').textContent='此分析目录没有 Cell egraph 登记数据。';for(const name of ['body','paths','typst','evidence'])el(name).replaceChildren();selected=null;detail=null;return;}
    const query=new URLSearchParams(location.search),unit=Number(query.get('cell_unit'));
    if(query.has('cell_unit')&&m.units.has(unit))await selectUnit(unit);else await selectClass(query.has('cell_eclass')&&m.classes.has(Number(query.get('cell_eclass')))?Number(query.get('cell_eclass')):m.classes.keys().next().value);
    if(graphPane.open)await svg('dot',overviewDot(m),el('overview'));
  },refresh(){return selected?selectUnit(selected.unit):Promise.resolve();}};
  return api;
}
