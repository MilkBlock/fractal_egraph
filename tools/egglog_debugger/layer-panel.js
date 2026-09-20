// Layer snapshots share the debugger's existing Typst plugin and Graphviz host.
// No second SVG layout engine or independent visualization page.
export function installLayerPanel(host, {post, previewRow, mountSvg, loadBrowserGenerator, editor, resolveRule}) {
    const panel=document.createElement('details');panel.id='native-layer-panel';panel.open=true;
    panel.innerHTML=`<summary>Layer / FractalComb · 每轮 DOT</summary>
      <div><select id="native-layer-round" aria-label="Layer 轮次"></select>
      <select id="native-layer-kind"><option value="fractals">FractalComb</option><option value="closed">ClosedState / Closed rule comb</option><option value="layers">Coarse / Smooth layers</option><option value="coverage">模板覆盖</option><option value="reuse">Use(T) / residual 复用</option><option value="use_fractals">Use(T) 递归候选</option></select>
      <select id="native-layer-scope" aria-label="Layer 或 Fractal"><option value="">全部</option></select>
      <select id="native-layer-format"><option value="dot">DOT / Graphviz</option><option value="typst">Typst / 现有 Fractal 模板</option></select>
      <button id="native-layer-render">显示</button><button id="native-layer-download">下载本轮 DOT</button></div>
      <div><input id="native-layer-directory" placeholder="out/ 中的分析目录" aria-label="分析目录"><button id="native-layer-load">载入分析目录</button><button id="native-layer-source">载入该次源码</button></div>
      <div id="native-layer-status">运行后逐轮接收 layer 快照；也可载入 CLI 分析目录。</div>
      <div id="native-layer-viewport"></div><pre id="native-layer-error"></pre>
      <details><summary>返回 binding、条件与 effect 证据</summary><pre id="native-layer-details"></pre></details>
      <details><summary>当前 Typst / DOT 源码</summary><pre id="native-layer-code"></pre></details>`;
    const catalogPanel=document.createElement('section');catalogPanel.hidden=true;catalogPanel.innerHTML=`<p>ClosedState / Closed rule comb · 每轮处理进度与共享闭包</p><input hidden id="native-closed-path" placeholder="out/ 中的 closed-catalog 目录"><button hidden id="native-closed-load">载入共享目录</button><select id="native-closed-state" aria-label="共享闭包"><option value="">全部概览</option></select><select id="native-closed-comb" aria-label="对应组合"><option value="">所有对应组合</option></select><select id="native-closed-instance" aria-label="触发实例"><option value="">示例实例</option></select><select id="native-closed-diagram" aria-label="ClosedState 图类型"><option value="comb">来源 rule comb</option><option value="egraph">闭包 e-graph</option><option value="layers">来源 Coarse / Smooth layer</option></select><div id="native-closed-caption"></div><div id="native-closed-status"></div><div id="native-closed-table"></div><div id="native-closed-view"></div><pre id="native-closed-rule" style="white-space:pre-wrap"></pre><pre id="native-closed-details"></pre>`;panel.append(catalogPanel);
    const cp=id=>catalogPanel.querySelector('#native-closed-'+id);
    let catalogData=null,catalogVersion=0,catalogError=null;
    const NO_CATALOG='尚无 ClosedState 快照：请先「运行并识别」（它会自动跑有预算的 ripen 队列），或在目录框载入一次带 ClosedState 的运行目录（含 catalog/）。Pending / Suspended 不代表已闭合。';
    const STALE_SNAPSHOTS='该目录的每轮快照里没有 ClosedState 数据（生成于 ClosedState 管线接入之前）。请用当前版本重新 analyze，或用「运行并识别」重跑一次。';
    function catalogGraph(){
        const c=catalogData.catalog,scope=cp('state').value,nodes=[],edges=[];
        const states=(c.state_groups||[]).filter(g=>scope===''||g.closed_state===Number(scope));
        for(const state of states)nodes.push({id:'c'+state.closed_state,label:`ClosedState C${state.closed_state}\n${state.trigger_count} Uses / ${state.template_count} templates`,kind:'fractal',detail:state});
        for(const g of c.comb_groups||[]){
            if(scope!==''&&g.closed_state!==Number(scope))continue;
            if(cp('comb').value!==''&&g.id!==Number(cp('comb').value))continue;
            if(cp('instance').value!==''&&!g.triggers.includes(Number(cp('instance').value)))continue;
            const members=cp('instance').value!==''?(selectedTrigger()?.binding_origin?.comb_members||g.members):g.members;
            const label=g.template===null?`入口 ${g.id}`:`T${g.template}`;
            nodes.push({id:'g'+g.id,label:`${label} · ${g.triggers.length} Uses\n`+g.members.map(m=>(m.use_kind==='CoarseComb'?'C:':'S:')+m.rule_name).join(' / '),kind:'template',detail:g});
            if(scope!==''&&members.length){
                for(const m of members){
                    const id=`g${g.id}m${m.slot}`;
                    nodes.push({id,label:`${m.use_kind} · ${m.rule_name}`,kind:m.use_kind==='CoarseComb'?'coarse':'smooth',detail:{group:g.id,member:m}});
                    if(!m.parents.length)edges.push({from:'g'+g.id,to:id,label:'entry'});
                    const linked=new Set();
                    for(const [input,w] of (m.binding||[]).entries())if(w.Local){linked.add(w.Local.member);edges.push({from:`g${g.id}m${w.Local.member}`,to:id,label:`out${w.Local.output} → in${input}`});}
                    for(const parent of m.parents)if(!linked.has(parent))edges.push({from:`g${g.id}m${parent}`,to:id,label:'context dependency'});
                }
                edges.push({from:'g'+g.id,to:'c'+g.closed_state,label:'ripen whole comb'});
            }else edges.push({from:'g'+g.id,to:'c'+g.closed_state,label:'exact state mapping'});
        }
        return {nodes,edges,sinks:states.map(s=>'c'+s.closed_state)};
    }
    function catalogTable(){
        const c=catalogData.catalog,scope=cp('state').value;cp('table').replaceChildren();
        const note=document.createElement('p');note.textContent='C/S 按此 Use 的直接记录依赖分类；原始 Coarse/Smooth layer 编号在详情中。关联不表示完整覆盖，符号入口等价不表示任意具体参数都 closed。';cp('table').append(note);
        const table=document.createElement('table'),head=document.createElement('tr');
        for(const title of scope===''?['ClosedState','Use 实例','不同模板','查看']:['组合模板','Coarse / Smooth 成员','Use 数','查看']){const th=document.createElement('th');th.textContent=title;head.append(th);}table.append(head);
        const rows=scope===''?(c.state_groups||[]):(c.comb_groups||[]).filter(g=>g.closed_state===Number(scope));
        for(const row of rows){
            const tr=document.createElement('tr'),values=scope===''?[`C${row.closed_state}`,row.trigger_count,row.template_count]:[`T${row.template??'—'}`,row.members.map(m=>(m.use_kind==='CoarseComb'?'C:':'S:')+m.rule_name).join(' / '),row.triggers.length];
            for(const value of values){const td=document.createElement('td');td.textContent=value;tr.append(td);}
            const td=document.createElement('td'),button=document.createElement('button');button.textContent=scope===''?'展开组合':'实例与 layer';
            button.onclick=()=>{if(scope===''){cp('state').value=String(row.closed_state);combOptions();renderCatalog();}else focusComb(row);};td.append(button);tr.append(td);table.append(tr);
        }
        cp('table').append(table);
    }
    function combOptions(){
        cp('comb').replaceChildren(new Option('所有对应组合',''));
        for(const g of catalogData?.catalog.comb_groups||[])if(cp('state').value===''||g.closed_state===Number(cp('state').value))cp('comb').add(new Option(`T${g.template??'—'} · ${g.triggers.length} Uses`,String(g.id)));
        instanceOptions();
    }
    function instanceOptions(){
        cp('instance').replaceChildren(new Option('示例实例',''));
        for(const [i,t] of (catalogData?.catalog.triggers||[]).entries()){
            if(cp('state').value!==''&&t.closed_state!==Number(cp('state').value))continue;
            if(cp('comb').value!==''&&!catalogData.catalog.comb_groups[Number(cp('comb').value)].triggers.includes(i))continue;
            cp('instance').add(new Option(`U${t.origin?.use_id??i} · T${t.origin?.template??'—'}`,String(i)));
        }
    }
    function selectedTrigger(){
        const i=cp('instance').value||cp('instance').options[1]?.value;
        return i===undefined?null:catalogData.catalog.triggers[Number(i)];
    }
    function closedDiagram(){
        const kind=cp('diagram').value;
        if(kind==='comb')return {g:catalogGraph(),caption:'rule comb → ClosedState；选择组合和触发实例可查看实际来源。'};
        // Both remaining diagrams describe exactly one ClosedState. With the overview
        // selected they used to render an empty canvas, which reads as "the selector did
        // nothing". Fall back to the first state and keep the selector in sync.
        let state=cp('state').value,auto=false;
        if(state===''&&catalogData.catalog.closed_states>0){state='0';cp('state').value=state;combOptions();auto=true;}
        if(state==='')return {g:{nodes:[],edges:[]},caption:'本轮没有可显示的 ClosedState：ripen 队列可能全部 Pending / Suspended，Pending 不代表已闭合。'};
        const autoNote=auto?'（概览下已自动选择 C0；此图按单个 ClosedState 显示。）':'';
        if(kind==='egraph'){
            const data=catalogData.states[Number(state)],q=JSON.stringify;
            const lines=['digraph G {rankdir=LR; node [shape=box];'];
            const nodes=[];
            data.values.forEach((v,i)=>{
                nodes.push({id:'v'+i,detail:{class:i,...v}});
                lines.push(`subgraph cluster_${i} {label=${q(v.sort+' · class '+i)};color="#97a6ba"; v${i} [label=${q(v.literal??'EClass '+i)},shape=ellipse];`);
                data.rows.forEach((r,j)=>{if(r.result===i){nodes.push({id:'n'+j,detail:r});lines.push(`n${j} [label=${q(r.op)},style=filled,fillcolor="#dff2ec"];`);}});
                lines.push('}');
            });
            data.rows.forEach((r,j)=>r.args.forEach((a,k)=>lines.push(`n${j} -> v${a} [label=${q('arg '+k)}];`)));
            for(const [name,v] of Object.entries(data.ports||{}))lines.push(`p${v} [label=${q(name)},shape=plaintext]; p${v} -> v${v};`);
            lines.push('}');
            return {g:{nodes,edges:[]},source:lines.join('\n'),caption:`C${state} · ${data.values.filter(v=>v.literal===null).length} eclasses / ${data.rows.length} constructor rows；框内为同一 class 的 enodes。RipenInput 是符号边界参数，不是原始数据。${autoNote}`};
        }
        const trigger=selectedTrigger(),members=trigger?.binding_origin?.comb_members||[];
        const nodes=[],edges=[],groups=new Map();
        for(const m of members){
            const layer=m.source_smooth_layer===null||m.source_smooth_layer===undefined?'C'+m.source_coarse_layer:'S'+m.source_smooth_layer;
            const coarse='C'+m.source_coarse_layer;
            if(!groups.has(coarse))groups.set(coarse,new Map());
            const children=groups.get(coarse);if(!children.has(layer))children.set(layer,[]);children.get(layer).push('m'+m.slot);
            nodes.push({id:'m'+m.slot,label:`${m.rule_name} · ${m.use_kind}\n原始 ${m.source_kind} · C${m.source_coarse_layer} / S${m.source_smooth_layer??'—'}`,kind:m.source_kind==='CoarseComb'?'coarse':'smooth',detail:m});
            for(const parent of m.parents)edges.push({from:'m'+parent,to:'m'+m.slot,label:'recorded dependency'});
        }
        let source=dot({nodes,edges}).slice(0,-1);
        for(const [coarse,children] of groups){
            source+=`\nsubgraph cluster_${coarse} {label=${JSON.stringify('来源 Coarse layer '+coarse+' · 参与部分')};`;
            for(const [layer,ids] of children){
                const members=ids.map(x=>JSON.stringify(x)).join(';')+';';
                source+=layer===coarse?members:`subgraph cluster_${coarse}_${layer} {label=${JSON.stringify('来源 Smooth layer '+layer+' · 参与部分')};${members}}`;
            }source+='}';
        }
        source+='}';
        return {g:{nodes,edges},source,caption:`U${trigger?.origin?.use_id??'—'}：原始 layer 中参与此 rule comb 的成员；不是整个 layer。Use 内的 Coarse/Smooth 分类与原始 layer 分类可能不同。${autoNote}`};
    }
    function focusComb(g){cp('state').value=String(g.closed_state);combOptions();cp('comb').value=String(g.id);instanceOptions();renderCatalog();showComb(g);}
    function memberText(m){
        const aliases=(m.input_roles||[]).map((r,i)=>`${r}=v${m.aliases?.[i]??'?'}`).join(', ');
        return `${m.use_kind} · ${m.rule_name}\n观测别名（模板内）: ${aliases}\n示例来源 layer: C${m.source_coarse_layer} / ${m.source_smooth_layer===null?'—':'S'+m.source_smooth_layer}\n${m.rule}`;
    }
    function showComb(g){
        const instances=g.triggers.map(i=>{const t=catalogData.catalog.triggers[i];return {origin:t.origin,source:t.source,value_map:t.value_map,comb_members:t.binding_origin?.comb_members};});
        cp('details').textContent=JSON.stringify({template:g.template,instances},null,2);
        cp('rule').textContent=(selectedTrigger()?.binding_origin?.comb_members||g.members).map(memberText).join('\n\n');
    }
    async function renderCatalog(){
        const v=++catalogVersion,c=catalogData.catalog;cp('view').dataset.ready='false';cp('rule').textContent='';
        try{
            let source=catalogData.dot,g=null;
            if(c.comb_groups){const diagram=closedDiagram();g=diagram.g;source=diagram.source||dot(g);cp('caption').textContent=diagram.caption;catalogTable();}cp('view').dataset.dot=source;
            const markup=await (await post('render',{kind:'dot',source})).text();if(v!==catalogVersion)return;
            cp('view').replaceChildren();const svg=mountSvg(markup,cp('view'));if(g&&g.nodes.length<=10){svg.style.maxWidth='100%';svg.style.height='auto';}cp('view').dataset.ready='true';
            for(const node of svg.querySelectorAll('.node')){
                const id=node.querySelector('title')?.textContent||'';node.style.cursor='pointer';
                node.onclick=()=>{
                    if(cp('diagram').value!=='comb'){const d=g?.nodes.find(n=>n.id===id)?.detail;cp('details').textContent=JSON.stringify(d,null,2);if(d?.rule)cp('rule').textContent=memberText(d);return;}
                    if(!g){cp('details').textContent=JSON.stringify(id.startsWith('t')?c.triggers[Number(id.slice(1))]:catalogData.states[Number(id.slice(1))],null,2);return;}
                    if(id.startsWith('c')){const state=Number(id.slice(1));cp('details').textContent=JSON.stringify({state:catalogData.states[state],groups:c.comb_groups.filter(g=>g.closed_state===state)},null,2);if(cp('state').value===''){cp('state').value=String(state);combOptions();renderCatalog();}}
                    else if(/^g\d+$/.test(id))focusComb(c.comb_groups[Number(id.slice(1))]);
                    else {const d=g.nodes.find(n=>n.id===id)?.detail;if(d){cp('details').textContent=JSON.stringify(d,null,2);cp('rule').textContent=memberText(d.member);}}
                };
            }
        }catch(e){cp('status').textContent=String(e);}
    }
    cp('diagram').onchange=()=>{if(catalogData)renderCatalog();};
    cp('instance').onchange=()=>{if(catalogData){renderCatalog();const g=catalogData.catalog.comb_groups?.find(g=>g.triggers.includes(Number(cp('instance').value)));if(g)showComb(g);}};
    cp('state').onchange=()=>{if(catalogData){combOptions();renderCatalog();}};
    cp('comb').onchange=()=>{if(!catalogData)return;const id=cp('comb').value;if(id==='')renderCatalog();else focusComb(catalogData.catalog.comb_groups[Number(id)]);};
    cp('load').onclick=async()=>{
        cp('load').disabled=true;cp('state').disabled=true;cp('comb').disabled=true;catalogVersion++;catalogData=null;catalogError=null;cp('table').replaceChildren();cp('view').replaceChildren();cp('rule').textContent='';cp('status').textContent='载入…';cp('details').textContent='';
        try{
            catalogData=await (await post('closed-catalog',{path:cp('path').value})).json();const c=catalogData.catalog;
            cp('state').replaceChildren(new Option('全部概览',''));
            for(let i=0;i<c.closed_states;i++)cp('state').add(new Option(`ClosedState C${i}`,String(i)));
            const requestedState=new URLSearchParams(location.search).get('closed_state');
            if(cp('path').value===initialCatalog&&requestedState!==null&&/^\d+$/.test(requestedState)&&Number(requestedState)<c.closed_states)cp('state').value=requestedState;
            combOptions();
            const templates=new Set(c.triggers.filter(t=>t.origin?.template!==undefined).map(t=>JSON.stringify([t.origin.history,t.origin.template]))).size;
            cp('status').textContent=`${c.triggers.length} 个 Trigger → ${c.closed_states} 个共享 ClosedState，涉及 ${templates} 种模板。未决比较 ${c.unresolved_comparisons||0}。`;
            if(c.scan)cp('status').textContent+=` 已检查 ${c.scan.attempts.length}/${c.scan.population.historical_uses} 个历史 Use；${JSON.stringify(c.scan.counts)}。预算 ${c.scan.budgets.rounds} 轮 / 每例 ${c.scan.budgets.seconds_per_use} 秒。跨模板共享组 ${c.scan.shared_template_states}。`;
            await renderCatalog();
        }catch(e){catalogError=String(e);cp('status').textContent=catalogError;}finally{cp('load').disabled=false;cp('state').disabled=false;cp('comb').disabled=false;}
    };
    const initialCatalog=new URLSearchParams(location.search).get('closed_catalog');

    host.append(panel);
    const $=id=>panel.querySelector('#native-layer-'+id);
    let frames=[],version=0,abort=null,previewCache=new Map(),pinned=false,rendered=false;
    const frame=()=>frames[Number($('round').value)];
    // Three different "nothing to show" cases need three different answers: nothing loaded
    // at all, snapshots taken before the ClosedState pipeline existed, or a run in which no
    // Use actually closed. Only the first is fixed by running again.
    const noClosedHint=()=>frames.length&&!frames.some(f=>f.closed)?STALE_SNAPSHOTS:NO_CATALOG;
    function mode(){
        const closed=$('kind').value==='closed';catalogPanel.hidden=!closed;
        for(const id of ['scope','format'])$(id).hidden=closed;$('round').hidden=closed&&!frames.length;
        for(const id of ['viewport','details','code']){const e=$(id);(e.closest('details')===panel?e:e.closest('details')||e).hidden=closed;}
        $('status').hidden=closed;$('source').disabled=!frame();
        $('directory').placeholder=closed?'out/ 中的共享目录或含 catalog/ 的运行目录':'out/ 中的分析目录';
        $('load').textContent=closed?'载入 ClosedState':'载入分析目录';
        $('download').textContent=closed?'下载当前 ClosedState DOT':'下载本轮 DOT';
        // Never overwrite a load failure with the generic hint: that used to erase the one
        // message telling the user why nothing rendered.
        if(closed&&!catalogData&&!catalogError&&!frame()?.closed)cp('status').textContent=noClosedHint();
    }
    function showClosedFrame(result){
        catalogVersion++;catalogData=result.catalog;
        cp('state').replaceChildren(new Option('全部概览',''));
        cp('table').replaceChildren();cp('view').replaceChildren();cp('rule').textContent='';
        cp('details').textContent=JSON.stringify(result.jobs,null,2);
        cp('status').textContent=`边界 ${result.boundary} · ${Object.entries(result.counts).map(([k,v])=>k+' '+v).join(' / ')||'尚无 Use'} · 队列外 ${result.not_queued}。每边界最多 ${result.limits.per_boundary} 个，总计 ${result.limits.jobs} 个；每例 ${result.limits.rounds} 轮。时间预算在任务之间检查。`;
        if(catalogData){
            for(let i=0;i<catalogData.catalog.closed_states;i++)cp('state').add(new Option(`ClosedState C${i}`,String(i)));
            cp('status').textContent+=` ${catalogData.catalog.triggers.length} 个 Trigger → ${catalogData.catalog.closed_states} 个共享 ClosedState。`;
            combOptions();if($('kind').value==='closed')renderCatalog();
        }else{
            combOptions();
            cp('status').textContent+=' 本轮没有任何 Use 达到 Closed（Pending / Suspended 不代表已闭合）；本次运行的数据已在内存里，无需手动载入目录。';
            // A run that rejects every queued job looks like "nothing happened" unless the
            // queue's own reason is surfaced here.
            const tally=new Map();
            for(const j of result.jobs||[])if(j.reason)tally.set(j.reason,(tally.get(j.reason)||0)+1);
            const top=[...tally.entries()].sort((a,b)=>b[1]-a[1])[0];
            if(top)cp('status').textContent+=` 最常见拒绝原因（${top[1]} 个）：${top[0].split('\n')[0].slice(0,200)}`;
        }
    }
    function clearCatalog(){catalogVersion++;catalogData=null;cp('path').value='';for(const id of ['view','table','details','rule','caption'])cp(id).replaceChildren();cp('state').replaceChildren(new Option('全部概览',''));combOptions();}

    // A ?layer_run=... from a generated fractal.html link seeds the directory box and reloads
    // that directory on every refresh, so a stale path looked like a hardcoded default and no
    // other option could shake it. Once new data arrives it is no longer the current context:
    // drop it from the URL and clear the box only if it still holds that exact value, so a
    // path the user typed themselves is left alone.
    function dropStaleDirectory(){
        const seeded=new URLSearchParams(location.search).get('layer_run');
        if(!seeded||$('directory').value!==seeded)return;
        $('directory').value='';
        const url=new URL(location.href);url.searchParams.delete('layer_run');history.replaceState(null,'',url);
    }
    function reset(){dropStaleDirectory();clearCatalog();mode();abort?.abort();version++;frames=[];pinned=false;rendered=false;previewCache.clear();$('round').replaceChildren();$('scope').replaceChildren(new Option('全部',''));$('viewport').replaceChildren();$('details').textContent='';$('code').textContent='';$('status').textContent='等待实际执行边界…';}
    function receive(f){
        if(f.kind!=='layer_snapshot'||!f.graphs||!f.analysis)throw Error('无效的 layer 快照');
        frames.push(f);$('round').add(new Option(f.label,String(frames.length-1)));
        // New data does not start expensive render subprocesses automatically.
        if(!pinned){$('round').value=String(frames.length-1);options(false);}
    }
    function options(pin=true){
        mode();
        if(frame()?.closed)showClosedFrame(frame().closed);
        if($('kind').value==='closed')return;
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
        if(g.sinks?.length)lines.push('{rank=sink;'+g.sinks.map(q).join(';')+';}');
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
        if($('kind').value==='closed'){mode();if(catalogData)await renderCatalog();else if(!catalogError)cp('status').textContent=noClosedHint();return;}
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
    $('download').onclick=()=>{if($('kind').value==='closed'){if(!catalogData){cp('status').textContent=catalogError||noClosedHint();return;}const a=document.createElement('a'),url=URL.createObjectURL(new Blob([cp('view').dataset.dot||dot(catalogGraph())],{type:'text/vnd.graphviz;charset=utf-8'}));a.href=url;a.download='closed-state.dot';a.click();setTimeout(()=>URL.revokeObjectURL(url),1000);return;}const f=frame();if(!f)return;const kind=$('kind').value,a=document.createElement('a'),url=URL.createObjectURL(new Blob([f.dots[kind]],{type:'text/vnd.graphviz;charset=utf-8'}));a.href=url;a.download=f.stem+'.'+kind+'.dot';a.click();setTimeout(()=>URL.revokeObjectURL(url),1000);};
    async function load(path){
        const controls=['load','render','round','kind','scope','format'];controls.forEach(id=>$(id).disabled=true);
        try {const data=await (await post('layer-run',{path})).json();reset();for(const f of data.frames)receive(f);if(data.closed_catalog&&!data.frames.some(f=>f.closed)){cp('path').value=data.closed_catalog;await cp('load').onclick();}mode();}
        finally {controls.forEach(id=>$(id).disabled=false);}
    }
    $('load').onclick=()=>{if($('kind').value==='closed'){cp('path').value=$('directory').value;return cp('load').onclick();}return load($('directory').value).catch(e=>$('error').textContent=e.message);};
    $('source').onclick=()=>{if(frame())editor.setValue(frame().preview_source);};
    const initialKind=new URLSearchParams(location.search).get('layer_kind');if(['layers','fractals','coverage','reuse','use_fractals','closed'].includes(initialKind))$('kind').value=initialKind;
    if(initialCatalog&&!new URLSearchParams(location.search).get('layer_run')){$('kind').value='closed';$('directory').value=initialCatalog;cp('path').value=initialCatalog;cp('load').click();}mode();
    const initial=new URLSearchParams(location.search).get('layer_run');if(initial){$('directory').value=initial;load(initial).then(async()=>{if(initialCatalog){$('kind').value='closed';cp('path').value=initialCatalog;await cp('load').onclick();mode();}return render();}).catch(e=>$('error').textContent=e.message);}
    // Re-render the current selection against the current source. Called after an
    // editor change so an edited annotation template is visible without re-running.
    async function refresh(){if(rendered&&frames.length)await render();}
    return {receive,reset,load,refresh,get snapshots(){return structuredClone(frames);}};
}
