"""Cold/warm fact-specialization ablation with new bindings and an input union."""
import argparse
import hashlib
import html
import json
from pathlib import Path
import subprocess
import sys

ROOT=Path(__file__).resolve().parents[3]
sys.path.insert(0,str(ROOT))
from experiments.cell_spine.transition_program import adaptive_demo as adaptive
from experiments.cell_spine.transition_program.runtime import Runtime

HERE=Path(__file__).resolve().parent


class FactOnlyRuntime(Runtime):
    """Ablation: supplied contexts are closed directly, without boundary match.

    This does NOT discover unknown neighbors or bindings. Its input already
    specifies the complete external constructor and both child Cells.
    """
    def process(self,ready):
        for n,left,right in ready:
            seed,values=self.context(self.nodes[n]['op'],left,right)
            target=self.cells.intern(self.library.add(seed),values)
            self.stats['direct_fact_contexts']+=1
            self.remember(n,left,right,target)
        self.batches.append(dict(matrices=0,branches=0,ready_nodes=[n for n,_,_ in ready],
                                 direct_fact_bake=True,logical_matrix_macs=0))


def metrics(rt):
    s={} if rt.specializations is None else rt.specializations.stats
    return dict(native_bakes=rt.library.stats['native_bakes'],
        matrix_batches=sum(b['matrices']>0 for b in rt.batches),
        logical_matrix_macs=sum(b.get('logical_matrix_macs',0) for b in rt.batches),
        fact_replays=s.get('replays',0),certificates=s.get('certificates',0),
        certificate_native_bakes=s.get('certificate_native_bakes',0),
        ground_closure_keys=rt.cells.stats['ground_closure_keys'],
        native_fallbacks=rt.stats['native_context_fallbacks'])


def exercise(out,binary,source,ast,enabled,backend='cpu',gpu_binary=None,check_native=False,fact_only=False):
    runtime=FactOnlyRuntime if fact_only else Runtime
    rt=runtime(out,binary,source,ast,backend,gpu_binary,specialize_facts=enabled)
    phases=[];snapshots=[]
    for phase,offset,refine in [('cold',0,False),('new-bindings',100,False),
                                ('new-equality',None,True),('transferred-equality',1000,True)]:
        before=metrics(rt);start=len(rt.nodes)
        if offset is not None:
            adaptive.fixture(rt,offset)
            nodes=list(range(start,len(rt.nodes)))
        else:nodes=list(range(15,20))
        if refine:rt.union_inputs((offset or 100)+1,(offset or 100)+3)
        rt.drain();after=metrics(rt)
        assert len(rt.outputs)==len(rt.nodes)
        checks=adaptive.audit(rt,out/f'oracle-{phase}',binary,source,nodes) if check_native else []
        phases.append(dict(phase=phase,metrics={k:after[k]-before[k] for k in before},
                           observed_nodes=nodes,native_checks=checks))
        snapshots.append([rt.cells.get(rt.resolve(('node',n)))['key'] for n in range(len(rt.nodes))])
    if enabled:
        for phase in (phases[1],phases[3]):
            assert phase['metrics']['fact_replays']>0
            assert phase['metrics']['matrix_batches']==0
            assert phase['metrics']['native_bakes']==0
        assert phases[2]['metrics']['certificates']>0, 'new fact conditions must not reuse an invalid guard'
    return dict(enabled=enabled,backend=backend,fact_only=fact_only,phases=phases,summary=rt.summary()),snapshots


