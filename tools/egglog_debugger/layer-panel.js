// Layer snapshots share the debugger's existing Typst plugin and Graphviz host.
// No second SVG layout engine or independent visualization page.
export function installLayerPanel(host, {post, previewRow, mountSvg, loadBrowserGenerator, editor, resolveRule}) {
    const panel=document.createElement('details');panel.id='native-layer-panel';panel.open=true;
    panel.innerHTML=`<summary>Layer / FractalComb · 每轮 DOT</summary>
      <div><select id="native-layer-round" aria-label="Layer 轮次"></select>
      <select id="native-layer-kind"><option value="fractals">FractalComb</option><option value="layers">Coarse / Smooth layers</option><option value="coverage">模板覆盖</option><option value="reuse">Use(T) / residual 复用</option><option value="use_fractals">Use(T) 递归候选</option></select>
      <select id="native-layer-scope" aria-label="Layer 或 Fractal"><option value="">全部</option></select>
      <select id="native-layer-format"><option value="dot">DOT / Graphviz</option><option value="typst">Typst / 现有 Fractal 模板</option></select>
      <button id="native-layer-render">显示</button><button id="native-layer-download">下载本轮 DOT</button></div>
      <div><input id="native-layer-directory" placeholder="out/ 中的分析目录" aria-label="分析目录"><button id="native-layer-load">载入分析目录</button><button id="native-layer-source">载入该次源码</button></div>
      <div id="native-layer-status">运行后逐轮接收 layer 快照；也可载入 CLI 分析目录。</div>
      <div id="native-layer-viewport"></div><pre id="native-layer-error"></pre>
      <details><summary>返回 binding、条件与 effect 证据</summary><pre id="native-layer-details"></pre></details>
      <details><summary>当前 Typst / DOT 源码</summary><pre id="native-layer-code"></pre></details>`;
    const catalogPanel=document.createElement('details');catalogPanel.innerHTML=`<summary>ClosedState 共享目录</summary><input id="native-closed-path" placeholder="out/ 中的 closed-catalog 目录"><button id="native-closed-load">载入共享目录</button><div id="native-closed-status"></div><div id="native-closed-view"></div><pre id="native-closed-details"></pre>`;panel.append(catalogPanel);
    const cp=id=>catalogPanel.querySelector('#native-closed-'+id);
    cp('load').onclick=async()=>{
        cp('load').disabled=true;cp('status').textContent='载入…';cp('details').textContent='';
        try{
            const data=await (await post('closed-catalog',{path:cp('path').value})).json();
            const markup=await (await post('render',{kind:'dot',source:data.dot})).text();
            cp('view').replaceChildren();const svg=mountSvg(markup,cp('view'));
            cp('status').textContent=`${data.catalog.triggers.length} 个 Trigger → ${data.catalog.closed_states} 个共享 ClosedState。固定端口与完整事实映射；入口条件保留。未决比较 ${data.catalog.unresolved_comparisons||0}。`;
            for(const node of svg.querySelectorAll('.node')){
                const id=node.querySelector('title')?.textContent||'';node.style.cursor='pointer';
                node.onclick=()=>{cp('details').textContent=JSON.stringify(id.startsWith('t')?data.catalog.triggers[Number(id.slice(1))]:data.states[Number(id.slice(1))],null,2);};
            }
        }catch(e){cp('status').textContent=String(e);}finally{cp('load').disabled=false;}
    };
    const initialCatalog=new URLSearchParams(location.search).get('closed_catalog');
    if(initialCatalog){catalogPanel.open=true;cp('path').value=initialCatalog;cp('load').click();}
    host.append(panel);
    const $=id=>panel.querySelector('#native-layer-'+id);
    let frames=[],version=0,abort=null,previewCache=new Map(),pinned=false,rendered=false;
    const frame=()=>frames[Number($('round').value)];
    function reset(){abort?.abort();version++;frames=[];pinned=false;rendered=false;previewCache.clear();$('round').replaceChildren();$('scope').replaceChildren(new Option('全部',''));$('viewport').replaceChildren();$('details').textContent='';$('code').textContent='';$('status').textContent='等待实际执行边界…';}
    function receive(f){
        if(f.kind!=='layer_snapshot'||!f.graphs||!f.analysis)throw Error('无效的 layer 快照');
        frames.push(f);$('round').add(new Option(f.label,String(frames.length-1)));
        // New data does not start expensive render subprocesses automatically.
        if(!pinned){$('round').value=String(frames.length-1);options(false);}
    }
    function options(pin=true){
        if(pin!==false)pinned=true;
        const f=frame();if(!f)return;
        $('scope').replaceChildren(new Option('全部',''));
        if($('kind').value==='layers')for(const g of f.graphs.layers.groups||[])$('scope').add(new Option(g.label,g.id));
        if($('kind').value==='fractals')f.analysis.fractals.forEach((x,i)=>$('scope').add(new Option(`F${i} · T${x.template} · depth ${x.max_observed_depth}`,'f'+i)));
        if($('kind').value==='coverage')f.analysis.templates.forEach((x,i)=>$('scope').add(new Option(`T${i} · ${x.interface.members.length} apply`,'t'+i)));
        if($('kind').value==='reuse')for(const i of f.reuse?.roots||[])if(i.Use!==undefined)$('scope').add(new Option(`Use(T${f.reuse.uses[i.Use].template}) · U${i.Use}`,'use'+i.Use));
        if($('kind').value==='use_fractals')for(const [i,x] of (f.analysis.use_fractals?.families||[]).entries())$('scope').add(new Option(`Use F${i} · T${x.template} · depth ${x.observed_depth}`,'rf'+i));
        const c=f.counts;$('status').textContent=`${f.label} · ${c.applications} apply · ${c.templates} templates · ${c.fractals} FractalComb · ${f.artifact_directory||'CLI / 导入快照'}。有限观察；结构覆盖不等于可替代。候选截断 ${c.truncated_candidates||0}，覆盖未检查 ${c.coverage_skipped||0}。`;
        if(f.reuse){const r=f.reuse.stats;$('status').textContent+=` Use 覆盖 ${r.covered_events}/${r.events}，residual ${r.residual_events}；后续端口复用 ${r.continuations_through_use}。编码模型 ${r.selected_wiring_units} + 已用字典 ${r.used_dictionary_units} / 原始 ${r.raw_wiring_units}；候选索引 ${r.candidate_index_units}（非字节数）。`; if(r.probation_templates!==undefined)$('status').textContent+=` 试用模板 ${r.probation_templates}，淘汰 ${r.probation_evictions}；边界调整 ${r.local_rotations}，分割访问 ${r.cut_visits}，预算跳过 ${r.cut_budget_stops}。`; }
        if(f.ripen)$('status').textContent+=` Ripen ${f.ripen.state}，第 ${f.ripen.round}/${f.ripen.max_rounds} 轮；作用域为整个局部单元，非每个子 Use。未导入 match ${f.ripen.excluded_matches}。`;
        if(f.ripen?.origin)$('status').textContent+=` 来源 U${f.ripen.origin.use_id}/T${f.ripen.origin.template}；符号入口，不含未知的边界内部结构。`;
        const uf=f.analysis.use_fractals;if(uf)$('status').textContent+=` Use 转移 ${uf.transitions.length}，递归候选 ${uf.families.length}（未证明无限归纳），截断 ${uf.truncated_sources}/${uf.truncated_edges}/${uf.truncated_paths}/${uf.truncated_branches}。`;
    }
    function selectedGraph(){
        const f=frame(),kind=$('kind').value,g=f.graphs[kind],scope=$('scope').value;
        if(!g)throw Error('此旧快照没有 Use 复用数据，请重新运行。');
        if(!scope)return g;
        let keep=new Set();
        if(kind==='layers'){const group=g.groups.find(g=>g.id===scope);for(const id of group?.members||[])keep.add(id);for(const e of g.edges)if(group?.members.includes(e.to))keep.add(e.from);}
        else if(kind==='use_fractals'){for(const n of g.nodes)if(n.id===scope||n.id.startsWith(scope+'u'))keep.add(n.id);}
        else if(kind==='fractals'){
            const family=f.analysis.fractals[Number(scope.slice(1))];keep.add(scope);
            for(const u of family.units)keep.add('u'+u);
            for(const n of g.nodes)if(n.id.startsWith(scope+'t'))keep.add(n.id);
        }else {keep.add(scope);for(const e of g.edges)if(e.from===scope||e.to===scope){keep.add(e.from);keep.add(e.to);}}
        return {...g,nodes:g.nodes.filter(n=>keep.has(n.id)),edges:g.edges.filter(e=>keep.has(e.from)&&keep.has(e.to))};
    }
    function dot(g){
        const q=JSON.stringify,lines=['digraph G { rankdir=LR; node [shape=box,style=filled];'];
        for(const n of g.nodes)lines.push(`${q(n.id)} [label=${q(n.label)},fillcolor=${q({coarse:'#fce6c9',smooth:'#dff2ec',fractal:'#e9e1fa',trigger:'#fbd5d0'}[n.kind]||'#edf0f5')}];`);
        for(const e of g.edges)lines.push(`${q(e.from)} -> ${q(e.to)} [label=${q(e.label)}];`);
        return lines.join('\n')+'\n}';
    }
    function expression(e,m){
        if(e.Input!==undefined){const role=m.input_roles[e.Input];return role?.startsWith('var:')?role.slice(4):null;}
        if(e.Int!==undefined)return String(e.Int);
        if(e.Text!==undefined)return JSON.stringify(e.Text);
        if(e.Call){const args=e.Call[1].map(x=>expression(x,m));return args.every(x=>x!==null)?`(${e.Call[0]} ${args.join(' ')})`:null;}
        return null;
    }
    function updates(t){
        return t.returns.flatMap((ports,r)=>ports.map((p,i)=>{
            const target=t.members[0].input_roles[i];
            if(!p.Internal||!target?.startsWith('var:'))return null;
            const m=t.members[p.Internal.member],term=t.members.length===1?expression(m.output_terms[p.Internal.output],m):`m${p.Internal.member}.out${p.Internal.output}`;
            return term===null?null:`${t.returns.length>1?'return '+r+' / ':''}${target.slice(4)} ← ${term}`;
        }).filter(Boolean));
    }
    async function typesetMember(f,t,member,iteration,container,signal){
        const site=f.sites.find(s=>s.source===member.rule);
        // The current editor text wins over the snapshot: editing an annotation
        // template, a rule name or anything else must show up here without running
        // recognition again. The snapshot is only the fallback, for a loaded
        // directory or a rule that was deleted, and the card says which one was used.
        const live=resolveRule?await resolveRule(member.rule,signal):null;
        const previewSource=live?live.source:(site?f.preview_source:null);
        const previewLine=live?live.line:(site?site.source_line:null);
        if(previewSource===null||previewLine===null)throw Error('快照没有匹配的源位置；请在编辑器里保留该规则，或重新生成快照。');
        const provenance=live?'editor':'snapshot';
        container.dataset.provenance=provenance;
        let request={source:previewSource,line:previewLine,mode:'combined',label_style:'recursive',recursive_strategy:'dag-expand'};
        if(iteration){
            request.fractal=iteration;
            // Only a single-member single-return family is a linear lane. A
            // multi-return unit is rendered member-by-member through the same
            // plugin template, with its return map shown as runtime evidence.
            if(t.members.length===1&&t.returns.length===1){
                const generator=await loadBrowserGenerator();
                const plan=generator?.fractalRuleSourceInBrowser(request.source,request.line,iteration.depth,iteration.update);
                if(plan)request={...request,source:plan.source,line:plan.line,fractal:{...iteration,chain:true,truncated:plan.truncated}};
            }
        }
        const key=JSON.stringify(request);
        let rendered=previewCache.get(key);if(!rendered){rendered=await previewRow(request,signal);previewCache.set(key,rendered);}
        if(signal.aborted)return;
        mountSvg(rendered.typst_svg,container);
        container.dataset.renderer=rendered.renderer||'';
        container.dataset.iteration=JSON.stringify(rendered.iteration||null);
        if(provenance==='snapshot'){
            const note=document.createElement('div');note.className='native-layer-source';
            note.textContent='源码：运行快照（编辑器里找不到这条规则，注解按运行时的源码）';
            container.prepend(note);
        }
        const source=document.createElement('details'),title=document.createElement('summary'),code=document.createElement('pre');title.textContent='现有插件生成的 Typst';code.textContent=rendered.typst;source.append(title,code);container.append(source);
        $('code').textContent+=(rendered.typst||'')+'\n';
    }
    async function render(){
        const f=frame();if(!f)return;pinned=true;
        abort?.abort();abort=new AbortController();const signal=abort.signal,v=++version;
        $('error').textContent='';$('viewport').replaceChildren();$('viewport').dataset.ready='false';$('code').textContent='';
        try{
            if($('format').value==='dot'){
                const g=selectedGraph(),source=$('scope').value?dot(g):f.dots[$('kind').value];
                $('code').textContent=source;
                const response=await post('render',{kind:'dot',source},signal),markup=await response.text();if(v!==version)return;
                const svg=mountSvg(markup,$('viewport'));
                for(const e of svg.querySelectorAll('.node')){
                    const id=e.querySelector('title')?.textContent,n=g.nodes.find(n=>n.id===id);if(!n)continue;
                    e.style.cursor='pointer';e.onclick=()=>{
                        $('details').textContent=JSON.stringify(n.detail,null,2);
                        if(/^f\d+$/.test(id))$('scope').value=id;
                    };
                }
            }else{
                let members=[],t=null,iteration=null,scope=$('scope').value;
                if($('kind').value==='fractals'){
                    if(!scope){scope=f.analysis.fractals.length?'f0':'';$('scope').value=scope;}
                    if(!scope)throw Error('本轮尚无有限 FractalComb。');
                    const family=f.analysis.fractals[Number(scope.slice(1))];t=f.analysis.templates[family.template].interface;members=t.members;
                    iteration={depth:family.max_observed_depth,operator:`LayerTemplate T${family.template}`,context:f.graphs.fractals.nodes.filter(n=>n.id.startsWith(scope+'t')).flatMap(n=>n.detail.context_events||[]).join(', ')||'external entry', update:updates(t),witness:`Observed finite unit; ${t.returns.length} return ports; unbounded induction unknown`};
                    $('details').textContent=JSON.stringify({family,interface:t},null,2);
                }else if($('kind').value==='use_fractals'){
                    const data=f.analysis.use_fractals;
                    if(!scope){scope=data?.families.length?'rf0':'';$('scope').value=scope;}
                    if(!scope)throw Error('本轮尚无满足重复端口转移条件的 Use 递归候选。');
                    const family=data.families[Number(scope.slice(2))],template=f.reuse.templates[family.template];
                    members=template.pattern.steps.map(step=>({...f.reuse.schemas[step.schema],binding:step.wiring.map(w=>w.Local?{Internal:w.Local}:{Boundary:{port:w.Input}})}));
                    const transfers=family.transitions.map(i=>({id:i,...data.transitions[i]}));
                    const text=transfers.map(t=>'F'+t.id+': '+t.transfer.inputs.map((p,i)=>`b'[${i}]=`+(p.Return?`out(m${p.Return.member},${p.Return.output})`:p.Carry?`b[${p.Carry.input}]`:`external[${p.External.slot}]`)).join(', ')).join('\n');
                    const pre=document.createElement('pre');pre.style.whiteSpace='pre-wrap';pre.textContent=text+'\nObserved finite recurrence; arbitrary-depth induction unknown.';$('viewport').append(pre);
                    $('details').textContent=JSON.stringify({family,transfers,template},null,2);
                }else if($('kind').value==='reuse'){
                    if(!scope)throw Error('请选择一个 Use 实例，或使用 DOT 查看整个复用图。');
                    const u=f.reuse.uses[Number(scope.slice(3))];members=f.reuse.templates[u.template].pattern.steps.map(step=>({...f.reuse.schemas[step.schema],binding:step.wiring.map(w=>w.Local?{Internal:w.Local}:{Boundary:{port:w.Input}})}));
                    $('details').textContent=JSON.stringify({use:u,template:f.reuse.templates[u.template]},null,2);
                }else if($('kind').value==='coverage'){
                    if(!scope)throw Error('请先选择一个模板。');t=f.analysis.templates[Number(scope.slice(1))].interface;members=t.members;$('details').textContent=JSON.stringify(t,null,2);
                }else{
                    if(!scope)throw Error('请先选择一个 coarse/smooth layer。');
                    const g=selectedGraph(),group=f.graphs.layers.groups.find(g=>g.id===scope);
                    members=g.nodes.filter(n=>group.members.includes(n.id)&&n.detail.source).map(n=>({rule:n.detail.source}));
                    $('details').textContent=JSON.stringify(group,null,2);
                }
                if(members.length>16)throw Error('该 layer 超过 16 个成员；请使用 DOT 查看完整结构。');
                for(let i=0;i<members.length;i++){
                    if(signal.aborted)return;const card=document.createElement('section'),label=document.createElement('strong'),content=document.createElement('div');label.textContent=`m${i} · ${iteration?'Fractal 单元成员':'Layer 成员'}`;card.append(label);
                    if(members[i].binding){const wiring=document.createElement('pre');wiring.textContent=members[i].binding.map((p,j)=>`input[${j}] (${members[i].input_roles[j]}) ← ${p.Internal?`m${p.Internal.member}.out${p.Internal.output}`:`boundary[${p.Boundary.port}]`}`).join('\n');card.append(wiring);}
                    card.append(content);$('viewport').append(card);
                    await typesetMember(f,t,members[i],i===0?iteration:null,content,signal);
                }
            }
            if(v===version){rendered=true;$('viewport').dataset.ready='true';}
        }catch(e){if(v===version&&e.name!=='AbortError')$('error').textContent=e.message;}
    }
    $('round').onchange=options;$('kind').onchange=options;$('render').onclick=render;
    $('download').onclick=()=>{const f=frame();if(!f)return;const kind=$('kind').value,a=document.createElement('a'),url=URL.createObjectURL(new Blob([f.dots[kind]],{type:'text/vnd.graphviz;charset=utf-8'}));a.href=url;a.download=f.stem+'.'+kind+'.dot';a.click();setTimeout(()=>URL.revokeObjectURL(url),1000);};
    async function load(path){
        const controls=['load','render','round','kind','scope','format'];controls.forEach(id=>$(id).disabled=true);
        try {const data=await (await post('layer-run',{path})).json();reset();for(const f of data.frames)receive(f);}
        finally {controls.forEach(id=>$(id).disabled=false);}
    }
    $('load').onclick=()=>load($('directory').value).catch(e=>$('error').textContent=e.message);
    $('source').onclick=()=>{if(frame())editor.setValue(frame().preview_source);};
    const initialKind=new URLSearchParams(location.search).get('layer_kind');if(['layers','fractals','coverage','reuse','use_fractals'].includes(initialKind))$('kind').value=initialKind;
    const initial=new URLSearchParams(location.search).get('layer_run');if(initial){$('directory').value=initial;load(initial).then(()=>render()).catch(e=>$('error').textContent=e.message);}
    // Re-render the current selection against the current source. Called after an
    // editor change so an edited annotation template is visible without re-running.
    async function refresh(){if(rendered&&frames.length)await render();}
    return {receive,reset,load,refresh,get snapshots(){return structuredClone(frames);}};
}
