"""Bake -> exact coordinate circuit -> CPU/Metal matrix program -> native audit."""
import argparse
from collections import Counter
import hashlib
import html
from itertools import combinations_with_replacement
import json
from pathlib import Path
import subprocess
import sys

ROOT=Path(__file__).resolve().parents[3]
sys.path.insert(0,str(ROOT))
from experiments.cell_spine.moment_compiler.compiler import (
    Circuit, bit_code, lift2, certify_commutative, layout, execute, reconstruct, check_guards)
from experiments.cell_spine.tensor_interaction import demo as tensor
from experiments.cell_spine.layered_context import demo as layers
from experiments.cell_spine.marker_context import demo as markers
from experiments.cell_spine.dense_response import demo as density
from experiments.cell_spine.factorized_boundary.demo import Graph, Family, bind, native, sexpr

HERE=Path(__file__).resolve().parent


def exhaustive():
    audits=[]
    for bits in range(1,7):
        seen={}
        codes=[lift2(bit_code(i,bits)) for i in range(1<<bits)]
        for a,b in combinations_with_replacement(range(1<<bits),2):
            f=tuple((u+v)//2 for u,v in zip(codes[a],codes[b]))
            assert f not in seen
            seen[f]=(a,b)
        audits.append(dict(bits=bits,width=len(codes[0]),unordered_pairs_including_aliases=len(seen)))
    # First moments collide; degree two separates them.
    linear=lambda pair:tuple(sum(v) for v in zip(*(bit_code(i,2) for i in pair)))
    second=lambda pair:tuple(sum(v) for v in zip(*(lift2(bit_code(i,2)) for i in pair)))
    assert linear((0,3))==linear((1,2)) and second((0,3))!=second((1,2))
    # Degree two does NOT encode arbitrary bags: cube parity is a counterexample.
    even=[i for i in range(8) if i.bit_count()%2==0]
    odd=[i for i in range(8) if i.bit_count()%2==1]
    bag=lambda ids:tuple(sum(v) for v in zip(*(lift2(bit_code(i,3)) for i in ids)))
    assert bag(even)==bag(odd)
    return dict(pair_exhaustive=audits,first_moment_collision=[[0,3],[1,2]],
                second_moment_four_leaf_collision=[even,odd])


def existing_consumers(plan,library,bindings,graph):
    """Validation adapter from actual Add facts, outside the numerical kernel.

    Connections are not inferred from factor compatibility. Each retained row
    remains a separate producer/consumer witness even when projections share.
    """
    anchors=[graph.value(bind(p['term'],bindings[p['instance']])) for p in plan['ports']]
    emits,labels=[],[]
    for q in plan['queries']:
        indices,records=[],[];n=len(q['wires'])
        for a,i in zip(q['channels'],q['channel_rows']):
            for b,j in zip(q['channels'],q['channel_rows']):
                left,right=plan['channels'][a],plan['channels'][b]
                key=('Add',(anchors[left['port']],anchors[right['port']]))
                if key in graph.lookup:
                    indices.append(i*n+j)
                    records.append([a,b,graph.lookup[key]])
        emits.append(indices);labels.append(records)
    return emits,labels


def union_audit(out,binary,c,library,plan,ports,tapes):
    """One explicit union event; independent native oracle checks completeness.

    Deliberately test the old library first. Aliases can invalidate it even when
    the numerical pair encoding itself is still exact.
    """
    ids={name:i for i,name in enumerate(tensor.ALPHABET)}
    before,after=ids['b'],ids['a']
    updated=[];new_ports=[];work=Counter()
    for old,base,(_,values) in zip(tapes,ports,tensor.CASES):
        slots={i for i,v in enumerate(values) if ids[v]==before}
        p=[bit_code(after,4) if i in slots else code for i,code in enumerate(base)]
        new,stats=c.evaluate(p,old,slots)
        new_ports.append(p);updated.append(new);work.update(stats)
    try:
        check_guards(plan,updated)
    except ValueError:
        pass
    else:
        raise AssertionError('union must invalidate old interface guard')
    # Deliberately bypass admission only to measure the counterexample. Normal
    # execution and the GPU backend refuse this stale plan.
    output=execute(plan,updated,validate=False);matched=reconstruct(plan,output)
    seeds='\n'.join(f'(let c{i} {sexpr(bind(library["templates"][t]["seed"],b))})' for i,(t,b) in enumerate(tensor.CASES))
    seeds+='\n(union (Var "a") (Var "b"))'
    terms=[sexpr(bind(p['term'],tensor.CASES[p['instance']][1])) for p in plan['ports']]
    witnesses='\n'.join(f'(Port {i} {t})' for i,t in enumerate(terms))
    grafts='\n'.join(f'(Graft {i} {j} (Add {a} {b}))' for i,a in enumerate(terms) for j,b in enumerate(terms))
    probe='''
(datatype Match (Hit i64 i64 Math Math Math))
(relation Port (i64 Math))
(relation Graft (i64 i64 Math))
(ruleset probe)
(rule ((Port i left) (Port j right) (Graft i j root)
       (= root (Add left right)) (= left (Mul k u)) (= right (Mul k v)))
      ((Hit i j k u v)) :ruleset probe)
'''
    native(binary,out/'union-native',tensor.RULES.read_text()+probe,seeds,
           witnesses+'\n'+grafts+'\n(run-schedule (run probe))',['Var','Add','Mul','Hit'])
    g=Graph(out/'union-native/graph.json')
    actual={(int(r['args'][0][1]),int(r['args'][1][1]),*(v for _,v in r['args'][2:])) for r in g.rows if r['op']=='Hit'}
    def predictions(lib,types,p,result):
        def field(ch,name):
            b=tensor.CASES[ch['instance']][1]
            return g.value(bind(lib['templates'][types[ch['instance']]]['channels'][ch['local']][name],b))
        expected=set()
        for i,a in enumerate(p['channels']):
            for j,b in enumerate(p['channels']):
                if result['gate'][i][j]:
                    expected.add((a['port'],b['port'],field(a,'k'),field(a,'u'),field(b,'u')))
        return expected
    stale=predictions(library,[t for t,_ in tensor.CASES],plan,matched)
    assert stale < actual, 'regression fixture must expose stale Bake incompleteness'
    # Specialize by equality partition, not names or data values. Keep exactly
    # the original observed port tags, adding newly available Mul alternatives.
    templates=[];lookup={};types=[];new_bakes=0
    for tid,values in tensor.CASES:
        canonical=[after if ids[v]==before else ids[v] for v in values]
        partition=tuple(canonical.index(x) for x in canonical)
        key=(tid,partition)
        if key not in lookup:
            lookup[key]=len(templates)
            original=library['templates'][tid]
            if partition==tuple(range(len(partition))):
                templates.append(original)
            else:
                def substitute(t):
                    return ('Input',partition[t[1]]) if t[0]=='Input' else (t[0],*(substitute(x) for x in t[1:]))
                seed=substitute(original['seed'])
                directory=out/f'alias-bake/{len(templates)}'
                native(binary,directory,tensor.RULES.read_text().replace('(Var String)','(RipenInput i64)'),
                       '(let root '+sexpr(seed)+')',constructors=['RipenInput','Add','Mul'])
                ag=Graph(directory/'graph.json');family=Family();chs=[];ps=[]
                for p in original['ports']:
                    term=substitute(p['term']);anchor=family.from_graph(ag,ag.value(term))
                    port=len(ps);ps.append(dict(p,term=term))
                    for (k,u),multiplicity in sorted(family.root_mul(anchor).items()):
                        chs.append(dict(port=port,k=k,u=u,recipe=0,local_spine_multiplicity=multiplicity))
                templates.append(dict(original,seed=seed,ports=ps,channels=chs,input_partition=partition))
                new_bakes+=1
        types.append(lookup[key])
    specialized=dict(library,templates=templates)
    sc=Circuit(c.bits,c.slots,c.certificate)
    sp=layout(sc,specialized,types,shared=True)
    st=[sc.evaluate(p)[0] for p in new_ports]
    so=execute(sp,st);sr=reconstruct(sp,so)
    repaired=predictions(specialized,types,sp,sr)
    assert repaired==actual,(len(repaired),len(actual))
    missing=min(actual-stale)
    example=[sexpr(g.representative(x)) for x in missing[2:]]
    payload=dict(circuit=sc.serialize(),ports=new_ports,jobs=[dict(name='union-specialized',plan=sp)])
    (out/'union-program.json').write_text(json.dumps(payload,indent=2)+'\n')
    (out/'alias-library.json').write_text(json.dumps(dict(templates=templates,types=types,
        partitions=[dict(source_template=t,partition=p,specialized_template=i) for (t,p),i in lookup.items()]),indent=2)+'\n')
    return dict(event='union Var(a) Var(b)',native_equal=True,
                stale_plan_canonical_bindings=len(stale),native_canonical_bindings=len(actual),
                missed_by_stale_plan=len(actual-stale),missing_example_k_u_v=example,
                specialized_plan_canonical_bindings=len(repaired),alias_specializations_baked=new_bakes,
                projected_binding_rows=sum(map(sum,sr['gate'])),coordinate_update=dict(work),
                same_compiled_plan=False,old_guard_rejected=True,plan_stats=sp['stats']),new_ports,st,so


def run(out,gpu=False,skip_build=False):
    out.mkdir(parents=True,exist_ok=False)
    if not skip_build:
        subprocess.run(['cargo','build','--offline','--release','--example','lhs_ast','--example','native_math_baseline'],cwd=ROOT,check=True)
    binary=ROOT/'target/release/examples/native_math_baseline'
    library=tensor.bake(out/'bake',binary)
    ast=json.loads((out/'bake/rules.json').read_text())
    c=Circuit(4,library['width'],certify_commutative(ast,'Add'))
    types=[t for t,_ in tensor.CASES]
    plans={f'{mode}-{style}':layout(c,library,types,mode,style=='shared')
           for mode in ('root','all') for style in ('fused','shared')}
    ports=[[bit_code(tensor.ALPHABET.index(v),4) for v in values]+
           [bit_code(0,4)]*(library['width']-len(values)) for _,values in tensor.CASES]
    evaluations=[c.evaluate(p) for p in ports]
    tapes=[v for v,_ in evaluations]
    construction=Counter()
    for _,counts in evaluations:construction.update(counts)
    answers={name:execute(plan,tapes) for name,plan in plans.items()}
    reconstructed={name:reconstruct(plan,answers[name]) for name,plan in plans.items()}
    book=tensor.Codebook(tensor.ALPHABET)
    oldcells=[book.encode(t,v,library['templates'][t]['arity'],library['width']) for t,v in tensor.CASES]
    prepared=layers.prepare(library,out/'bake')
    baseline={}
    marker=markers.MarkerPlan(library,'pair')
    marked=[marker.build(cell)[0] for cell in oldcells]
    for mode in ('root','all'):
        old=markers.marker_score(library,prepared,marked,marker,mode)
        # The old all-anchor port numbering includes non-Mul ports; channel
        # order and full candidate binding order remain the same.
        for style in ('fused','shared'):
            assert reconstructed[f'{mode}-{style}']['gate']==old['gate']
        baseline[mode]=old['stats']
    result=reconstructed['all-shared']
    records=tensor.native_oracle(out,binary,library,oldcells,book,result)
    vector_records=[dict(template=cell.template,ports=cell.ports) for cell in oldcells]
    existing=density.interaction_blocks(out,library,vector_records,result)
    graph=Graph(out/'existing-only/graph.json')
    bindings=[values for _,values in tensor.CASES]
    jobs=[]; native_emitted_counts={}
    for name,plan in plans.items():
        emits,labels=existing_consumers(plan,library,bindings,graph)
        selected=[[answers[name][qi][i] for i in row] for qi,row in enumerate(emits)]
        native_emitted_counts[name]=dict(external_connections=sum(map(len,emits)),
                                       successful_connections=sum(map(sum,selected)))
        jobs.append(dict(name=name,plan=plan,emits=emits))
    # Known graft/substitution updates are circuit dependencies, not new Join.
    old=tapes[0]
    updated,update_stats=c.evaluate(ports[1],old,{3})
    assert updated==tapes[1]
    pairnode=c.compile(('Add',('Input',1),('Input',2)))[1]
    assert updated[pairnode] is old[pairnode]
    # An alias is valid for this coordinate algebra; structural Bake validity is
    # still a separate issue. Do not advertise this as general union support.
    aliased=list(ports[1]);aliased[2]=aliased[1]
    alias,alias_stats=c.evaluate(aliased,tapes[1],{2})
    assert alias==c.evaluate(aliased)[0]
    union_result,union_ports,union_tapes,union_output=union_audit(out,binary,c,library,plans['all-shared'],ports,tapes)
    payload=dict(circuit=c.serialize(),ports=ports,jobs=jobs)
    (out/'program.json').write_text(json.dumps(payload,indent=2)+'\n')
    gpu_report=None
    if gpu:
        executable=out/'gpu'
        subprocess.run(['swiftc','-O',str(HERE/'gpu.swift'),'-o',str(executable)],check=True)
        (out/'stale-program.json').write_text(json.dumps(dict(payload,ports=union_ports))+'\n')
        rejected=subprocess.run([str(executable),str(out/'stale-program.json'),str(out/'stale-output.json')],capture_output=True,text=True)
        assert rejected.returncode != 0 and 'input partition invalidates' in rejected.stderr
        subprocess.run([str(executable),str(out/'program.json'),str(out/'gpu.json')],check=True)
        raw=json.loads((out/'gpu.json').read_text())
        flat=[x for tape in tapes for row in tape for x in row]
        for job,r in zip(jobs,raw['results']):
            assert r['name']==job['name']
            assert r['outputs']==answers[r['name']], r['name']
            assert r['feature_values']==flat
            assert r['emitted']==[[answers[r['name']][qi][i] for i in row] for qi,row in enumerate(job['emits'])]
        subprocess.run([str(executable),str(out/'union-program.json'),str(out/'union-gpu.json')],check=True)
        ur=json.loads((out/'union-gpu.json').read_text())['results'][0]
        assert ur['outputs']==union_output
        assert ur['feature_values']==[x for tape in union_tapes for row in tape for x in row]
        union_result['gpu_exact']=True
        # Signed binary/moment coordinates are exact FP16 values; use a float32
        # result matrix and compare every output, including union specialization.
        mixed=[]
        for stem,source,expected in [('mixed',payload,raw['results']),
                                     ('union-mixed',json.loads((out/'union-program.json').read_text()),[ur])]:
            (out/f'{stem}-program.json').write_text(json.dumps(dict(source,half_inputs=True))+'\n')
            subprocess.run([str(executable),str(out/f'{stem}-program.json'),str(out/f'{stem}-gpu.json')],check=True)
            observed=json.loads((out/f'{stem}-gpu.json').read_text())
            for a,b in zip(expected,observed['results']):
                assert a['outputs']==b['outputs'] and a['emitted']==b['emitted'] and a['feature_values']==b['feature_values']
            mixed.append(dict(case=stem,backend=observed['backend'],all_bits_equal=True,
                              timings=[{k:r[k] for k in ('name','gpu_seconds','submit_wait_seconds')} for r in observed['results']]))
        gpu_report=dict(device=raw['device'],backend=raw['backend'],all_bits_equal=True,
                        all_features_equal=True,consumer_gather_equal=True,
                        stale_plan_rejected_by_host_preflight=True,
                        mixed_precision=mixed,
                        timings=[{k:r[k] for k in ('name','gpu_seconds','submit_wait_seconds')} for r in raw['results']])
    report=dict(scope='standalone certified interface compiler; native Bake/oracle; not production engine acceleration',
        certificate=c.certificate,bits=4,templates=len(library['templates']),instances=len(ports),
        circuit_nodes=len(c.nodes),tape_components_per_cell=c.tape_width,
        base_components_per_cell=c.bits*c.slots,construction=dict(construction),
        baselines=baseline,plans={name:plan['stats'] for name,plan in plans.items()},
        blocks={name:[dict(rows=len(b['wires']),width=b['width']) for b in plan['blocks']] for name,plan in plans.items()},
        full_binding_matches=len(records),global_bindings=json.loads((out/'native-validation.json').read_text())['global_rule_bindings'],
        native_exact=True,existing_canonical_matches=existing['existing']['nonzeros'],
        consumers=native_emitted_counts,update=dict(t0_to_t1=update_stats,reused_pair_object=True,alias_coordinate_only=alias_stats),
        union=union_result,algebra=exhaustive(),gpu=gpu_report,
        provenance=dict(base_head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),
            native_binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
            rules_sha256=hashlib.sha256(tensor.RULES.read_bytes()).hexdigest(),
            compiler_sha256=hashlib.sha256((HERE/'compiler.py').read_bytes()).hexdigest(),
            gpu_source_sha256=hashlib.sha256((HERE/'gpu.swift').read_bytes()).hexdigest()))
    (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    render(out,report)
    return report


def render(out,r):
    def table(headers,rows):
        return '<table><tr>'+''.join('<th>'+html.escape(str(h))+'</th>' for h in headers)+'</tr>'+''.join('<tr>'+''.join('<td>'+html.escape(str(v))+'</td>' for v in row)+'</tr>' for row in rows)+'</table>'
    text='''<!doctype html><html lang="zh"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Cell 边界 · 精确坐标编译</title>
<style>body{font:16px system-ui;line-height:1.8;max-width:1150px;margin:36px auto;padding:0 22px;background:#f4f7fa;color:#183044}section{background:white;padding:24px;margin:20px 0;border-radius:12px;border:1px solid #dbe4ec}pre{overflow:auto;background:#eff3f7;padding:16px}table{border-collapse:collapse;width:100%}td,th{border:1px solid #dbe4ec;padding:10px;text-align:left}.note{border-left:4px solid #c47b1d;padding-left:16px}</style>
<h1>把边界匹配编译成固定矩阵程序</h1><p>四条真实 math rewrite · 两个 Bake 模板 · root 和内部触点 · 原生完整 binding 核对</p>
<section><h2>1. 输入编码不再要求正交</h2><pre>x → q 个 ±1 二进制坐标
x=y ⇔ ||code(x)-code(y)||² = 0

ℓ(x) = [x₁,…,xq, x₁x₂,…,x(q-1)xq]
Pair(a,b) = (ℓ(a)+ℓ(b))/2

q=4：16 种身份；Input 用4维，unordered Pair 用10维。
q=32：最多2³²种身份；Input 用32维，Pair 用528维。
</pre><p>二阶坐标精确区分无序 pair，包括 a+a。没有随机碰撞，也不枚举交换排列。不同树形仍分开；不同角色用拼接保留。</p></section>
<section><h2>2. 两种等价布局</h2><pre>融合： [Pair(a,b), code(c)] → 一次 GEMM → 距离为0？
共享： Pair 的距离矩阵 + c 的距离矩阵 → 总距离为0？

每个分量的平方距离非负；总距离为0，当且仅当全部条件成立。
已有子矩阵覆盖另一项所需的全部接线时，直接索引复用。
</pre><p>全部分组来自模板和相对槽位；不会先比较输入值，再把已知相等的位置凑成矩阵。</p>'''
    text+=table(['范围','Pair marker旧原型 MAC','新融合 MAC','新共享 MAC','共享矩阵数'],[[m,r['baselines'][m]['multiply_adds'],r['plans'][m+'-fused']['matrix_macs'],r['plans'][m+'-shared']['matrix_macs'],r['plans'][m+'-shared']['blocks']] for m in ('root','all')])
    text+='<p class="note">这里比较矩阵乘加，不是运行时间。新编码构造、投影复制、norm、逐元素距离与输出均有成本。完整报告逐项记录结构性工作量。</p></section>'
    text+='<section><h2>3. 真实 GPU 路径</h2><pre>二进制输入矩阵\n    ↓ Metal 固定特征电路\n共享 feature tape\n    ↓ 固定槽位投影\nMPS GEMM\n    ↓ 非负距离累加 + 精确零判定\n共享条件矩阵\n    ↓ 按真实外部连接回填（保留各自 binding）\n匹配结果</pre>'
    if r['gpu']:
        text+='<p>'+html.escape(r['gpu']['device'])+'：GPU 每个特征值、每个匹配位、每个外部连接输出都与整数 CPU 参考一致。</p>'
        text+='<p>FP16 输入 / FP32 输出的 MPS GEMM 也通过全部原始和 union 专化用例。当前坐标是有界整数；这不涉及近似相似度阈值。</p>'
        text+=table(['方案','GPU命令时间 ms','提交及等待 ms'],[[x['name'],round(x['gpu_seconds']*1000,3),round(x['submit_wait_seconds']*1000,3)] for x in r['gpu']['timings']])
    else:text+='<p>本次未运行 GPU；可加 --gpu 使用本机 Metal/MPS。</p>'
    text+='<p class="note">这是极小的正确性验证，包含启动开销；不能据此推断大规模吞吐或 egglog 加速。编译、JSON读写和证据回读不在 GPU 时间里。</p></section>'
    text+=f'<section><h2>4. union 暴露的接口失效</h2><p>无新增 union 时，{r["full_binding_matches"]} 条带触点 binding（去标签 {r["global_bindings"]} 条）与原生一致。只使用原图已有 Add 连接时，{r["existing_canonical_matches"]} 条 canonical binding 与原生一致。</p><p class="note">union(a,b) 后，仅更新坐标的旧计划只覆盖 {r["union"]["stale_plan_canonical_bindings"]} 条，而原生有 {r["union"]["native_canonical_bindings"]} 条。编码没有碰撞；是模板前提失效产生了新的 Mul 分解。</p><p>按输入相等关系重新 Bake {r["union"]["alias_specializations_baked"]} 个专化接口后，完整匹配恢复为 {r["union"]["specialized_plan_canonical_bindings"]} 条，并在 GPU 核对。这是一个 union fixture 的修复路径，尚未接入运行时自动失效系统，也未证明任意 union 都只需输入别名专化。GPU 目前重新构建特征；增量更新在 CPU 参考验证。</p></section>'
    text+='''<section><h2>5. 明确失败的推广</h2><pre>只存一阶和：{00,11} 与 {01,10} 碰撞 → 必须补二阶信息。
随意扩大为四叶袋：三位二进制的偶校验四点与奇校验四点
有相同的一阶、二阶和 → Pair 证明不能推广到任意大袋。

递归使用一般 unordered-pair moments 虽然精确，维度会增长：
4 → 10 → 65 → 2210 → 2445365。
应保留共享分量和结构角色，不应一味物化更大的特征。
</pre></section><p><a href="report.json">报告</a> · <a href="program.json">完整编译程序和输入</a> · <a href="graft-check.egg">原生验证</a></p></html>'''
    (out/'index.html').write_text(text)


if __name__=='__main__':
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--output',type=Path,required=True)
    ap.add_argument('--gpu',action='store_true')
    ap.add_argument('--skip-build',action='store_true')
    a=ap.parse_args();r=run(a.output.resolve(),a.gpu,a.skip_build)
    print(json.dumps({k:r[k] for k in ('plans','native_exact','full_binding_matches','existing_canonical_matches','gpu')},indent=2))
