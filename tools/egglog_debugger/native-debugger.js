import { applyTypstRenderings } from "./plugin-overlay.js";
// A static deployment (for example GitHub Pages) cannot run the Python/Rust bridge,
// so the page talks to a bridge on this machine instead. Override with
// `window.__EGGLOG_BRIDGE__` before loading this module.
const LOCAL_HOSTS = new Set(['127.0.0.1', 'localhost', '::1', '[::1]']);
const BRIDGE_BASE = window.__EGGLOG_BRIDGE__ ?? (LOCAL_HOSTS.has(location.hostname) ? '' : 'http://127.0.0.1:8080');
const STATIC_PAGE = BRIDGE_BASE !== '';
// Native debugging is independent of the upstream WASM run and its egraph view.
export function installNativeDebugger(editor) {
    window.nativeDebugger = true;
    const link = document.createElement('link'); link.rel='stylesheet'; link.href='native-debugger.css'; document.head.append(link);
    const panel = document.createElement('section'); panel.id='native-debugger';
    panel.innerHTML = `<strong>Native Rule Compose / Fractal</strong>
      <button id="native-run">运行并识别</button><button id="native-stop" disabled>停止</button>
      <button id="native-download">下载 .egg</button><button id="native-export">导出日志</button><button id="native-restore">载入日志源码</button><label>回放日志 <input id="native-import" type="file" accept=".json"></label>
      <div id="native-status">点击 .egg 规则任意一行查看 Pattern。原生识别使用本地 egg_layout（单个自包含 datatype，名字任意）。</div>
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
      <div id="native-targets" role="group" aria-label="当前规则的可编辑目标"></div>
      <form id="native-name-editor" hidden>
        <label id="native-edit-label" for="native-name-input">显示名称</label>
        <input id="native-name-input" maxlength="100" required autocomplete="off">
        <div id="native-edit-scope"></div><div id="native-field-editor"></div>
        <div id="native-template-editor" hidden>
          <label for="native-template-input">Typst 模板（用 {字段} 引用参数）</label>
          <button type="button" id="native-template-help" aria-expanded="false" aria-controls="native-symbol-help" aria-label="常见数学符号与模板">i</button>
          <textarea id="native-template-input" rows="2" spellcheck="false" autocomplete="off"></textarea>
          <div id="native-template-note" role="status"></div>
          <label for="native-precedence-input">优先级</label>
          <input id="native-precedence-input" type="number" min="0" max="65535" step="1">
        </div>
        <button type="submit" id="native-name-save">保存到 .egg 注释</button><button type="button" id="native-name-cancel">取消</button>
        <div id="native-symbol-help" role="dialog" aria-label="常见数学符号与模板" hidden></div>
        <div id="native-edit-error" role="alert"></div>
      </form>
      <form id="native-condition-editor" hidden>
        <label for="native-condition-input">规则附加条件（.egg）</label>
        <textarea id="native-condition-input" rows="4" placeholder="例如 (> a 0)；留空表示没有显式附加条件"></textarea>
        <div>这里编辑真实匹配条件，保存会改变规则的匹配行为。由模式本身产生的约束仍保留在源码中。</div>
        <button id="native-condition-save" type="submit">保存条件到 .egg</button><button id="native-condition-cancel" type="button">取消</button>
        <div id="native-condition-error" role="alert"></div>
      </form>
      <pre id="native-render-error"></pre>
      <details><summary>Typst / DOT 源码</summary><pre id="native-source"></pre></details>
      <details><summary>绑定、effect 与路径证据</summary><pre id="native-details"></pre></details>
      <details><summary>Rust 源码</summary><pre id="native-rust"></pre></details>`;
    document.getElementById('panel').insertBefore(panel, document.getElementById('graph'));
    const el = id => document.getElementById('native-'+id);
    let rows=[], snapshotSource='', runStatus='idle', selected=null, patterns=[], parsedSource=null, revision=0, renderRevision=0, selectionIntent=0, renderedCount=0;
    let controller=null, timer=null, marker=null, previewAbort=null, parseAbort=null, currentEdit=null, activeRequest=null, templateDirty=false;
    const status = text => {el('status').textContent=text;};
    let wasmDebuggerPromise=null, bridgeReady=null, browserRendererPromise=null;
    // The same patched instrumented runtime, compiled to wasm32. The static build
    // uses it when no bridge is running on this machine.
    function loadWasmDebugger(){
        if(!wasmDebuggerPromise)wasmDebuggerPromise=(async()=>{
            const module=await import('./wasm/egglog_debug_wasm.js');
            await module.default();
            return module;
        })();
        return wasmDebuggerPromise;
    }
    async function bridgeAvailable(){
        if(!STATIC_PAGE)return true;
        if(bridgeReady===null){
            try{bridgeReady=(await fetch(BRIDGE_BASE+'/plugin-overlay.js',{method:'GET'})).ok;}catch{bridgeReady=false;}
        }
        return bridgeReady;
    }
    async function listPatterns(source,signal){
        if(await bridgeAvailable())return (await (await post('patterns',{source},signal)).json());
        return JSON.parse((await loadWasmDebugger()).debug_patterns(source));
    }
    // The preview pipeline itself: the same plugin modules and the same
    // `renderPreview`, with the transpiler, extractor, Typst and Graphviz wasm
    // instead of the extension's subprocesses. See browser/README.md.
    function loadBrowserRenderer(){
        if(!browserRendererPromise)browserRendererPromise=import('./browser/preview.js');
        return browserRendererPromise;
    }
    async function previewRow(request,signal){
        if(await bridgeAvailable())return (await (await post('preview',request,signal)).json());
        return (await loadBrowserRenderer()).renderPreviewInBrowser(request);
    }
    // Writes back to .egg. The browser path keeps the bridge's guarantee that a
    // template must compile and that an edited program must still be recognized
    // by the patched runtime, it just runs both from wasm.
    async function editRequest(path,body){
        if(await bridgeAvailable())return (await (await post(path,body)).json());
        const browser=await loadBrowserRenderer();
        const result=path==='edit-display'?await browser.editDisplayInBrowser(body):await browser.editConditionsInBrowser(body);
        if(path==='edit-conditions')(await loadWasmDebugger()).debug_patterns(result.source);
        return result;
    }
    if(STATIC_PAGE)(async()=>{
        const local=await bridgeAvailable();
        status(local
            ? `静态部署：已连接本机 bridge（${BRIDGE_BASE}），预览/编辑与识别都可用。`
            : '静态部署：运行、识别与公式/DOT 预览都在浏览器内的 wasm 中运行；饱和程序的命中历史可能与本地 native 不同（要权威 event id 请跑 bridge）。写回 .egg 的编辑仍需要本机 bridge（python3 tools/egglog_debugger/server.py）。');
    })();
    // Common Typst math spellings; {field} placeholders are bound to the constructor's
    // fields in declaration order. Escaped braces {{ }} stay literal.
    const MATH_SYMBOLS = [
        ['字面名称（推荐）', 'upright("Mul")({left}, {right})'],
        ['字面名称 + 分组', 'upright("Mul") {{ {left} dot {right} }}'],
        ['字面名称（op）', 'op("Mul")({left}, {right})'],
        ['花括号分组', '{{ {left} + {right} }}'],
        ['加法', '{left} + {right}'], ['减法', '{left} - {right}'],
        ['乘法（点乘）', '{left} dot {right}'], ['叉乘', '{left} times {right}'],
        ['分式', 'frac({left}, {right})'], ['幂', '{base}^({exp})'],
        ['下标', '{value}_({index})'], ['平方根', 'sqrt({value})'],
        ['n 次根', 'root({n}, {value})'], ['绝对值', 'abs({value})'],
        ['向下取整', 'floor({value})'], ['向上取整', 'ceil({value})'],
        ['四舍五入', 'round({value})'],
        ['小于等于', '{left} <= {right}'], ['不等于', '{left} != {right}'],
        ['属于', '{left} in {right}'], ['等价', '{left} equiv {right}'],
        ['推导箭头', '{left} arrow.r.double {right}'],
        ['求和', 'sum_({i} = 0)^({n}) {f}'], ['求积', 'product_({i} = 0)^({n}) {f}'],
        ['积分', 'integral_({a})^({b}) {f} dif {x}'], ['极限', 'lim_({x} -> oo) {f}'],
        ['向量', 'arrow({v})'], ['矩阵', 'mat(1, 2; 3, 4)'],
        ['分段函数', 'cases({a} & "x > 0", {b} & "x <= 0")'],
        ['普通函数', 'upright("Name")({arg0})'],
        ['希腊字母', 'alpha beta gamma delta theta lambda mu pi sigma phi omega'],
    ];
    for(const [label,example] of MATH_SYMBOLS){
        const row=document.createElement('button');row.type='button';row.className='native-symbol-row';row.dataset.template=example;
        const code=document.createElement('code');code.textContent=example;
        const name=document.createElement('span');name.textContent=label;
        row.append(code,name);row.onclick=()=>insertTemplate(example);el('symbol-help').append(row);
    }
    for(const line of [
        '模板里的 {字段} 会替换为对应参数；{{ 和 }} 表示字面花括号（分组用）。',
        '多字母名称必须写成 upright("Mul") 或 op("Mul")。直接写 Mul，Typst 数学模式会读成 M·u·l 三个变量并报 unknown variable。',
        '保存前会检查重名、未声明字段和真实 Typst 编译结果。',
        '如果只是裸名称这类可确定的问题，保存时会先把修正结果填回模板框，再按一次“确认修正并保存”才会写入 .egg。',
    ]){const note=document.createElement('p');note.textContent=line;el('symbol-help').prepend(note);}
    function fingerprint(text){let hash=2166136261;for(let i=0;i<text.length;i++){hash^=text.charCodeAt(i);hash=Math.imul(hash,16777619);}return (hash>>>0).toString(16)+':'+text.length;}
    async function post(path, body, signal) {
        let response;
        try {
            response=await fetch(BRIDGE_BASE+'/api/'+path,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body),signal});
        } catch (error) {
            if(error.name==='AbortError')throw error;
            throw Error(STATIC_PAGE
                ? `未能访问本机 bridge（${BRIDGE_BASE}）：请用本仓库的 python3 tools/egglog_debugger/server.py 启动，它默认允许 https://milkblock.github.io 跨域。`
                : '无法连接本地调试服务。');
        }
        if (!response.ok) {
            let payload;try {payload=await response.json();}catch {payload={error:'请通过 tools/egglog_debugger/server.py 启动本地调试服务'};}
            const error=Error(payload.error || '请求失败');error.payload=payload;throw error;
        }
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
    function closeNameEditor(){currentEdit=null;el('name-editor').hidden=true;el('condition-editor').hidden=true;hideSymbolHelp();}
    function hideSymbolHelp(){el('symbol-help').hidden=true;el('template-help').setAttribute('aria-expanded','false');}
    function clearPendingFix(){el('name-save').textContent='保存到 .egg 注释';el('template-note').classList.remove('native-note-pending');}
    function insertTemplate(example){const area=el('template-input');if(!example)return;const start=area.selectionStart ?? area.value.length,end=area.selectionEnd ?? start;area.value=area.value.slice(0,start)+example+area.value.slice(end);templateDirty=true;clearPendingFix();area.focus();area.selectionStart=area.selectionEnd=start+example.length;}
    function renamePlaceholder(template,from,to){if(!from||!to||from===to)return template;const escaped=from.replace(/[.*+?^${}()|[\]\\]/g,'\\$&');return template.replace(new RegExp('\\{'+escaped+'\\}','g'),'{'+to+'}');}
    function currentFields(){return [...el('field-editor').querySelectorAll('.native-field-name')].map(input=>input.value.trim());}
    // A value with {field} placeholders is a formula, not a label: typing the whole
    // template into the name field must not produce upright("{left} + {right}").
    function looksLikeTemplate(value){return /[{}]/.test(value);}
    function templateFromName(){const name=el('name-input').value.trim();const fields=currentFields();const valid=fields.length?fields.map((field,index)=>field||`arg${index}`):[];if(looksLikeTemplate(name))return name;return 'upright('+JSON.stringify(name)+')'+'('+valid.map(field=>'{'+field+'}').join(', ')+')';}
    // A clickable target list makes editing independent of glyph hit testing: a
    // template without a literal name, or a plugin text fallback, must not lock
    // the user out of a second edit.
    function renderTargets(targets, editable, note){
        const bar=el('targets');bar.replaceChildren();
        if(!editable || !targets.length)return;
        const caption=document.createElement('span');caption.id='native-targets-caption';caption.textContent='可编辑目标：';bar.append(caption);
        for(const target of targets){
            const button=document.createElement('button');button.type='button';button.dataset.target=target.id;
            button.textContent=target.kind==='constructor'?`构造器 ${target.name}`:target.kind==='conditions'?'规则条件':`变量 ${target.name}${target.rendered && target.rendered!==target.name?' · '+target.rendered:''}`;
            button.onclick=()=>openEditor(target,target.rendered || target.display || target.name);
            bar.append(button);
        }
        if(note){const hint=document.createElement('span');hint.id='native-targets-note';hint.textContent=note;bar.append(hint);}
    }
    function openEditor(target, regionText){
        const request=activeRequest;
        if(!request || editor.getValue()!==request.source){status('源码已变化，请等待新公式渲染后再编辑。');return;}
        closeNameEditor();
        currentEdit={source:request.source,line:request.line,target_id:target.id};
        if(target.kind==='conditions'){
            el('condition-input').value=target.condition_source || '';
            el('condition-error').textContent='';el('condition-editor').hidden=false;
            el('condition-input').focus();return;
        }
        // A field binding renders as `<node>.<field>`; editing it names the node, so seed
        // the input with the variable's own name instead of the whole accessor text.
        const seed=target.kind==='binding'?(target.display||target.name):regionText;
        el('edit-label').textContent=`显示名称 · ${seed}`;
        el('name-input').value=seed;el('edit-error').textContent='';
        el('edit-scope').textContent=target.kind==='constructor'?`更新 ${target.name} 的 dsl_type 显示模板，作用于本文件中的该构造器。`:`更新本条规则中 ${target.name} 的 labels.bindings 显示名称。`;
        el('field-editor').replaceChildren();
        templateDirty=false;
        if(target.kind==='constructor' && target.field_labels?.length){
            const title=document.createElement('div');title.textContent='字段名称（用于模板占位符）';el('field-editor').append(title);
            const placeholders=(target.template||'').match(/\{([A-Za-z_][A-Za-z0-9_]*)\}/g)?.map(token=>token.slice(1,-1)) || [];
            target.field_labels.forEach((field,index)=>{const label=document.createElement('label');label.className='native-field-row';label.textContent=`字段 ${index+1}`;const input=document.createElement('input');input.className='native-field-name';input.value=field;input.maxLength=80;input.required=true;input.dataset.index=index;input.dataset.placeholder=placeholders[index] || field;input.addEventListener('input',()=>{const next=input.value.trim();if(next && next!==input.dataset.placeholder && /^[A-Za-z_][A-Za-z0-9_]*$/.test(next)){el('template-input').value=renamePlaceholder(el('template-input').value,input.dataset.placeholder,next);input.dataset.placeholder=next;templateDirty=true;clearPendingFix();}});label.append(input);el('field-editor').append(label);});
        }
        const constructor=target.kind==='constructor';
        el('template-editor').hidden=!constructor;
        el('name-save').textContent='保存到 .egg 注释';
        el('template-note').classList.remove('native-note-pending');
        if(constructor){
            // Recover annotations written before template-like names were routed:
            // a label of "{left} + {right}" wrapped by upright("...") was meant as
            // the template itself, so show the intended formula on reopen.
            let template=target.template || '';
            const label=target.display || regionText;
            const wrapped='upright('+JSON.stringify(label)+')';
            if(looksLikeTemplate(label) && template.startsWith(wrapped)){
                template=label;
                el('template-note').textContent='检测到保存时被包成 upright("...") 的模板，已还原为公式；保存即可修正。';
            }else el('template-note').textContent='';
            el('template-input').value=template;
            el('precedence-input').value=target.precedence ?? 90;
            hideSymbolHelp();
        }
        el('name-editor').hidden=false;el('name-input').focus();el('name-input').select();
    }
    function installEditTargets(svg, rendered, request, row){
        el('edit-hint').textContent=row.kind?'日志是只读快照；请载入源码并点击编辑器中的规则进行编辑。':'点击公式中带高亮的名称修改显示文本；点击 if 条件区编辑真实匹配条件。';
        if(row.kind){renderTargets([], false);return;}
        renderTargets(rendered.edit_targets || [], true,
            el('format').value==='typst' && rendered.typst_mode!=='math' ? '当前公式由插件渲染为文本，仍可用上面的按钮打开编辑器。' : '');
        if(el('format').value!=='typst')return;
        const targets=new Map((rendered.edit_targets || []).map(target=>[target.id,target]));
        for(const region of rendered.edit_regions || []){
            const target=targets.get(region.target_id);if(!target)continue;
            const rect=document.createElementNS('http://www.w3.org/2000/svg','rect');
            rect.classList.add('native-edit-hit');rect.dataset.target=target.id;
            rect.setAttribute('x',Math.max(0,region.x-1));rect.setAttribute('y',Math.max(0,region.y-1));
            rect.setAttribute('width',region.width+2);rect.setAttribute('height',region.height+2);
            rect.setAttribute('rx','1');rect.setAttribute('tabindex','0');rect.setAttribute('role','button');
            rect.setAttribute('aria-label',`编辑 ${region.text}`);
            const open=()=>openEditor(target,region.text);
            rect.addEventListener('click',open);rect.addEventListener('keydown',event=>{if(event.key==='Enter' || event.key===' '){event.preventDefault();open();}});
            svg.append(rect);
        }
        if(rendered.edit_error)el('edit-hint').textContent='当前公式的文字定位失败；原始插件预览仍可查看，可用下方按钮编辑。';
    }
    el('name-input').addEventListener('input',()=>{if(currentEdit && !el('template-editor').hidden){const name=el('name-input').value.trim();el('template-input').value=templateFromName();templateDirty=true;clearPendingFix();el('template-note').textContent=looksLikeTemplate(name)?'检测到 {字段}，已按 Typst 模板处理；显示名称只是标签文字。':'';}});
    el('template-input').addEventListener('input',()=>{templateDirty=true;clearPendingFix();});
    el('template-help').onclick=event=>{event.stopPropagation();const open=el('symbol-help').hidden;el('symbol-help').hidden=!open;el('template-help').setAttribute('aria-expanded',String(open));};
    el('symbol-help').addEventListener('click',event=>event.stopPropagation());
    document.addEventListener('click',()=>{if(!el('symbol-help').hidden)hideSymbolHelp();});
    el('name-cancel').onclick=closeNameEditor;
    el('name-editor').addEventListener('keydown',event=>{if(event.key==='Escape'){event.preventDefault();closeNameEditor();}});
    el('name-input').onkeydown=event=>{if(event.key==='Escape'){event.preventDefault();closeNameEditor();}};
    el('name-editor').onsubmit=async event=>{
        event.preventDefault();const edit=currentEdit;if(!edit)return;
        if(editor.getValue()!==edit.source){el('edit-error').textContent='源码已变化，请取消并重新点击公式；未覆盖你的修改。';return;}
        el('name-save').disabled=true;el('edit-error').textContent='';
        try{
            const fields=currentFields();
            const body={...edit,value:el('name-input').value};
            if(fields.length)body.fields=fields;
            if(!el('template-editor').hidden){
                body.template=el('template-input').value;
                const raw=el('precedence-input').value.trim();
                const precedence=raw===''?NaN:Number(raw);
                if(Number.isInteger(precedence))body.precedence=precedence;
            }
            const result=await editRequest('edit-display',body);
            if(currentEdit!==edit)return;
            if(editor.getValue()!==edit.source)throw Error('源码已变化，未写入旧版本的修改。');
            editor.operation(()=>{
                editor.replaceRange(result.source,{line:0,ch:0},editor.posFromIndex(edit.source.length),'+display-annotation');
                editor.setCursor({line:result.line-1,ch:0});
            });
            clearPendingFix();closeNameEditor();clearTimeout(timer);status('显示注释已写回 .egg；可用编辑器撤销，也可下载源码。');
            await previewLine();
        }catch(error){
            const payload=error.payload;
            if(payload?.suggestion && payload.suggestion!==el('template-input').value){
                // Never write on the first failure: show the corrected template and
                // wait for the user to confirm it with a second submit.
                el('template-input').value=payload.suggestion;
                el('edit-error').textContent='';
                el('template-note').textContent=`已自动修正${payload.notes?.length?'：'+payload.notes.join('、'):''}。尚未写入 .egg，请再按一次“确认修正并保存”。`;
                el('template-note').classList.add('native-note-pending');
                el('name-save').textContent='确认修正并保存';
                status('模板已自动修正，等待你确认后写入。');
            }else el('edit-error').textContent=error.message;
        }
        finally{el('name-save').disabled=false;}
    };
    el('condition-cancel').onclick=closeNameEditor;
    el('condition-editor').addEventListener('keydown',event=>{if(event.key==='Escape'){event.preventDefault();closeNameEditor();}});
    el('condition-editor').onsubmit=async event=>{
        event.preventDefault();const edit=currentEdit;if(!edit)return;
        if(editor.getValue()!==edit.source){el('condition-error').textContent='源码已变化，请重新点击条件区；未覆盖你的修改。';return;}
        el('condition-save').disabled=true;el('condition-error').textContent='';
        try{
            const result=await editRequest('edit-conditions',{source:edit.source,line:edit.line,conditions:el('condition-input').value});
            if(currentEdit!==edit)return;
            if(editor.getValue()!==edit.source)throw Error('源码已变化，未覆盖你的修改。');
            editor.operation(()=>{
                editor.replaceRange(result.source,{line:0,ch:0},editor.posFromIndex(edit.source.length),'+condition-edit');
                editor.setCursor({line:result.line-1,ch:0});
            });
            closeNameEditor();clearTimeout(timer);status('规则附加条件已写回 .egg（匹配行为已更新），可撤销。');
            await previewLine();
        }catch(error){el('condition-error').textContent=error.message;}
        finally{el('condition-save').disabled=false;}
    };
    el('download').onclick=()=>{
        const url=URL.createObjectURL(new Blob([editor.getValue()],{type:'text/plain;charset=utf-8'}));
        const link=document.createElement('a');link.href=url;link.download='egglog-preview.egg';link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);
    };
    // The fractal lane is one repetition statement, not a sequence of distinct
    // rules: listing every event would repeat the same rule preview up to 168
    // times. The per-step bindings stay in the evidence panel.
    function fractalDepth(row) {
        const events=row.evidence?.events;
        return Array.isArray(events)&&events.length?events.length:(row.steps || []).length;
    }
    function fractalRequest(row) {
        const evidence=row.evidence;if(!evidence)return null;
        const depth=fractalDepth(row);if(!depth)return null;
        // The lane's own chain is its trigger plus the applications it reports. The
        // composition behind `step_details` also walks unrelated branches, so pick
        // the records by event id instead of using it as the sequence.
        const byEvent=new Map((row.step_details || []).map(step => [step.event, step.binding || []]));
        const chain=[evidence.trigger, ...(evidence.events || [])];
        const bindings=chain.map(event => byEvent.get(event)).filter(step => step && step.length);
        return {depth,operator:evidence.operator ?? null,context:evidence.trigger ?? null,update:evidence.update || [],witness:evidence.higher ?? null,bindings};
    }
    function selectSteps(row) {
        const steps=el('step');steps.replaceChildren();
        if(row.kind==='fractal' && row.evidence)steps.add(new Option(`紧凑（×${fractalDepth(row)}）`,'fractal'));
        else if(row.composition_mode==='flattened rule')steps.add(new Option('组合后的规则','combined'));
        if(row.kind!=='fractal' || !row.evidence)for(const step of row.step_details || [])steps.add(new Option(`event ${step.event} · ${step.rule} · L${step.source_line}`,String(step.event)));
        steps.hidden=!steps.options.length;
        if(steps.options.length)steps.value=steps.options[0].value;
    }
    function previewRequest(row) {
        let source=row.kind?snapshotSource:row.preview_source;
        let line=row.source_line;
        const step=(row.step_details || []).find(step=>String(step.event)===el('step').value);
        if(row.composition_mode==='flattened rule' && el('step').value==='combined'){
            line=source.split('\n').length+1;
            source+='\n'+row.source;
        }else if(step){line=step.source_line;}
        const fractal=row.kind==='fractal'?fractalRequest(row):null;
        return {source,line,mode:el('dot-mode').value,label_style:el('label-style').value,recursive_strategy:el('recursive').value,...(fractal?{fractal}:{})};
    }
    async function show(row, fromTrace=false) {
        const changed=selected!==row;selected=row; const version=++renderRevision;
        // A debounced re-render of the same rule must not close an editor the user
        // has open (it would also discard the in-flight save response).
        if(changed || fromTrace)closeNameEditor();
        if(changed || fromTrace)selectSteps(row);
        previewAbort?.abort(); previewAbort=new AbortController();
        const kind=el('format').value;
        el('title').textContent=`${row.kind || 'pattern'} · ${row.rule || ''} · L${row.source_line || '?'}`
            +(row.kind==='fractal'&&row.evidence?` · ×${fractalDepth(row)}`:'');
        el('source').textContent='';el('rust').textContent='';
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
            const request=previewRequest(row);activeRequest=request;
            if(!request.source || !request.line)throw Error('此旧日志缺少源位置，不能可靠地交给插件渲染；请重新运行生成日志。');
            // The same line number means different formulas after a source edit, so the
            // fingerprint keeps a cached preview from being shown for another rule.
            const key=JSON.stringify([el('step').value,request.line,request.mode,request.label_style,request.recursive_strategy,fingerprint(request.source)]);
            row.plugin_previews ||= {};
            const rendered=row.plugin_previews[key] || await previewRow(request,previewAbort.signal);
            if(version!==renderRevision)return;
            row.plugin_previews[key]=rendered;
            el('source').textContent=rendered[kind];
            el('rust').textContent=rendered.rust_source?`// 转译后 Rust（第 ${rendered.rust_start_line} 行起，交给 extractor 的规则作用域）\n${rendered.rust_source}`:'没有对应的 Rust 源码。';
            const svg=mountSvg(kind==='typst'?rendered.typst_svg:rendered.dot_svg);
            if(kind==='dot')applyTypstRenderings(svg,rendered.typst_renderings);
            installEditTargets(svg,rendered,request,row);
            el('renderer').textContent=`${rendered.renderer} · ${rendered.renderer_revision.slice(0,12)} · ${rendered.config.mode} / ${rendered.config.label_style} / ${rendered.config.recursive_strategy}`;
            if(kind==='typst' && rendered.typst_mode!=='math')el('render-error').textContent='插件使用了文本 fallback；请检查 Typst 模板。';
            el('preview').dataset.ready='true';renderedCount++;
        }catch(error){if(version===renderRevision && error.name!=='AbortError'){el('render-error').textContent=error.message;el('renderer').textContent='插件渲染失败';renderTargets([],false);}}
    }
    async function previewLine() {
        const source=editor.getValue(), version=revision, intent=++selectionIntent;
        try {
            if(parsedSource!==source){
                parseAbort?.abort();parseAbort=new AbortController();
                const data=await listPatterns(source,parseAbort.signal);
                if(version!==revision || intent!==selectionIntent)return;
                patterns=data.patterns;parsedSource=source;
            }
            const line=editor.getCursor().line+1;
            const row=patterns.find(p=>p.source_line<=line && line<=p.end_line);
            if(row){row.preview_source=source;await show(row);}
            else {selected=null;++renderRevision;previewAbort?.abort();marker?.clear();el('title').textContent=`L${line} 没有 rule/rewrite`;el('preview').hidden=true;el('source').textContent='';el('rust').textContent='';el('details').textContent='';el('render-error').textContent='';renderTargets([],false);}
        }catch(error){if(version===revision && intent===selectionIntent && error.name!=='AbortError'){selected=null;el('render-error').textContent=error.message;el('preview').hidden=true;renderTargets([],false);}}
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
            let complete=false;
            const consume=line=>{if(!line.trim())return;const row=JSON.parse(line);
                if(row.kind==='error')throw Error(row.error);
                if(row.kind==='complete'){complete=true;runStatus='complete';status(`完成：${rows.filter(r=>r.kind==='application').length} 个有效应用，${rows.filter(r=>r.kind==='compose').length} 个组合，${rows.filter(r=>r.kind==='fractal').length} 个 Fractal 证据。`);}
                else if(row.kind==='boundary')status(`执行边界 ${row.boundary}：${row.applications} 个有效应用；${row.logical_matches} 个逻辑匹配，排除 ${row.excluded} 个。`);
                else {rows.push(row);addRow(row);}
            };
            if(await bridgeAvailable()){
                const response=await post('trace',{source:snapshotSource},current.signal);
                const reader=response.body.getReader(),decoder=new TextDecoder();let buffer='';
                while(true){const {value,done}=await reader.read();buffer+=decoder.decode(value || new Uint8Array(),{stream:!done});let newline;while((newline=buffer.indexOf('\n'))>=0){const line=buffer.slice(0,newline);buffer=buffer.slice(newline+1);consume(line);}if(done)break;}
                if(buffer.trim())consume(buffer);
                if(!complete)throw Error('事件流提前结束，当前日志为部分结果');
            }else{
                // Same patched runtime, compiled to wasm: no local process involved.
                const wasm=await loadWasmDebugger();
                status('运行浏览器内的 wasm 插桩 runtime…');
                wasm.debug_stream(snapshotSource,line=>consume(line));
                if(!complete)throw Error('wasm 事件流没有给出完成事件');
            }
        }catch(error){runStatus=error.name==='AbortError'?'cancelled':'failed';status(error.name==='AbortError'?'已停止，保留已收到的日志。':`运行失败：${error.message}`);}
        finally{if(controller===current){el('run').disabled=false;el('stop').disabled=true;el('import').disabled=false;controller=null;}}
    };
    el('restore').onclick=()=>{if(snapshotSource){editor.setValue(snapshotSource);status('已载入日志对应源码；点击日志重现公式。');}};
    el('stop').onclick=()=>controller?.abort();
    el('export').onclick=()=>{const url=URL.createObjectURL(new Blob([JSON.stringify({version:2,status:runStatus,source:snapshotSource,rows},null,2)],{type:'application/json'}));const a=document.createElement('a');a.href=url;a.download='egglog-debug-history.json';a.click();setTimeout(()=>URL.revokeObjectURL(url),1000);};
    el('import').onchange=async event=>{try{const file=event.target.files[0];if(!file)return;const data=JSON.parse(await file.text());if(![1,2].includes(data.version) || typeof data.source!=='string' || !Array.isArray(data.rows) || data.rows.some(r=>!['application','compose','fractal'].includes(r.kind) || typeof r.id!=='string'))throw Error('无效的日志格式');controller?.abort();snapshotSource=data.source;runStatus=data.status || 'unknown';rows=data.rows;selected=null;el('preview').hidden=true;el('filter').onchange();status(`已载入 ${rows.length} 条公式快照（${runStatus}）；点击日志回放。`);}catch(error){status(error.message);}event.target.value='';};
    // Expose read-only state for debugging and browser regression tests.
    window.egglogNative={get rows(){return structuredClone(rows);},get selected(){return structuredClone(selected);},get rendered(){return renderedCount;}};
}
