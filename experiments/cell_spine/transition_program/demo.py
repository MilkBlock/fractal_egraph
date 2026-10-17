"""Four rule families, shared GPU conditions, and two dependent Cell stages."""
import argparse
from collections import Counter
import hashlib
import html
import json
from pathlib import Path
import subprocess
import sys

ROOT=Path(__file__).resolve().parents[3]
sys.path.insert(0,str(ROOT))
from experiments.cell_spine.transition_program.compiler import Bake,Program
from experiments.cell_spine.moment_compiler.compiler import bit_code,execute
from experiments.cell_spine.factorized_boundary.demo import Graph,native,sexpr
from experiments.cell_spine.shared_matching.demo import source_text
from tools.bench.math_graph_equal import verify

HERE=Path(__file__).resolve().parent
CONSTRUCTORS=['Var','Add','Mul','Pow','Sin','Cos','Diff','Integral']


def input_term(t,values):
    return ('Var',f'v{values[t[1]]}') if t[0]=='Input' else (t[0],*(input_term(c,values) for c in t[1:]))


def ref_term(t,values):
    return ('Var',f'v{values[t[1]][t[2]]}') if t[0]=='Ref' else (t[0],*(ref_term(c,values) for c in t[1:]))


def decode(codes):
    return [[sum((x==1)<<j for j,x in enumerate(v)) for v in cell] for cell in codes]


def example(bake,ast):
    p=Program(bake,ast)
    a=('Add',('Input',0),('Input',1))
    seeds=dict(m=('Mul',a,('Input',2)),p=('Pow',a,('Input',2)),a=a,s=('Sin',a),c=('Cos',a))
    tids={name:bake.add(seed) for name,seed in seeds.items()}
    cells={}
    examples=[('m0','m',[0,1,2]),('m1','m',[1,0,3]),('m_bad','m',[0,4,5]),
              ('m2','m',[0,1,6]),('m_alias','m',[0,1,2]),
              ('p0','p',[0,1,7]),('p1','p',[1,0,8]),('p_bad','p',[0,4,9]),('p2','p',[0,1,10]),
              ('a0','a',[0,1]),('a_bad','a',[0,4]),('s0','s',[0,1]),('c0','c',[1,0])]
    for name,kind,binding in examples:cells[name]=p.instance(name,tids[kind],binding)
    first=[];named={}
    for label,rule,l,r in [('factor','factor','m0','m1'),('wrong-factor','factor','m0','m_bad'),
                            ('alias-factor','factor','m0','m_alias'),('powers','powers','p0','p1'),
                            ('wrong-powers','powers','p0','p_bad'),('integral','integral-cos','c0','a0'),
                            ('derivative','diff-sin','a0','s0'),('wrong-derivative','diff-sin','a_bad','s0')]:
        branches=p.edge(label,rule,cells[l],cells[r]);first+=branches;named[label]=branches
    p.stages.append(first)
    second=[]
    for label,rule,l,r in [
        ('factor-again','factor',p.pick(named['factor']),cells['m2']),
        ('powers-again','powers',p.pick(named['powers']),cells['p2']),
        ('different-rule-after-integral','diff-sin',cells['a0'],p.pick(named['integral'])),
        ('different-rule-after-derivative','integral-cos',p.pick(named['derivative']),cells['a0']),
        ('blocked-after-mismatch','factor',p.pick(named['wrong-factor']),cells['m2']),
        ('blocked-after-alias','factor',p.pick(named['alias-factor']),cells['m2'])]:
        second+=p.edge(label,rule,l,r)
    p.stages.append(second)
    return p


