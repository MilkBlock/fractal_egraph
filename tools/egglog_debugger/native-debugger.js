import { applyTypstRenderings } from "./plugin-overlay.js";
// Native debugging is independent of the upstream WASM run and its egraph view.
export function installNativeDebugger(editor) {
    window.nativeDebugger = true;
    const link = document.createElement('link'); link.rel='stylesheet'; link.href='native-debugger.css'; document.head.append(link);
    const panel = document.createElement('section'); panel.id='native-debugger';
    panel.innerHTML = `<strong>Native Rule Compose / Fractal</strong>
      <button id="native-run">运行并识别</button><button id="native-stop" disabled>停止</button>
      <button id="native-download">下载 .egg</button><button id="native-export">导出日志</button><button id="native-restore">载入日志源码</button><label>回放日志 <input id="native-import" type="file" accept=".json"></label>
      <div id="native-status">点击 .egg 规则任意一行查看 Pattern。原生识别使用本地 egg_layout（自包含 Math datatype）。</div>
      <select id="native-filter"><option value="all">所有日志</option><option value="application">有效应用</option><option value="compose">Rule Compose</option><option value="fractal">Fractal</option></select>
      <div id="native-trace" role="log" aria-label="增量识别日志"></div>
      <div><select id="native-format"><option value="typst">Typst 公式</option><option value="dot">DOT 图</option></select><span id="native-title"></span></div>
      <div id="native-render-options">
        <select id="native-dot-mode" aria-label="DOT view"><option value="combined">action + pattern.dot</option><option value="pattern">pattern.dot</option><option value="action">action.dot</option></select>
        <select id="native-label-style" aria-label="Label style"><option value="recursive">recursive</option><option value="compact">compact</option><option value="full">full</option></select>
        <select id="native-recursive" aria-label="Recursive strategy"><option value="dag-expand">dag-expand</option><option value="tree-safe">tree-safe</option></select>
        <select id="native-step" aria-label="组合中的步骤" hidden></select>
      </div>
      <div id="native-evidence"></div><div id="native-renderer"></div>
      <div id="native-viewport"><div id="native-preview" role="img" aria-label="所选规则的插件预览" hidden></div></div>
      <div id="native-edit-hint"></div>
      <form id="native-name-editor" hidden>
        <label id="native-edit-label" for="native-name-input">显示名称</label>
        <input id="native-name-input" maxlength="100" required autocomplete="off">
        <button type="submit" id="native-name-save">保存到 .egg 注释</button><button type="button" id="native-name-cancel">取消</button>
        <div id="native-edit-scope"></div><div id="native-field-editor"></div><div id="native-edit-error" role="alert"></div>
      </form>
      <pre id="native-render-error"></pre>
      <details><summary>Typst / DOT 源码</summary><pre id="native-source"></pre></details>
      <details><summary>绑定、effect 与路径证据</summary><pre id="native-details"></pre></details>`;
    document.getElementById('panel').insertBefore(panel, document.getElementById('graph'));
    const el = id => document.getElementById('native-'+id);
    let rows=[], snapshotSource='', runStatus='idle', selected=null, patterns=[], parsedSource=null, revision=0, renderRevision=0, selectionIntent=0;
    let controller=null, timer=null, marker=null, previewAbort=null, parseAbort=null, currentEdit=null;
    const status = text => {el('status').textContent=text;};
    async function post(path, body, signal) {
        const response=await fetch('/api/'+path,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body),signal});
        if (!response.ok) {let error;try {error=(await response.json()).error;}catch {error='请通过 tools/egglog_debugger/server.py 启动本地调试服务';}throw Error(error);}
        return response;
    }
    function mountSvg(markup) {
        const doc=new DOMParser().parseFromString(markup,'image/svg+xml');
        if(doc.querySelector('parsererror') || doc.documentElement.localName!=='svg')throw Error('插件返回无效的 SVG');
        // Imported history is data, not executable HTML. Keep local SVG references
        // and formula image data, but never execute scripts or external URLs.
        doc.querySelectorAll('script,foreignObject,style').forEach(node=>node.remove());
        for(const node of doc.querySelectorAll('*'))for(const attr of [...node.attributes]){
            if(attr.name.toLowerCase().startsWith('on') ||
                (attr.localName==='href' && !attr.value.startsWith('#') && !attr.value.startsWith('data:image/')))
                node.removeAttributeNode(attr);
        }
        const svg=document.importNode(doc.documentElement,true);
        el('preview').replaceChildren(svg);el('preview').hidden=false;
        return svg;
    }
    function closeNameEditor(){currentEdit=null;el('name-editor').hidden=true;}
    function installEditTargets(svg, rendered, request, row){
        el('edit-hint').textContent=row.kind?'日志是只读快照；请载入源码并点击编辑器中的规则进行编辑。':'点击公式中带高亮的名称修改显示文本；修改会写回 .egg 注释。';
        if(row.kind || el('format').value!=='typst')return;
        const targets=new Map((rendered.edit_targets || []).map(target=>[target.id,target]));
        for(const region of rendered.edit_regions || []){
            const target=targets.get(region.target_id);if(!target)continue;
            const rect=document.createElementNS('http://www.w3.org/2000/svg','rect');
            rect.classList.add('native-edit-hit');rect.dataset.target=target.id;
            rect.setAttribute('x',Math.max(0,region.x-1));rect.setAttribute('y',Math.max(0,region.y-1));
            rect.setAttribute('width',region.width+2);rect.setAttribute('height',region.height+2);
            rect.setAttribute('rx','1');rect.setAttribute('tabindex','0');rect.setAttribute('role','button');
            rect.setAttribute('aria-label',`编辑 ${region.text}`);
            const open=()=>{
                if(editor.getValue()!==request.source){status('源码已变化，请等待新公式渲染后再编辑。');return;}
                currentEdit={source:request.source,line:request.line,target_id:target.id};
                el('edit-label').textContent=`显示名称 · ${region.text}`;
                el('name-input').value=region.text;el('edit-error').textContent='';
                el('edit-scope').textContent=target.kind==='constructor'?`更新 ${target.name} 的 dsl_type 显示模板，作用于本文件中的该构造器。`:`更新本条规则中 ${target.name} 的 labels.bindings 显示名称。`;
                el('field-editor').replaceChildren();
                if(target.kind==='constructor' && target.field_labels?.length){
                    const title=document.createElement('div');title.textContent='字段名称（用于模板占位符）';el('field-editor').append(title);
                    target.field_labels.forEach((field,index)=>{const label=document.createElement('label');label.className='native-field-row';label.textContent=`字段 ${index+1}`;const input=document.createElement('input');input.className='native-field-name';input.value=field;input.maxLength=80;input.required=true;input.dataset.index=index;label.append(input);el('field-editor').append(label);});
                }
                el('name-editor').hidden=false;el('name-input').focus();el('name-input').select();
            };
            rect.addEventListener('click',open);rect.addEventListener('keydown',event=>{if(event.key==='Enter' || event.key===' '){event.preventDefault();open();}});
            svg.append(rect);
        }
        if(rendered.edit_error)el('edit-hint').textContent='当前公式的文字定位失败；原始插件预览仍可查看。';
    }
    el('name-cancel').onclick=closeNameEditor;
    el('name-editor').addEventListener('keydown',event=>{if(event.key==='Escape'){event.preventDefault();closeNameEditor();}});
    el('name-input').onkeydown=event=>{if(event.key==='Escape'){event.preventDefault();closeNameEditor();}};
    el('name-editor').onsubmit=async event=>{
        event.preventDefault();const edit=currentEdit;if(!edit)return;
        if(editor.getValue()!==edit.source){el('edit-error').textContent='源码已变化，请取消并重新点击公式；未覆盖你的修改。';return;}
        el('name-save').disabled=true;el('edit-error').textContent='';
        try{
            const fields=[...el('field-editor').querySelectorAll('.native-field-name')].map(input=>input.value);
            const result=await(await post('edit-display',{...edit,value:el('name-input').value,fields:fields.length?fields:undefined})).json();
            if(currentEdit!==edit)return;
            if(editor.getValue()!==edit.source)throw Error('源码已变化，未写入旧版本的修改。');
            editor.operation(()=>{
                editor.replaceRange(result.source,{line:0,ch:0},editor.posFromIndex(edit.source.length),'+display-annotation');
                editor.setCursor({line:result.line-1,ch:0});
            });
            closeNameEditor();clearTimeout(timer);status('显示注释已写回 .egg；可用编辑器撤销，也可下载源码。');
            await previewLine();
        }catch(error){el('edit-error').textContent=error.message;}
        finally{el('name-save').disabled=false;}
    };
    el('download').onclick=()=>{
        const url=URL.createObjectURL(new Blob([editor.getValue()],{type:'text/plain;charset=utf-8'}));
        const link=document.createElement('a');link.href=url;link.download='egglog-preview.egg';link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);
    };
    function selectSteps(row) {
        const steps=el('step');steps.replaceChildren();
        if(row.composition_mode==='flattened rule')steps.add(new Option('组合后的规则','combined'));
        for(const step of row.step_details || [])steps.add(new Option(`event ${step.event} · ${step.rule} · L${step.source_line}`,String(step.event)));
        steps.hidden=!steps.options.length;
        if(steps.options.length)steps.value=row.composition_mode==='flattened rule'?'combined':steps.options[steps.options.length-1].value;
    }
    function previewRequest(row) {
        let source=row.kind?snapshotSource:row.preview_source;
        let line=row.source_line;
        const step=(row.step_details || []).find(step=>String(step.event)===el('step').value);
        if(row.composition_mode==='flattened rule' && el('step').value==='combined'){
            line=source.split('\n').length+1;
            source+='\n'+row.source;
        }else if(step){line=step.source_line;}
        return {source,line,mode:el('dot-mode').value,label_style:el('label-style').value,recursive_strategy:el('recursive').value};
    }
    async function show(row, fromTrace=false) {
        closeNameEditor();
        const changed=selected!==row;selected=row; const version=++renderRevision;
        if(changed || fromTrace)selectSteps(row);
        previewAbort?.abort(); previewAbort=new AbortController();
        const kind=el('format').value;
        el('title').textContent=`${row.kind || 'pattern'} · ${row.rule || ''} · L${row.source_line || '?'}`;
        el('source').textContent='';
        const {plugin_previews,typst,dot,preview_source,...details}=row;el('details').textContent=JSON.stringify(details,null,2);
        el('preview').hidden=true;el('preview').dataset.ready='false';el('render-error').textContent='';
        el('renderer').textContent='加载 Eggplant VS Code 插件渲染…';
        el('evidence').textContent=row.evidence?`${row.evidence.higher} · 观察到的事件 ${row.evidence.events.join(', ')}`:(row.reason || '');
        marker?.clear(); marker=null;
        if(fromTrace && editor.getValue()===snapshotSource && row.source_line){
            marker=editor.markText({line:row.source_line-1,ch:0},{line:row.end_line || row.source_line,ch:0},{className:'native-source-selection'});
            editor.scrollIntoView({line:row.source_line-1,ch:0},80);
        }
        try{
            const request=previewRequest(row);
            if(!request.source || !request.line)throw Error('此旧日志缺少源位置，不能可靠地交给插件渲染；请重新运行生成日志。');
            const key=JSON.stringify([el('step').value,request.line,request.mode,request.label_style,request.recursive_strategy]);
            row.plugin_previews ||= {};
            const rendered=row.plugin_previews[key] || await (await post('preview',request,previewAbort.signal)).json();
            if(version!==renderRevision)return;
            row.plugin_previews[key]=rendered;
            el('source').textContent=rendered[kind];
            const svg=mountSvg(kind==='typst'?rendered.typst_svg:rendered.dot_svg);
            if(kind==='dot')applyTypstRenderings(svg,rendered.typst_renderings);
            installEditTargets(svg,rendered,request,row);
            el('renderer').textContent=`${rendered.renderer} · ${rendered.renderer_revision.slice(0,12)} · ${rendered.config.mode} / ${rendered.config.label_style} / ${rendered.config.recursive_strategy}`;
            if(kind==='typst' && rendered.typst_mode!=='math')el('render-error').textContent='插件使用了文本 fallback；请检查 Typst 模板。';
            el('preview').dataset.ready='true';
        }catch(error){if(version===renderRevision && error.name!=='AbortError'){el('render-error').textContent=error.message;el('renderer').textContent='插件渲染失败';}}
    }
    async function previewLine() {
        const source=editor.getValue(), version=revision, intent=++selectionIntent;
        try {
            if(parsedSource!==source){
                parseAbort?.abort();parseAbort=new AbortController();
                const data=await (await post('patterns',{source},parseAbort.signal)).json();
                if(version!==revision || intent!==selectionIntent)return;
                patterns=data.patterns;parsedSource=source;
            }
            const line=editor.getCursor().line+1;
            const row=patterns.find(p=>p.source_line<=line && line<=p.end_line);
            if(row){row.preview_source=source;await show(row);}
            else {selected=null;++renderRevision;previewAbort?.abort();marker?.clear();el('title').textContent=`L${line} 没有 rule/rewrite`;el('preview').hidden=true;el('source').textContent='';el('details').textContent='';el('render-error').textContent='';}
        }catch(error){if(version===revision && intent===selectionIntent && error.name!=='AbortError'){selected=null;el('render-error').textContent=error.message;el('preview').hidden=true;}}
    }
    editor.on('cursorActivity',()=>{clearTimeout(timer);timer=setTimeout(previewLine,120);});
    editor.on('mousedown',()=>{clearTimeout(timer);timer=setTimeout(previewLine,120);});
    editor.on('change',()=>{revision++;clearTimeout(timer);timer=setTimeout(previewLine,300);if(rows.length)status('源码已修改；日志仍保存上次运行的公式快照，重新运行可更新识别。');});
    function addRow(row) {
        const filter=el('filter').value;if(filter!=='all' && filter!==row.kind)return;
        const button=document.createElement('button');button.dataset.id=row.id;button.setAttribute('aria-pressed','false');
        button.textContent=`B${row.boundary} · ${row.kind} · L${row.source_line} · ${row.rule} · ${row.id}`;
        button.onclick=()=>{panel.querySelectorAll('#native-trace button').forEach(b=>b.setAttribute('aria-pressed','false'));button.setAttribute('aria-pressed','true');clearTimeout(timer);++selectionIntent;show(row,true);};
        el('trace').append(button);
    }
    el('filter').onchange=()=>{el('trace').replaceChildren();rows.forEach(addRow);};
    for(const control of ['format','dot-mode','label-style','recursive','step'])el(control).onchange=()=>{if(selected)show(selected);};
    el('run').onclick=async()=>{
        controller?.abort();controller=new AbortController();const current=controller;
        snapshotSource=editor.getValue();runStatus='running';rows=[];selected=null;el('preview').hidden=true;el('trace').replaceChildren();status('运行实际 egglog runtime，等待增量事件…');el('run').disabled=true;el('stop').disabled=false;el('import').disabled=true;
        try {
            const response=await post('trace',{source:snapshotSource},current.signal);
            const reader=response.body.getReader(),decoder=new TextDecoder();let buffer='',complete=false;
            const consume=line=>{if(!line.trim())return;const row=JSON.parse(line);
                if(row.kind==='error')throw Error(row.error);
                if(row.kind==='complete'){complete=true;runStatus='complete';status(`完成：${rows.filter(r=>r.kind==='application').length} 个有效应用，${rows.filter(r=>r.kind==='compose').length} 个组合，${rows.filter(r=>r.kind==='fractal').length} 个 Fractal 证据。`);}
                else if(row.kind==='boundary')status(`执行边界 ${row.boundary}：${row.applications} 个有效应用；${row.logical_matches} 个逻辑匹配，排除 ${row.excluded} 个。`);
                else {rows.push(row);addRow(row);}
            };
            while(true){const {value,done}=await reader.read();buffer+=decoder.decode(value || new Uint8Array(),{stream:!done});let newline;while((newline=buffer.indexOf('\n'))>=0){const line=buffer.slice(0,newline);buffer=buffer.slice(newline+1);consume(line);}if(done)break;}
            if(buffer.trim())consume(buffer);if(!complete)throw Error('事件流提前结束，当前日志为部分结果');
        }catch(error){runStatus=error.name==='AbortError'?'cancelled':'failed';status(error.name==='AbortError'?'已停止，保留已收到的日志。':`运行失败：${error.message}`);}
        finally{if(controller===current){el('run').disabled=false;el('stop').disabled=true;el('import').disabled=false;controller=null;}}
    };
    el('restore').onclick=()=>{if(snapshotSource){editor.setValue(snapshotSource);status('已载入日志对应源码；点击日志重现公式。');}};
    el('stop').onclick=()=>controller?.abort();
    el('export').onclick=()=>{const url=URL.createObjectURL(new Blob([JSON.stringify({version:2,status:runStatus,source:snapshotSource,rows},null,2)],{type:'application/json'}));const a=document.createElement('a');a.href=url;a.download='egglog-debug-history.json';a.click();setTimeout(()=>URL.revokeObjectURL(url),1000);};
    el('import').onchange=async event=>{try{const file=event.target.files[0];if(!file)return;const data=JSON.parse(await file.text());if(![1,2].includes(data.version) || typeof data.source!=='string' || !Array.isArray(data.rows) || data.rows.some(r=>!['application','compose','fractal'].includes(r.kind) || typeof r.id!=='string'))throw Error('无效的日志格式');controller?.abort();snapshotSource=data.source;runStatus=data.status || 'unknown';rows=data.rows;selected=null;el('preview').hidden=true;el('filter').onchange();status(`已载入 ${rows.length} 条公式快照（${runStatus}）；点击日志回放。`);}catch(error){status(error.message);}event.target.value='';};
    // Expose read-only state for debugging and browser regression tests.
    window.egglogNative={get rows(){return structuredClone(rows);},get selected(){return structuredClone(selected);}};
}
