// Native debugging is independent of the upstream WASM run and its egraph view.
export function installNativeDebugger(editor) {
    window.nativeDebugger = true;
    const link = document.createElement('link'); link.rel='stylesheet'; link.href='native-debugger.css'; document.head.append(link);
    const panel = document.createElement('section'); panel.id='native-debugger';
    panel.innerHTML = `<strong>Native Rule Compose / Fractal</strong>
      <button id="native-run">运行并识别</button><button id="native-stop" disabled>停止</button>
      <button id="native-export">导出日志</button><button id="native-restore">载入日志源码</button><label>回放日志 <input id="native-import" type="file" accept=".json"></label>
      <div id="native-status">点击 .egg 规则任意一行查看 Pattern。原生识别使用本地 egg_layout（自包含 Math datatype）。</div>
      <select id="native-filter"><option value="all">所有日志</option><option value="application">有效应用</option><option value="compose">Rule Compose</option><option value="fractal">Fractal</option></select>
      <div id="native-trace" role="log" aria-label="增量识别日志"></div>
      <div><select id="native-format"><option value="typst">Typst 公式</option><option value="dot">DOT 图</option></select><span id="native-title"></span></div>
      <div id="native-viewport"><img id="native-preview" alt="所选规则的预览" hidden></div>
      <pre id="native-render-error"></pre>
      <details><summary>Typst / DOT 源码</summary><pre id="native-source"></pre></details>
      <details><summary>绑定、effect 与路径证据</summary><pre id="native-details"></pre></details>`;
    document.getElementById('panel').insertBefore(panel, document.getElementById('graph'));
    const el = id => document.getElementById('native-'+id);
    let rows=[], snapshotSource='', runStatus='idle', selected=null, patterns=[], parsedSource=null, revision=0, renderRevision=0, selectionIntent=0;
    let controller=null, timer=null, marker=null, objectUrl=null, previewAbort=null, parseAbort=null;
    const status = text => {el('status').textContent=text;};
    async function post(path, body, signal) {
        const response=await fetch('/api/'+path,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body),signal});
        if (!response.ok) {let error;try {error=(await response.json()).error;}catch {error='请通过 tools/egglog_debugger/server.py 启动本地调试服务';}throw Error(error);}
        return response;
    }
    async function show(row, fromTrace=false) {
        selected=row; const version=++renderRevision;
        previewAbort?.abort(); previewAbort=new AbortController();
        const kind=el('format').value;
        el('title').textContent=`${row.kind || 'pattern'} · ${row.rule || ''} · L${row.source_line || '?'}`;
        el('source').textContent=row[kind] || '';
        el('details').textContent=JSON.stringify(row,null,2);
        el('preview').hidden=true; el('render-error').textContent=row.reason || '';
        marker?.clear(); marker=null;
        if (fromTrace && editor.getValue()===snapshotSource && row.source_line) {
            marker=editor.markText({line:row.source_line-1,ch:0},{line:row.end_line || row.source_line,ch:0},{className:'native-source-selection'});
            // Scrolling does not change the cursor, so replay cannot trigger a pattern request.
            editor.scrollIntoView({line:row.source_line-1,ch:0},80);
        }
        if (!row[kind]) return;
        try {
            const response=await post('render',{kind,source:row[kind]},previewAbort.signal);
            const blob=await response.blob();
            if(version!==renderRevision)return;
            if(objectUrl) URL.revokeObjectURL(objectUrl);
            objectUrl=URL.createObjectURL(blob);el('preview').src=objectUrl;el('preview').hidden=false;
        }catch(error){if(version===renderRevision && error.name!=='AbortError')el('render-error').textContent=error.message;}
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
            if(row) await show(row);
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
    el('format').onchange=()=>{if(selected)show(selected);};
    el('run').onclick=async()=>{
        controller?.abort();controller=new AbortController();const current=controller;
        snapshotSource=editor.getValue();runStatus='running';rows=[];el('trace').replaceChildren();status('运行实际 egglog runtime，等待增量事件…');el('run').disabled=true;el('stop').disabled=false;el('import').disabled=true;
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
    el('export').onclick=()=>{const url=URL.createObjectURL(new Blob([JSON.stringify({version:1,status:runStatus,source:snapshotSource,rows},null,2)],{type:'application/json'}));const a=document.createElement('a');a.href=url;a.download='egglog-debug-history.json';a.click();setTimeout(()=>URL.revokeObjectURL(url),1000);};
    el('import').onchange=async event=>{try{const file=event.target.files[0];if(!file)return;const data=JSON.parse(await file.text());if(data.version!==1 || typeof data.source!=='string' || !Array.isArray(data.rows) || data.rows.some(r=>!['application','compose','fractal'].includes(r.kind) || typeof r.id!=='string'))throw Error('无效的日志格式');controller?.abort();snapshotSource=data.source;runStatus=data.status || 'unknown';rows=data.rows;selected=null;el('preview').hidden=true;el('filter').onchange();status(`已载入 ${rows.length} 条公式快照（${runStatus}）；点击日志回放。`);}catch(error){status(error.message);}event.target.value='';};
    // Expose read-only state for debugging and browser regression tests.
    window.egglogNative={get rows(){return structuredClone(rows);},get selected(){return structuredClone(selected);}};
}
