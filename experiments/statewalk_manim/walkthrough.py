"""A self-contained, offline reader for the generated Python execution trace."""
import json
import re
from syntax import python_lines, html_code, css


def export(out):
    flow=json.loads((out/'dataflow.json').read_text())
    flow['syntax']={mode:python_lines('\n'.join(line.split('  #')[0] for line in flow[key].rstrip().splitlines()))
                    for mode,key in [('plain','plain_python'),('tokens','python')]}
    payload=json.dumps(flow,ensure_ascii=False).replace('<','\\u003c')
    page=r'''<!doctype html><html lang="zh-CN"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>从 Statewalk 到可运行的 Python</title><style>
:root{color-scheme:dark;background:#101014;color:#eee;font-family:"PingFang SC",system-ui,sans-serif}*{box-sizing:border-box}body{margin:0}main{max-width:1280px;margin:auto;padding:32px 28px 60px}h1{font-size:34px;font-weight:500}h2{font-size:22px;font-weight:500}p{color:#bcbccb;line-height:1.85}a{color:#83c167;text-underline-offset:4px}.eyebrow{color:#58c4dd;letter-spacing:.12em}.panels{display:grid;grid-template-columns:1.1fr 1fr;gap:22px}.panel{background:#19191f;border:1px solid #343440;border-radius:12px;padding:20px;min-width:0}.code{font:14px/1.9 Menlo,monospace;white-space:pre;overflow:auto}.line{display:block;padding:0 10px}.active{background:#3b331a;box-shadow:inset 3px 0 #ffff00;border-radius:4px}.number{display:inline-block;color:#787888;width:27px;user-select:none}button{background:#20202b;color:#eee;border:1px solid #4b4b61;border-radius:6px;padding:10px 16px;cursor:pointer;font:inherit}button:disabled{opacity:.4;cursor:default}select{background:#20202b;color:#eee;border:1px solid #4b4b61;border-radius:6px;padding:8px;font:inherit}.controls{display:flex;gap:12px;align-items:center;flex-wrap:wrap;margin:18px 0}table{width:100%;border-collapse:collapse;font:15px/1.6 Menlo,monospace}td{padding:7px 12px;border-bottom:1px solid #32323d}.changed{color:#83c167}pre{white-space:pre-wrap;color:#58c4dd;font:14px/1.8 Menlo,monospace}.note{color:#aaaab9;font-size:14px}.phases{display:grid;grid-template-columns:repeat(3,1fr);gap:12px;margin:30px 0}.phases article{border-top:2px solid #58c4dd;padding:8px 14px;background:#19191f}.phases h2{font-size:19px}.phases p{font-size:14px}details{border-top:1px solid #343440;padding:20px 0}summary{cursor:pointer;font-size:20px}kbd{border:1px solid #555;border-radius:3px;padding:1px 5px}@media(max-width:850px){main{padding:20px 14px}.panels,.phases{grid-template-columns:1fr}h1{font-size:27px}}
</style><main><div class="eyebrow">真实 eggcc IR → 自动生成 → 实际执行记录</div><h1>提取结束以后，程序究竟长什么样？</h1>
<p>左边是从 raytrace 的 <code>vec_add</code> 提取结果自动生成的 Python。右边显示该 Python 真正执行后记录的变量；这里的前进按钮是在回放已验证的执行记录。<a href="index.html">返回讲解视频</a></p>
<div class="controls"><button id="prev">← 上一步</button><button id="next">下一步 →</button><button id="reset">回到开始</button><span id="progress"></span><select id="mode" aria-label="代码形式"><option value="plain">简化数据流</option><option value="tokens">显式状态依赖</option></select></div>
<div class="panels"><section class="panel"><h2>自动生成的 Python</h2><div id="code" class="code"></div><p class="note">紫色：关键字；蓝色：函数；橙色：数字。黄色边线表示当前执行行。切换右上菜单可显示状态传递。<kbd>←</kbd> / <kbd>→</kbd> 也可以逐步查看。</p></section>
<section class="panel"><h2 id="action">尚未执行</h2><p>a = [2, 4, 6]　b = [10, 20, 30]</p><table id="values"></table><h2>对应的原始 IR 节点</h2><pre id="ir"></pre><p id="explain" class="note"></p></section></div>
<div class="phases"><article><h2>① 搜索：有哪些合法来源？</h2><p>已知一次 Load 合法，就传播 Get 和 FAdd 的“可提取”状态。这里没有读取示例数组，也没有真的求出 24。已有节点的依赖变得满足，不是新增 rewrite 节点。</p></article><article><h2>② 提取：选哪些节点，怎么连？</h2><p>回溯状态路径，展开重复的副作用位置，去掉不可提取节点，再补上被选中的纯计算依赖。已经输出的共享结果继续引用。原生包含成本估计、活跃性和启发式，不保证全局最优。</p></article><article><h2>③ 执行：这时才得到数值</h2><p>现在执行生成的 Python，先读 b₀，再读 a₁、b₁……最后实际相加并写入结果。r₁ 很早就“可提取”，最终代码却在第二次 Write 前才计算它。</p></article></div>
<details open><summary>完整的验证口径</summary><p>Python 独立复核了原生 1,864 次候选依赖检查、1,465 次纯闭包传播和 807 次状态登记的成本加法。生成的 Python 与直接解释选中 IR，在 131 组输入上核对了浮点结果和副作用顺序。它没有重新实现原生优先队列、活跃性归并或最终贪心选择。</p><p>简化版本删掉类型节点，内联 Get 与 PtrAdd。显式版本用递增整数标记执行顺序，它不是 eggcc 的原始 state 表示。这个固定示例不模拟一般指针别名、完整分配语义或任意程序。</p></details>
<details><summary>纯计算传播的可运行简化算法</summary><pre>def pure_closure(nodes, known):
    known = set(known)
    while True:
        ready = {
            n.eclass for n in nodes
            if not n.effectful
            and n.eclass not in known
            and all(c in known for c in n.children)
        }
        if not ready:
            return known
        known.update(ready)</pre><p>同一个节点的孩子必须全部满足；同一个 e-class 的多个节点只要一个可用即可。展示版反复扫描，便于阅读；Tiger 使用反向索引、依赖计数和持久化集合增量维护。</p></details>
<p><a href="vec_add_plain.py" download>下载简化 Python</a> · <a href="vec_add_python.py" download>下载显式状态版</a> · <a href="dataflow_runtime.py" download>状态版所需的小运行器</a> · <a href="vec_add-extracted.egg" download>原始选中 IR</a> · <a href="dataflow.json" download>执行记录与验证信息</a></p>
<p class="note">简化版可直接运行；显式版与小运行器放在同一目录。131 组核对是此翻译的测试，不是对任意输入或任意程序的形式化证明。</p></main><script>
const data=__DATA__, $=s=>document.querySelector(s);let cursor=-1;
const explanations={load:'从实际输入数组读取一个浮点数，同时向前传递状态。',alloc:'分配唯一的一份输出数组；后面的三次写入共享它。',add:'现在才实际计算浮点加法。搜索阶段只判断它是否可提取。',write:'使用已经算好的值更新共享输出数组，并传递状态。',return:'返回同一个输出数组；显式版也返回最终状态。'};
const labels={load:'读取输入',alloc:'分配结果',add:'执行加法',write:'写入结果',return:'返回结果'};
function format(v){return Array.isArray(v)?'['+v.map(x=>x===null?'·':x).join(', ')+']':String(v)}
function render(){const tokens=$('#mode').value==='tokens',step=data.steps[cursor];const lines=(tokens?data.python:data.plain_python).trimEnd().split('\n');const selected=step?(tokens?step.python_line-1:cursor+1):-1;$('#code').replaceChildren();lines.forEach((line,i)=>{const row=document.createElement('span');row.className='line'+(i===selected?' active':'');const num=document.createElement('span');num.className='number';num.textContent=i+1;row.append(num);data.syntax[tokens?'tokens':'plain'][i].forEach(piece=>{const span=document.createElement('span');span.className='syntax-'+piece.kind;span.textContent=piece.text;row.append(span)});$('#code').append(row)});$('#progress').textContent=cursor<0?'尚未开始':`${cursor+1} / ${data.steps.length}`;$('#prev').disabled=cursor<0;$('#next').disabled=cursor===data.steps.length-1;$('#action').textContent=step?labels[step.kind]:'尚未执行';$('#ir').textContent=step?step.node+'\n'+step.ir:'点击下一步，从第一条 Load 开始。';$('#explain').textContent=step?explanations[step.kind]:'';$('#values').replaceChildren();const previous=cursor>0?data.steps[cursor-1].environment:{};if(step){Object.entries(step.environment).filter(([k])=>tokens||!/^s\d+$/.test(k)).forEach(([k,v])=>{const tr=document.createElement('tr'),a=document.createElement('td'),b=document.createElement('td');a.textContent=k;b.textContent=format(v);if(JSON.stringify(v)!==JSON.stringify(previous[k]))tr.className='changed';tr.append(a,b);$('#values').append(tr)})}}
$('#next').onclick=()=>{cursor=Math.min(cursor+1,data.steps.length-1);render()};$('#prev').onclick=()=>{cursor=Math.max(-1,cursor-1);render()};$('#reset').onclick=()=>{cursor=-1;render()};$('#mode').onchange=render;document.addEventListener('keydown',e=>{if(e.target.tagName==='SELECT')return;if(e.key==='ArrowRight')$('#next').click();if(e.key==='ArrowLeft')$('#prev').click()});render();
</script></html>'''
    # The algorithm excerpt uses the same lexer and colors as the live listing.
    page=re.sub(r'<pre>(def pure_closure[\s\S]*?)</pre>',lambda m:'<pre>'+html_code(m.group(1))+'</pre>',page)
    page=page.replace('</style>',css()+'</style>')
    (out/'walkthrough.html').write_text(page.replace('__DATA__',payload))
