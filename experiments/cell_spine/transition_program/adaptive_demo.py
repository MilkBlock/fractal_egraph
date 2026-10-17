"""Event-driven local Cell runtime with alias repair, interning and input union."""
import argparse
import hashlib
import html
import json
from pathlib import Path
import subprocess
import sys

ROOT=Path(__file__).resolve().parents[3]
sys.path.insert(0,str(ROOT))
from experiments.cell_spine.transition_program.runtime import Runtime,closure_key
from experiments.cell_spine.transition_program.demo import input_term,HERE
from experiments.cell_spine.factorized_boundary.demo import native,sexpr
from tools.bench.math_graph_equal import verify


def fixture(rt,offset=0):
    def seed(term,values):return rt.seed(term,[v+offset for v in values])
    pair=('Add',('Input',0),('Input',1));mul=('Mul',pair,('Input',2));power=('Pow',pair,('Input',2))
    a=seed(mul,[0,1,2]);b=seed(mul,[0,3,2])
    swapped=seed(mul,[1,0,2])
    assert rt.resolve(a)==rt.resolve(swapped), 'whole grounded closure must absorb this symmetry'
    first=rt.construct('Add',a,b,'alias-target')
    duplicate=rt.construct('Add',swapped,b,'duplicate-boundary')
    assert duplicate==first
    chain=first
    for value in (4,5,6,7):
        other=seed(mul,[0,value,2])
        chain=rt.construct('Add',chain,other,f'factor-{value}')
    independent=rt.construct('Add',seed(mul,[10,11,12]),seed(mul,[10,13,12]),'independent-alias')
    no_match=rt.construct('Add',a,seed(mul,[30,31,32]),'no-shared-factor')
    angles=seed(pair,[20,21]);cos=seed(('Cos',pair),[20,21]);loop=cos;cycle=[]
    for step in range(6):
        loop=rt.construct('Integral',loop,angles,f'integral-{step}') if step%2==0 else rt.construct('Diff',angles,loop,f'diff-{step}')
        cycle.append(loop)
    powers=rt.construct('Mul',seed(power,[40,41,42]),seed(power,[41,40,43]),'powers')
    powers=rt.construct('Mul',powers,seed(power,[40,41,44]),'powers-again')
    return dict(first=first,factor=chain,independent=independent,no_match=no_match,
                cycle=cycle,powers=powers,source=a)


def original(rt,ref):
    if ref[0]=='cell':
        term,values=rt.origins[ref]
        return input_term(term,values)
    n=rt.nodes[ref[1]]
    return (n['op'],original(rt,n['left']),original(rt,n['right']))


def audit(rt,out,binary,source,nodes=None):
    checks=[]
    unions='\n'.join(f'(union (Var "v{a}") (Var "v{b}"))' for a,b in rt.input_unions)
    constructors=['Var']+[c for c in rt.library.constructors if c!='RipenInput']
    for n in range(len(rt.nodes)) if nodes is None else nodes:
        h=rt.resolve(('node',n))
        if h is None:raise AssertionError('unresolved constructor fact')
        row=rt.cells.get(h);t=rt.library.templates[row['template']]
        expected=original(rt,('node',n));encoded=input_term(t['seed'],row['binding'])
        for name,term in [('original',expected),('cell',encoded)]:
            native(binary,out/f'{n}/{name}',source,f'(let root {sexpr(term)})\n'+unions,'',constructors)
        equality=verify(out/f'{n}/original/graph.json',out/f'{n}/cell/graph.json')
        checks.append(dict(node=n,label=rt.nodes[n]['label'],**equality))
    return checks


def exercise(out,binary,source,ast,backend,gpu_binary=None):
    rt=Runtime(out,binary,source,ast,backend,gpu_binary)
    roots=fixture(rt);rt.drain()
    first_summary=rt.summary();before=audit(rt,out/'audit-before',binary,source)
    first_batches=len(rt.batches)
    independent=rt.resolve(roots['independent'])
    # No artificial two-round limit: six constructor-dependent steps complete.
    assert len(rt.batches)>2 and len(rt.outputs)==len(rt.nodes)
    assert rt.stats['alias_targets_resumed']>0
    assert rt.library.stats['alias_cache_hits']>0
    cycle=[rt.resolve(r) for r in roots['cycle']]
    assert cycle[1]==cycle[3]==cycle[5]
    assert cycle[2]==cycle[4]
    rt.union_inputs(1,3);rt.drain()
    after=audit(rt,out/'audit-after',binary,source)
    touched={n for b in rt.batches[first_batches:] for n in b['ready_nodes']}
    assert roots['independent'][1] not in touched
    assert rt.resolve(roots['independent'])==independent
    # New facts can arrive after the worklist has drained.
    continuation=rt.construct('Add',roots['factor'],rt.seed(('Mul',('Add',('Input',0),('Input',1)),('Input',2)),[0,8,2]),'late-factor')
    rt.drain();assert rt.resolve(continuation) is not None
    late=audit(rt,out/'audit-late',binary,source)
    count=len(rt.batches);bakes=rt.library.stats['native_bakes'];rt.drain()
    assert len(rt.batches)==count and rt.library.stats['native_bakes']==bakes
    final_keys=[rt.cells.get(rt.resolve(('node',n)))['key'] for n in range(len(rt.nodes))]
    return dict(before=first_summary,after=rt.summary(),checks=dict(before=before,after=after,late=late),
                union_touched_nodes=sorted(touched),independent_untouched=True,
                cycle_cells=cycle,quiescent_repeat_no_work=True),final_keys