def render(out,r):
    def table(headers,rows):
        return '<table><tr>'+''.join('<th>'+html.escape(str(v))+'</th>' for v in headers)+'</tr>'+''.join('<tr>'+''.join('<td>'+html.escape(str(v))+'</td>' for v in row)+'</tr>' for row in rows)+'</table>'
    rows=[]
    for a,b in zip(r['baseline']['phases'],r['specialized']['phases']):
        x,y=a['metrics'],b['metrics']
        rows.append([a['phase'],x['matrix_batches'],y['matrix_batches'],x['native_bakes'],y['native_bakes'],y['fact_replays']])
    body='''<!doctype html><html lang="zh"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Fact 驱动的 Cell Specialization</title>
<style>body{font:16px system-ui;line-height:1.8;max-width:1150px;margin:32px auto;padding:0 22px;background:#f3f7fa;color:#163149}section{padding:24px;margin:18px 0;background:white;border:1px solid #dce5ee;border-radius:12px}table{border-collapse:collapse;width:100%;font-size:14px}td,th{border:1px solid #dce5ee;padding:9px;vertical-align:top}pre{background:#edf2f6;padding:16px;overflow:auto}.note{border-left:4px solid #bc7b2c;padding-left:14px}</style>
<h1>把观察到的事实条件编译为可迁移的 Cell 转移</h1><section><pre>两个父 Cell 模板 + 外部 constructor + 输入相等关系
       ↓ 原生符号闭包验证（首次出现）
目标模板 + 输入槽位复制
       ↓ 同样的事实条件、换一组具体输入
直接生成目标 Cell；跳过重复矩阵匹配和原生 Bake</pre><p>守卫包括所有输入分组，不同组当前必须保持不同身份。这里只泛化输入名字，不放宽条件，也不预先枚举所有别名组合。</p></section>
<section><h2>冷态／新输入／新相等关系对照</h2>'''+table(['阶段','原流程矩阵批次','启用专化','原流程 Bake','启用专化 Bake','直接迁移次数'],rows)
    body+='''<p>两个变体都保留之前的模板缓存、别名修复和实例去重。区别仅为是否学习这种参数化事实转移。新输入使用全新的身份；不是同一实例命中缓存。</p><p class="note">统计的是原生 Bake 调用、矩阵批次和逻辑工作，不是端到端速度。直接迁移仍需在 CPU 分类少量输入身份、复制槽位并做完整闭包实例判重。</p></section><section><h2>为什么可以迁移</h2><p>规则环境不读取输入名字或原生数值，且只包含正向 constructor rewrite。保持相等分组不变的输入改名，会同步改名整个推导。首次学习时，完整的源事实闭包与目标闭包已经按符号输入核对。因此它是有条件的精确复用，不是频率猜测。</p></section>'''
    body+='<section><h2>再做一个更直接的对照</h2><p>如果两个孩子和外部 constructor 已经给定，可以直接把这份事实上下文送去 Bake，再记录相同的专化证明。这个对照完全不做矩阵匹配：</p>'
    body+=table(['阶段','原流程 Bake','匹配后学习 Bake','直接按事实 Bake'],
                [[a['phase'],a['metrics']['native_bakes'],b['metrics']['native_bakes'],c['metrics']['native_bakes']]
                 for a,b,c in zip(r['baseline']['phases'],r['specialized']['phases'],r['direct_fact']['phases'])])
    body+='<p class="note">这个输入模型已经给出了完整相邻关系。对照说明此处的收益来自事实专化与闭包复用，不能将它归功于 GPU。GPU 对尚未确定的邻居／binding 搜索是否有价值，需要另行验证。</p></section>'
    recipes=r['specialized']['summary']['fact_specializations']['recipes']
    entries=[]
    for x in sorted(recipes,key=lambda x:(-x['replays'],x['id'])):
        groups=' | '.join('='.join(g) for g in x['equal_groups'])
        entries.append([x['id'],x['operator'],str(x['source_templates']),groups,x['target_template'],', '.join(x['output_ports']),x['replays']])
    body+='<section><h2>实际学到的事实转移</h2><p>L/R 表示左／右父 Cell 的参数位置；竖线分隔的组必须保持不同。表中不包含具体输入名字。</p>'+table(['编号','外部事实','父模板','输入分组','目标模板','复制哪些槽位','迁移次数'],entries)+'</section>'
    body+='<section><h2>验证与边界</h2><p>所有阶段与不启用专化的运行结果相同。首次输入、新名字、新 union、union 后的新名字都进行了完整状态比较。新增相等关系会改变守卫，触发新证明，不会沿用旧转移。</p>'
    if r['gpu']:body+='<p>真实 Metal/MPS 的冷态匹配和后续直接迁移，与 CPU 参考的每阶段完整 Cell 状态相同。</p>'
    body+='''<p class="note">仅用于当前正向 constructor 环境和局部 Cell 运行时。新规则环境、primitive/guard/set、跨 Cell 内部全局 union 不在保证内。尚未自动寻找最弱的事实条件；完整闭包判重和每批 GPU 进程启动的开销仍存在。</p></section><p><a href="report.json">完整对照</a></p></html>'''
    (out/'index.html').write_text(body)


def run(out,gpu=True,skip_build=False):
    out.mkdir(parents=True,exist_ok=False)
    binary=ROOT/'target/release/examples/native_math_baseline'
    if not skip_build:
        subprocess.run(['cargo','build','--offline','--release','--example','lhs_ast','--example','native_math_baseline'],cwd=ROOT,check=True)
    source=(HERE/'rules.egg').read_text()
    subprocess.run([ROOT/'target/release/examples/lhs_ast',HERE/'rules.egg',out/'rules.json'],check=True)
    ast=json.loads((out/'rules.json').read_text())
    baseline,base_states=exercise(out/'baseline',binary,source,ast,False,check_native=True)
    specialized,special_states=exercise(out/'specialized',binary,source,ast,True)
    assert special_states==base_states
    direct,direct_states=exercise(out/'direct-fact',binary,source,ast,True,fact_only=True)
    assert direct_states==base_states
    gpu_report=None
    if gpu:
        executable=out/'gpu-runtime'
        subprocess.run(['swiftc','-O',HERE.parent/'moment_compiler/gpu.swift','-o',executable],check=True)
        gpu_report,gpu_states=exercise(out/'gpu',binary,source,ast,True,'gpu',executable)
        assert gpu_states==base_states
    r=dict(scope='fact-guarded local Cell specialization; native cold proof; no global egglog speedup claim',
           baseline=baseline,specialized=specialized,direct_fact=direct,gpu=gpu_report,all_phase_states_equal=True,
           provenance=dict(base_head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),
            rules_sha256=hashlib.sha256(source.encode()).hexdigest(),
            specialization_sha256=hashlib.sha256((HERE/'specializations.py').read_bytes()).hexdigest(),
            runtime_sha256=hashlib.sha256((HERE/'runtime.py').read_bytes()).hexdigest(),
            gpu_source_sha256=hashlib.sha256((HERE.parent/'moment_compiler/gpu.swift').read_bytes()).hexdigest(),
            native_binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest()))
    (out/'report.json').write_text(json.dumps(r,indent=2)+'\n');render(out,r)
    return r


if __name__=='__main__':
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--output',type=Path,required=True)
    ap.add_argument('--cpu-only',action='store_true')
    ap.add_argument('--skip-build',action='store_true')
    a=ap.parse_args();r=run(a.output.resolve(),not a.cpu_only,a.skip_build)
    print(json.dumps({name:[dict(phase=x['phase'],**x['metrics']) for x in r[name]['phases']] for name in ['baseline','specialized','direct_fact']},indent=2))