def reference(c,jobs,ports,initial,skip_inactive=False):
    codes=[[tuple(v) for v in cell] for cell in ports];live=list(initial);stages=[]
    for job in jobs:
        before_codes=[[tuple(v) for v in cell] for cell in codes];before_live=live[:]
        tapes=[c.evaluate(cell)[0] if live[i] or not skip_inactive else
               [(0,)*n['width'] for n in c.nodes] for i,cell in enumerate(codes)]
        # Newly allocated target rows are inactive, and may not satisfy their
        # future partition. Transitions below check every active target guard.
        answers=execute(job['plan'],tapes,validate=False)
        statuses=[]
        for t in job['transitions']:
            target=t['target'];live[target]=0
            if not all(before_live[i] for i in t['sources']) or not all(answers[q][i] for q,i in t['conditions']):
                statuses.append(0);continue
            values=[before_codes[i][slot] for i,slot in t['bindings']]
            codes[target]=values+[bit_code(0,c.bits)]*(c.slots-len(values))
            partition=[values.index(v) for v in values]
            if partition!=t['partition']:statuses.append(2);continue
            statuses.append(1);live[target]=1
        stages.append(dict(outputs=answers,feature_values=[v for tape in tapes for row in tape for v in row],
                           transition_status=statuses,cell_values=[v for cell in codes for row in cell for v in row],
                           cell_valid=live[:],before_codes=before_codes,before_live=before_live))
    return stages


def native_audit(out,binary,source,p,jobs,stages):
    results=[];state_checks=[];rhs_only=[]
    for si,(job,state) in enumerate(zip(jobs,stages)):
        values=decode(state['before_codes']);after_values=decode([
            [state['cell_values'][(i*p.c.slots+j)*p.c.bits:(i*p.c.slots+j+1)*p.c.bits] for j in range(p.c.slots)]
            for i in range(len(p.instances))])
        # Each supplied boundary has a source rule and directed endpoint pair.
        active={}
        for t,status in zip(job['transitions'],state['transition_status']):
            b=p.branches[t['branch']]
            if all(state['before_live'][i] for i in (b['left'],b['right'])):active[b['label']]=b
        labels=sorted(active);indices={name:i for i,name in enumerate(labels)}
        needed=sorted({i for b in active.values() for i in (b['left'],b['right'])})
        seeds='\n'.join(f'(let cell{i} {sexpr(input_term(p.bake.templates[p.instances[i]["template"]]["seed"],values[i]))})' for i in needed)
        constructors=[];probe=[];offers=[]
        for ri,(name,r) in enumerate(sorted(p.rules.items())):
            var=r['variables'];left,right=r['lhs'][1:]
            constructors.append(f'(Hit{ri} i64 '+ ' '.join('Math' for _ in var)+')')
            probe.append(f'(relation Offer{ri} (i64 Math Math Math))')
            probe.append(f'(rule ((Offer{ri} event left right root) (= root ({r["lhs"][0]} left right)) '
                         f'(= left {source_text(left)}) (= right {source_text(right)})) '
                         f'((Hit{ri} event {" ".join(var)})) :ruleset probe)')
            for label,b in active.items():
                if b['rule']==name:
                    offers.append(f'(Offer{ri} {indices[label]} cell{b["left"]} cell{b["right"]} '
                                  f'({r["lhs"][0]} cell{b["left"]} cell{b["right"]}))')
        env=source+'\n(datatype Audit '+' '.join(constructors)+')\n(ruleset probe)\n'+'\n'.join(probe)
        native(binary,out/f'native-stage{si}',env,seeds,'\n'.join(offers)+'\n(run-schedule (run probe))',
               CONSTRUCTORS+[f'Hit{i}' for i in range(len(p.rules))])
        graph=Graph(out/f'native-stage{si}/graph.json')
        actual={(r['op'],int(r['args'][0][1]),*(v for _,v in r['args'][1:])) for r in graph.rows if r['op'].startswith('Hit')}
        expected=set();by_rule=Counter()
        rule_ids={name:i for i,name in enumerate(sorted(p.rules))}
        for t,status in zip(job['transitions'],state['transition_status']):
            b=p.branches[t['branch']]
            if status:
                var=p.rules[b['rule']]['variables']
                expected.add((f'Hit{rule_ids[b["rule"]]}',indices[b['label']],
                    *(graph.value(ref_term(b['binding_terms'][v],values)) for v in var)))
                by_rule[b['rule']]+=1
            if status!=1:continue
            target=t['target'];tid=p.instances[target]['template'];template=p.bake.templates[tid]
            actual_seed=(p.rules[b['rule']]['lhs'][0],
                *(input_term(p.bake.templates[p.instances[i]['template']]['seed'],values[i]) for i in (b['left'],b['right'])))
            encoded_seed=input_term(template['seed'],after_values[target])
            rhs=input_term(b['rhs'],after_values[target])
            name=f'{si}-{t["branch"]}'
            native(binary,out/f'closures/{name}/source',source,f'(let root {sexpr(actual_seed)})',
                   f'(check (= root {sexpr(rhs)}))',CONSTRUCTORS)
            native(binary,out/f'closures/{name}/encoded',source,f'(let root {sexpr(encoded_seed)})','',CONSTRUCTORS)
            equal=verify(out/f'closures/{name}/source/graph.json',out/f'closures/{name}/encoded/graph.json')
            state_checks.append(dict(label=b['label'],**equal))
            if b['label']=='powers':
                native(binary,out/'rhs-only',source,f'(let root {sexpr(rhs)})','',CONSTRUCTORS)
                short=Graph(out/'rhs-only/graph.json')
                assert len(short.rows)<equal['enodes']
                try:
                    verify(out/f'closures/{name}/source/graph.json',out/'rhs-only/graph.json')
                except AssertionError:
                    pass
                else:raise AssertionError('RHS-only must lose one-way rewrite history')
                rhs_only.append(dict(label=b['label'],full_enodes=equal['enodes'],
                    rhs_only_enodes=len(short.rows),full_eclasses=equal['eclasses'],
                    rhs_only_eclasses=len(short.by_class),exact_equal=False))
        assert actual==expected,(si,len(actual),len(expected),actual-expected,expected-actual)
        results.append(dict(stage=si,native_full_bindings=len(actual),by_rule=dict(by_rule),exact=True))
    return dict(matches=results,full_closure_checks=state_checks,rhs_only_negative_ablation=rhs_only)