def render(out,r):
    primary=r['gpu'] or r['cpu'];summary=primary['after']
    body='''<!doctype html><html lang="zh"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>自适应 Cell 执行</title>
<style>body{font:16px system-ui;line-height:1.8;background:#f4f7fa;color:#163047;max-width:1120px;margin:32px auto;padding:0 20px}section{background:white;padding:24px;border:1px solid #dbe4ed;border-radius:12px;margin:18px 0}pre{background:#eef3f7;padding:16px;overflow:auto}table{border-collapse:collapse;width:100%}td,th{border:1px solid #dbe4ed;padding:9px}.note{border-left:4px solid #bf761c;padding-left:16px}</style>
<h1>由事实驱动，自动修复别名并继续执行</h1><section><pre>真实的外部 constructor 事实
    ↓ 孩子就绪，按根 operator 路由规则
GPU 匹配与目标编码
    ├─ 正常 → 完整闭包判重 → 发布
    └─ 输入别名 → CPU 按相等关系 Bake／复用 → 发布
                                                   ↓
                                      唤醒依赖它的 constructor
</pre><p>不再手写两阶段，不枚举任意两个 Cell，也不在没有外部事实的地方制造连接。冷态专化和目录管理在 CPU；这条自适应路径允许 CPU/GPU 分批接续。</p></section>'''
    metrics=[('已解析外部事实',summary['resolved_facts']),('库模板',summary['templates']),
             ('当前活跃 Cell',summary['live_cells']),('自动恢复的别名目标',summary['runtime'].get('alias_targets_resumed',0)),
             ('别名专化缓存命中',summary['library'].get('alias_cache_hits',0)),
             ('原生上下文 fallback',summary['runtime'].get('native_context_fallbacks',0))]
    body+='<section><table>'+''.join('<tr><th>'+html.escape(k)+'</th><td>'+str(v)+'</td></tr>' for k,v in metrics)+'</table></section>'
    body+='<section><h2>验证</h2><p>六步积分／求导链自动跑完，闭包相同的后继复用已有 Cell。输入 union 后，只处理受影响的参数使用者及相关边界，独立链保持不动。队列清空后可以追加新事实并继续运行。</p><p>变化前、输入 union 后、追加事实后，每个节点都将原始表达式交给原生 egglog，与编码 Cell 的完整 e-node／e-class 状态比较。</p>'
    if r['gpu']:body+='<p>真实 Metal/MPS 路径完成相同流程，最终所有 Cell 与 CPU 参考逐项一致。</p>'
    body+='''</section><section><h2>仍然是局部 Cell runtime</h2><p class="note">原始 constructor DAG 由输入提供；尚未从任意 .egg 自动识别所有 Cell，也未实现跨 Cell 内部 e-class 的全局 union/rebuild。这里支持外部输入身份的 union、完整局部闭包去重和依赖通知。遇到不支持的 GPU 接口或没有匹配，保留该 constructor 事实，由原生 Bake 构建上下文，不能据此声称纯 GPU 或端到端加速。</p><p>按完整闭包判重仍有 CPU 开销。GPU 缓冲区按当前批次分配，别名修复后会重新打包下一批；目前不是持久 GPU 服务。</p></section><p><a href="report.json">完整统计与验证</a></p></html>'''
    (out/'index.html').write_text(body)


def run(out,gpu=True,skip_build=False):
    out.mkdir(parents=True,exist_ok=False)
    binary=ROOT/'target/release/examples/native_math_baseline'
    if not skip_build:
        subprocess.run(['cargo','build','--offline','--release','--example','lhs_ast','--example','native_math_baseline'],cwd=ROOT,check=True)
    source=(HERE/'rules.egg').read_text()
    subprocess.run([ROOT/'target/release/examples/lhs_ast',HERE/'rules.egg',out/'rules.json'],check=True)
    ast=json.loads((out/'rules.json').read_text())
    cpu,cpu_keys=exercise(out/'cpu',binary,source,ast,'cpu')
    gpu_report=None
    if gpu:
        executable=out/'gpu-runtime'
        subprocess.run(['swiftc','-O',HERE.parent/'moment_compiler/gpu.swift','-o',executable],check=True)
        gpu_report,gpu_keys=exercise(out/'gpu',binary,source,ast,'gpu',executable)
        assert gpu_keys==cpu_keys
    report=dict(scope='adaptive local constructor-Cell runtime; native fallback/Bake; not a full GPU egglog engine',
                cpu=cpu,gpu=gpu_report,cpu_gpu_states_equal=gpu_report is not None,
                provenance=dict(base_head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),
                    native_binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
                    runtime_sha256=hashlib.sha256((HERE/'runtime.py').read_bytes()).hexdigest(),
                    gpu_source_sha256=hashlib.sha256((HERE.parent/'moment_compiler/gpu.swift').read_bytes()).hexdigest()))
    (out/'report.json').write_text(json.dumps(report,indent=2)+'\n');render(out,report)
    return report


if __name__=='__main__':
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--output',type=Path,required=True)
    ap.add_argument('--cpu-only',action='store_true')
    ap.add_argument('--skip-build',action='store_true')
    a=ap.parse_args();r=run(a.output.resolve(),not a.cpu_only,a.skip_build)
    print(json.dumps({k:v['after'] if v else None for k,v in [('cpu',r['cpu']),('gpu',r['gpu'])]},indent=2))