def run(out,gpu=True,skip_build=False):
    out.mkdir(parents=True,exist_ok=False)
    binary=ROOT/'target/release/examples/native_math_baseline'
    if not skip_build:
        subprocess.run(['cargo','build','--offline','--release','--example','lhs_ast','--example','native_math_baseline'],cwd=ROOT,check=True)
    source=(HERE/'rules.egg').read_text()
    upstream=(ROOT/'egglog/tests/math-microbenchmark.egg').read_text()
    assert all(line in upstream for line in source.splitlines() if line.startswith('(rewrite'))
    subprocess.run([ROOT/'target/release/examples/lhs_ast',HERE/'rules.egg',out/'rules.json'],check=True)
    ast=json.loads((out/'rules.json').read_text())
    bake=Bake(out/'bake',binary,source);p=example(bake,ast)
    c,jobs=p.lower();p.c=c
    ports=[];initial=[]
    for inst in p.instances:
        values=inst['binding'] or []
        ports.append([bit_code(v,c.bits) for v in values]+[bit_code(0,c.bits)]*(c.slots-len(values)))
        initial.append(int(inst['initial']))
    payload=dict(circuit=c.serialize(),ports=ports,active=initial,jobs=jobs,chain_stages=True)
    (out/'program.json').write_text(json.dumps(payload,indent=2)+'\n')
    states=reference(c,jobs,ports,initial)
    audit=native_audit(out,binary,source,p,jobs,states)
    # The exact same static program must respond to new input data. Reverse
    # which factoring edge is enabled; no new templates or connection search.
    heldout_ports=[[tuple(v) for v in cell] for cell in ports]
    for name,values in [('m1',[0,4,3]),('m_bad',[1,0,5])]:
        i=next(i for i,x in enumerate(p.instances) if x['name']==name)
        heldout_ports[i]=[bit_code(v,c.bits) for v in values]+[bit_code(0,c.bits)]*(c.slots-len(values))
    heldout=reference(c,jobs,heldout_ports,initial)
    assert heldout[0]['transition_status'] != states[0]['transition_status']
    assert heldout[1]['transition_status'] != states[1]['transition_status']
    heldout_audit=native_audit(out/'heldout',binary,source,p,jobs,heldout)
    gpu_reports=[]
    if gpu:
        executable=out/'gpu'
        subprocess.run(['swiftc','-O',ROOT/'experiments/cell_spine/moment_compiler/gpu.swift','-o',executable],check=True)
        for half in (False,True):
            name='fp16' if half else 'fp32'
            (out/f'{name}-program.json').write_text(json.dumps(dict(payload,half_inputs=half))+'\n')
            subprocess.run([executable,out/f'{name}-program.json',out/f'{name}-gpu.json'],check=True)
            raw=json.loads((out/f'{name}-gpu.json').read_text())
            assert raw['chained']
            for a,b in zip(states,raw['results']):
                for key in ('outputs','feature_values','transition_status','cell_values','cell_valid'):
                    assert a[key]==b[key],(name,key)
            heldout_payload=dict(payload,ports=heldout_ports,half_inputs=half)
            (out/f'heldout-{name}-program.json').write_text(json.dumps(heldout_payload)+'\n')
            subprocess.run([executable,out/f'heldout-{name}-program.json',out/f'heldout-{name}-gpu.json'],check=True)
            hr=json.loads((out/f'heldout-{name}-gpu.json').read_text())
            assert hr['chained']
            for a,b in zip(heldout,hr['results']):
                for key in ('outputs','feature_values','transition_status','cell_values','cell_valid'):
                    assert a[key]==b[key],('heldout',name,key)
            gpu_reports.append(dict(backend=raw['backend'],device=raw['device'],all_values_equal=True,
                heldout_same_program_equal=True,single_submission=True,
                program_gpu_seconds=raw['program_gpu_seconds'],
                stages=[dict(name=r['name'],status=r['transition_status']) for r in raw['results']]))
    details=[]
    for si,(job,state) in enumerate(zip(jobs,states)):
        for t,status in zip(job['transitions'],state['transition_status']):
            b=p.branches[t['branch']];target=p.instances[t['target']]['template']
            details.append(dict(branch=t['branch'],stage=si,label=b['label'],rule=b['rule'],status=status,
                target=t['target'],template=target,source_cells=[b['left'],b['right']],
                seed=sexpr(bake.templates[target]['seed']),rhs=sexpr(b['rhs']),bindings=b['bindings']))
    report=dict(scope='finite two-stage, supplied-boundary program; no global egglog hashcons/union runtime',
        source_rules={name:r['source'] for name,r in p.rules.items()},source_instances=sum(initial),
        reserved_cells=len(p.instances),baked_templates=len(bake.templates),bits=c.bits,slots=c.slots,
        stages=[dict(stage=i,branches=len(j['transitions']),statuses=dict(Counter(s['transition_status'])),
            shared_matrices=len(j['plan']['blocks']),logical_macs=sum(len(b['wires'])**2*b['width'] for b in j['plan']['blocks'])) for i,(j,s) in enumerate(zip(jobs,states))],
        transitions=details,native=audit,gpu=gpu_reports,
        heldout=dict(same_compiled_program=True,changed_input_instances=['m1','m_bad'],
                     statuses=[s['transition_status'] for s in heldout],native=heldout_audit),
        provenance=dict(base_head=subprocess.check_output(['git','rev-parse','HEAD'],text=True,cwd=ROOT).strip(),
            rules_sha256=hashlib.sha256((HERE/'rules.egg').read_bytes()).hexdigest(),
            native_binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
            compiler_sha256=hashlib.sha256((HERE/'compiler.py').read_bytes()).hexdigest(),
            gpu_source_sha256=hashlib.sha256((HERE.parent/'moment_compiler/gpu.swift').read_bytes()).hexdigest()))
    (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    (out/'library.json').write_text(json.dumps(dict(templates=bake.templates,instances=p.instances),indent=2)+'\n')
    render(out,report)
    return report


def render(out,r):
    rows=[]
    names={0:'不匹配／上游无效',1:'新 Cell 有效',2:'需要输入别名专化'}
    for t in r['transitions']:
        rows.append('<tr>'+''.join('<td>'+html.escape(str(v))+'</td>' for v in [t['stage'],t['label'],t['rule'],names[t['status']],t['rhs']])+'</tr>')
    text='''<!doctype html><html lang="zh"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>GPU Cell 转移 · 两阶段</title>
<style>body{font:16px system-ui;line-height:1.8;background:#f4f7fa;color:#173047;max-width:1180px;margin:32px auto;padding:0 20px}section{background:white;border:1px solid #d9e1ea;border-radius:12px;padding:24px;margin:18px 0}table{border-collapse:collapse;width:100%;font-size:13px}td,th{padding:8px;border:1px solid #d9e1ea}pre{background:#edf2f6;padding:16px;overflow:auto}.note{border-left:4px solid #b67a1d;padding-left:16px}</style>
<h1>从匹配结果到下一次 Cell 转移</h1><p>四族真实 math rewrite，共享条件矩阵；GPU 生成目标输入矩阵，再供下一阶段读取。</p>
<section><pre>父 Cell 输入编码 + 外部给定的连接
    ↓ 共享特征／矩阵判断
    ↓ 按原始 RHS 和保留的父闭包来源编译接线
新 Cell 的输入编码 + 有效位
    ↓ 下一阶段 GPU 特征构造
再次匹配／再次转移</pre><p>两阶段编码进同一次 GPU 提交，中间状态留在 GPU 缓冲区。CPU 不读取第一阶段的成功结果，也不据此决定第二阶段匹配什么。阶段快照使用设备内复制，全部完成后才回读核对。</p></section>
<section><h2>不能只保存 RHS</h2><pre>Mul(Pow(k,b),Pow(k,c)) → Pow(k,Add(b,c))</pre><p>右式不能靠这条单向规则恢复两个原始 Pow 节点。目标模板因此保留两个父 Cell 的构造来源和新边界，再用 native saturation 得到完整闭包。GPU 只复制这个目标模板需要的输入坐标。</p></section>
<section><h2>具体结果</h2><table><tr><th>阶段</th><th>连接</th><th>规则</th><th>结果</th><th>对应 RHS</th></tr>'''+''.join(rows)+'''</table></section>
<section><h2>保证及限制</h2><p>匹配成功但目标输入发生未 Bake 的别名关系时，返回“需要专化”；不会把它伪装成不匹配，也不会让后续阶段读取无效目标。普通不匹配和上游无效都不会继续推导。</p><p>每个接受的目标与从真实父上下文重新执行 native egglog 的完整 e-node/e-class 状态核对，保留 one-way rewrite 的旧事实。FP32 和 FP16 输入两种 GPU 路径逐项核对输入、输出与有效位。再改变两个输入实例，使用完全相同的编译程序，让成功/失败分支翻转；两阶段结果仍与原生一致。</p><p class="note">外部连接和两阶段范围由实验明确给定。没有自动发掘所有边界，没有生产级全局 hashcons、union/rebuild、目标去重或动态显存分配；不能称为完整 GPU egglog。</p></section><p><a href="report.json">报告</a> · <a href="program.json">GPU 程序</a> · <a href="library.json">Bake 模板与实例</a></p></html>'''
    (out/'index.html').write_text(text)


if __name__=='__main__':
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--output',type=Path,required=True)
    ap.add_argument('--skip-build',action='store_true')
    ap.add_argument('--cpu-only',action='store_true')
    a=ap.parse_args();r=run(a.output.resolve(),not a.cpu_only,a.skip_build)
    print(json.dumps({k:r[k] for k in ('baked_templates','stages','native','gpu')},indent=2))
