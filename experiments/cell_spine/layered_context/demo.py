"""Exact depth-frontier batching over baked, partner-independent Cell codes.

One boundary rule (factoring); four native rules build the two closed templates.
No beam, candidate budget, or speculative match acceptance.
"""
from collections import Counter, defaultdict
import argparse
import html
import json
from pathlib import Path
import subprocess
import sys

ROOT=Path(__file__).resolve().parents[3]
sys.path.insert(0,str(ROOT))
from experiments.cell_spine.tensor_interaction import demo as tensor
from experiments.cell_spine.factorized_boundary.demo import Family, Graph, mm, transpose
from experiments.cell_spine.shared_matching.demo import class_index, locations

HERE=Path(__file__).resolve().parent


def shape_depths(term):
    if term[0]=='Input':
        return 'I',[0]
    if term[0]!='Add':
        raise ValueError('unsupported context constructor')
    a,b=[shape_depths(x) for x in term[1:]]
    if a[0]>b[0]:
        a,b=b,a
    return 'A('+a[0]+','+b[0]+')',[d+1 for d in a[1]+b[1]]


def prepare(library,bake_dir):
    """All shared anchor contexts and relative condition depths, computed once."""
    family,roots=Family(),[]
    for i,t in enumerate(library['templates']):
        g=Graph(bake_dir/f't{i}/graph.json')
        roots.append(family.from_graph(g,g.value(t['seed'])))
    index=class_index(family,roots)
    contexts=[]
    depths={}
    for tid,t in enumerate(library['templates']):
        contexts.append([dict(anchor=c,term=family.representative(c),location='root' if c==roots[tid] else 'non-root',path=p)
                         for c,p in sorted(locations(index,roots[tid]).items())])
        for ch in t['channels']:
            shape,ds=shape_depths(ch['k'])
            assert shape==library['recipes'][ch['recipe']]['shape']
            if ch['recipe'] in depths:
                assert depths[ch['recipe']]==ds
            depths[ch['recipe']]=ds
    return dict(contexts=contexts,depths=depths)


def batch_layout(library,prepared,cells,mode):
    all_ports,channels=[],[]
    mapping={}
    for instance,cell in enumerate(cells):
        for p in prepared['contexts'][cell.template]:
            if mode=='root' and p['location']!='root':
                continue
            mapping[instance,p['anchor']]=len(all_ports)
            all_ports.append(dict(instance=instance,**p))
    for instance,cell in enumerate(cells):
        t=library['templates'][cell.template]
        for local,ch in enumerate(t['channels']):
            p=t['ports'][ch['port']]
            key=(instance,p['anchor'])
            if key in mapping:
                channels.append(dict(instance=instance,local=local,port=mapping[key],recipe=ch['recipe']))
    # All offered Add(left,right) contexts. Constructor availability is certified
    # by Bake and includes every enode alternative, not one chosen representative.
    with_mul={c['port'] for c in channels}
    return all_ports,channels,dict(offered_contexts=len(all_ports)**2,
        constructor_rejected=len(all_ports)**2-len(with_mul)**2,
        constructor_continued=len(with_mul)**2)


def candidates(library,prepared,channels):
    result=[]
    width=library['width']
    for i,a in enumerate(channels):
        for j,b in enumerate(channels):
            plan=library['contractions'][a['recipe']][b['recipe']]
            alternatives=set()
            if plan is not None:
                for term in plan['terms']:
                    conditions={}
                    for (x,y),d in zip(term,prepared['depths'][a['recipe']]):
                        p,q=sorted((a['instance']*width+x,b['instance']*width+y))
                        conditions[p,q]=min(d,conditions.get((p,q),d))
                    alternatives.add(tuple(sorted((d,p,q) for (p,q),d in conditions.items())))
            # Simultaneous symmetry permutations can give the same conjunction.
            # OR over unique conjunctions preserves existential compatibility.
            result.append(dict(left=i,right=j,alternatives=tuple(sorted(alternatives))))
    return result


class EqualityBatch:
    def __init__(self,cells,width):
        self.width=width
        self.vectors=[v for c in cells for v in c.ports]
        self.n=len(self.vectors[0])
        self.cache={}
        self.stats=Counter()
        self.blocks=[]

    def known(self,p,q):
        self.stats['evidence_reads']+=1
        if p//self.width==q//self.width:
            # encode() guarantees distinct, valid inputs within each instance.
            return p==q
        return self.cache.get((p,q))

    def resolve(self,requests,depth):
        self.stats['condition_requests']+=len(requests)
        missing=set()
        for p,q in requests:
            if self.known(p,q) is None:
                missing.add((p,q))
            else:
                self.stats['known_condition_requests']+=1
        groups=defaultdict(set)
        for p,q in missing:
            groups[p%self.width,q%self.width].add((p,q))
        self.stats['missing_unique_conditions']+=len(missing)
        for slots,needed in sorted(groups.items()):
            left=sorted({p for p,_ in needed})
            right=sorted({q for _,q in needed})
            l=[self.vectors[p] for p in left]
            r=[self.vectors[q] for q in right]
            products=mm(l,transpose(r))
            reused=speculative=0
            for i,p in enumerate(left):
                for j,q in enumerate(right):
                    key=tuple(sorted((p,q)))
                    if self.known(*key) is not None:
                        reused+=1
                    if key not in needed:
                        speculative+=1
                    value=products[i][j]
                    if value not in (0,self.n):
                        raise ValueError('invalid exact codeword equality')
                    self.cache[key]=bool(value)
            cells=len(left)*len(right)
            self.stats.update(matrix_calls=1,matrix_cells=cells,
                              multiply_adds=cells*self.n,
                              batch_input_elements=(len(left)+len(right))*self.n,
                              speculative_matrix_cells=speculative,
                              already_known_matrix_cells=reused)
            self.blocks.append(dict(depth=depth,slots=slots,left_sources=left,right_sources=right,
                                    shape=[len(left),self.n,len(right)],requested=len(needed),
                                    matrix_cells=cells,speculative_cells=speculative,
                                    already_known_cells=reused))


def classify(candidate,engine):
    """None means unresolved, not false. Alternatives share one output binding."""
    engine.stats['state_classifications']+=1
    live=[]
    for branch in candidate['alternatives']:
        values=[engine.known(p,q) for _,p,q in branch]
        if any(v is False for v in values):
            continue
        if all(v is True for v in values):
            return 'complete',[]
        live.append(branch)
    return ('continue',live) if live else ('rejected',[])


def summary_states(states):
    return {name:sum(s==name for s in states) for name in ('complete','rejected','continue')}


def staged(library,prepared,cells,mode='all',eager=False,*,
           candidate_builder=candidates,engine_factory=EqualityBatch):
    ports,channels,context=batch_layout(library,prepared,cells,mode)
    jobs=candidate_builder(library,prepared,channels)
    engine=engine_factory(cells,library['width'])
    states,live=zip(*(classify(j,engine) for j in jobs))
    states,live=list(states),list(live)
    trace=[dict(depth='Bake guarantees',**summary_states(states),new_complete=states.count('complete'),
                new_rejected=states.count('rejected'),continued=states.count('continue'),
                work=dict(engine.stats),batches=0,branch_rows=sum(len(v) for v in live))]
    snapshots=[list(states)]
    levels=sorted({d for j in jobs for branch in j['alternatives'] for d,_,_ in branch})
    if eager:
        levels=['all depths']
    for depth in levels:
        previous=list(states)
        before=Counter(engine.stats)
        before_blocks=len(engine.blocks)
        requests=[]
        for state,branches in zip(states,live):
            if state=='continue':
                requests.extend((p,q) for branch in branches for d,p,q in branch
                                if eager or d==depth)
        engine.resolve(requests,depth)
        for i,state in enumerate(states):
            if state=='continue':
                # Old evidence is retained; completed/failed jobs never restart.
                states[i],live[i]=classify({'alternatives':live[i]},engine)
        trace.append(dict(depth=depth,**summary_states(states),
                          new_complete=sum(a=='continue' and b=='complete' for a,b in zip(previous,states)),
                          new_rejected=sum(a=='continue' and b=='rejected' for a,b in zip(previous,states)),
                          continued=states.count('continue'),branch_rows=sum(len(v) for v in live),
                          work=dict(Counter(engine.stats)-before),batches=len(engine.blocks)-before_blocks))
        snapshots.append(list(states))
    assert 'continue' not in states
    n=len(channels)
    gate=[[int(states[i*n+j]=='complete') for j in range(n)] for i in range(n)]
    aggregate=[[0]*len(ports) for _ in ports]
    for i,a in enumerate(channels):
        for j,b in enumerate(channels):
            aggregate[a['port']][b['port']]+=gate[i][j]
    return dict(ports=ports,channels=channels,gate=gate,aggregate=aggregate,
                context=context,trace=trace,stats=dict(engine.stats),blocks=engine.blocks,
                state_snapshots=snapshots,
                condition_alternatives=sum(len(j['alternatives']) for j in jobs))


def compare_legacy(library,cells,result):
    eager=tensor.score(library,cells)
    lookup={(c['instance'],c['local']):i for i,c in enumerate(eager['channels'])}
    selected=[lookup[c['instance'],c['local']] for c in result['channels']]
    assert result['gate']==[[eager['gate'][i][j] for j in selected] for i in selected]


def run(out,skip_build=False):
    out.mkdir(parents=True,exist_ok=False)
    if not skip_build:
        subprocess.run(['cargo','build','--offline','--release','--example','lhs_ast','--example','native_math_baseline'],cwd=ROOT,check=True)
    binary=ROOT/'target/release/examples/native_math_baseline'
    library=tensor.bake(out/'bake',binary)
    prepared=prepare(library,out/'bake')
    book=tensor.Codebook(tensor.ALPHABET)
    cells=[book.encode(t,b,library['templates'][t]['arity'],library['width']) for t,b in tensor.CASES]
    studies={}
    all_result=None
    for mode in ('root','all'):
        lazy=staged(library,prepared,cells,mode)
        eager=staged(library,prepared,cells,mode,eager=True)
        assert lazy['gate']==eager['gate']
        compare_legacy(library,cells,lazy)
        studies[mode]=dict(context=lazy['context'],channels=len(lazy['channels']),
                           condition_alternatives=lazy['condition_alternatives'],
                           trace=lazy['trace'],blocks=lazy['blocks'],
                           layered=lazy['stats'],eager=eager['stats'],
                           matches=sum(map(sum,lazy['gate'])),
                           status_snapshots=lazy['state_snapshots'])
        if mode=='all':
            all_result=lazy
    records=tensor.native_oracle(out,binary,library,cells,book,all_result)
    # Also assert that each emitted binding is already valid at its emission stage.
    for mode,study in studies.items():
        final=study['status_snapshots'][-1]
        for snapshot in study['status_snapshots']:
            assert all(s!='complete' or f=='complete' for s,f in zip(snapshot,final))
            assert all(s!='rejected' or f=='rejected' for s,f in zip(snapshot,final))
    report=dict(scope='CPU depth-frontier reference for the factoring boundary; not a GPU or native speed benchmark',
                depth_definition='depth inside the common-factor expression; graft Add and child Mul topology are already baked',
                batch_group='same code dimension and left/right relative input-slot layout',
                studies=studies,native_full_binding_equal=True,eager_equal=True,legacy_tensor_equal=True,
                matches=len(records),global_rule_bindings=json.loads((out/'native-validation.json').read_text())['global_rule_bindings'],
                complete_candidates_never_restarted=True,unknown_never_treated_as_false=True,
                candidate_budget=None,
                provenance=dict(head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip()))
    (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    (out/'prepared.json').write_text(json.dumps(prepared,indent=2)+'\n')
    (out/'interaction.json').write_text(json.dumps(dict(ports=all_result['ports'],channels=all_result['channels'],
        gate=all_result['gate'],aggregate=all_result['aggregate']),indent=2)+'\n')
    render(out,report)
    return report


def render(out,r):
    esc=html.escape
    def table(rows,headers):
        return '<table><tr>'+''.join('<th>'+esc(str(h))+'</th>' for h in headers)+'</tr>'+''.join('<tr>'+''.join('<td>'+esc(str(x))+'</td>' for x in row)+'</tr>' for row in rows)+'</table>'
    body='''<!doctype html><html lang="zh"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>逐层上下文匹配 · 完成 / 排除 / 继续</title><style>body{font:16px system-ui;line-height:1.7;color:#172d43;background:#f5f7fa;max-width:1120px;margin:30px auto;padding:0 20px}section{background:white;padding:22px;border:1px solid #dce4eb;border-radius:12px;margin:16px 0}pre{background:#eef3f7;padding:16px;overflow:auto;font-size:13px}table{border-collapse:collapse;width:100%;font-size:13px}td,th{border:1px solid #dce4eb;padding:8px}.warn{background:#fff1da;padding:16px}select{padding:8px}</style>
<h1>逐层捞取匹配：已经确定的退出，未确定的继续</h1>
<p>四条 math 规则 Bake 两个模板。本次仅实现因式提取边界的逐层运行时，使用八个向量实例；CPU 参考实现，没有 GPU 加速结论。</p>
<section><h2>1. 读什么上下文</h2><pre>新增 coarse 上下文：Add(left_port, right_port)
匹配：Add(Mul(k,u), Mul(k,v))

Bake：构造器拓扑、接口树形、端口接线已确定。
深度 0：k 是 Input 时，检查该 Input 的身份。
深度 1：读取 Add 直接连接的 Input 身份。
深度 2：仍有可能的候选，继续读取嵌套 Add 内部的 Input。
</pre><p>不是把全树形偷偷查一遍再分层：形状来自既有 Bake。运行时逐层取得的是新实例之间尚未确定的身份关系。未知不等于失败；同一对输出 binding 的交换排列可以共享和合并。</p></section>
'''
    body+='<section><h2>两个实际的继续／排除例子</h2><pre>I1: (a+b)+c  对  I6: (a+c)+b\n深度 1：外层输入 c 与 b 不同，立即排除，不再读取内部 Add。\n\nI3: (d+e)+f  对  I5: (e+d)+f\n深度 1：f 与 f 相同，但内部 Add 尚未确认，必须继续。\n深度 2：交换后的 d/e 对齐，完成匹配。</pre><p>以上描述的是 root 中的复合因子通道；同一个触点对的其他 binding 通道仍然独立保留。</p></section>'
    for mode,label in [('root','仅 root 触点'),('all','全部 root / 非 root 触点')]:
        s=r['studies'][mode]
        body+=f'<section><h2>2. {label}</h2><p>提供 {s["context"]["offered_contexts"]} 个 Add 上下文；Bake 证明缺少 Mul 接口而排除 {s["context"]["constructor_rejected"]} 个，剩 {s["context"]["constructor_continued"]} 个。之后 {s["channels"]} 个有向接口形成 {s["channels"]**2} 个完整 binding 通道候选。</p>'
        body+=table([[x['depth'],x['complete'],x['rejected'],x['continue'],x['new_complete'],x['new_rejected'],
                      x['work'].get('multiply_adds',0),x['batches']] for x in s['trace']],
                    ['深度','累计完成','累计排除','仍需继续','本层完成','本层排除','本层矩阵乘加','批次数'])
        body+='<p>上表的候选单位始终是完整 binding 通道；一个触点对中某个 binding 完成，不会取消其他不同 binding。</p>'
        keys=['condition_requests','missing_unique_conditions','matrix_calls','matrix_cells','multiply_adds','batch_input_elements','speculative_matrix_cells','already_known_matrix_cells','state_classifications','evidence_reads']
        body+=table([[k,s['eager'].get(k,0),s['layered'].get(k,0)] for k in keys],['实际工作量','一次读完全部深度','逐层继续'])
        body+='</section>'
    body+='''<section><h2>3. 同形批次与代价</h2><p>每一层把相同输入码维度、相同左右相对槽位布局的缺失条件放在一起，构造 L、R 并计算 LRᵀ。缓存以源向量坐标为键，不以实际 binding 值为键；不存在 value-based Join。</p><p>矩阵的交叉乘积可能额外计算本层没有要求的点。报告显式计入这些 speculative matrix cells，以及矩阵中重算的已知条件。提前排除不保证矩阵总工作量下降。</p><select id="mode"><option>仅 root</option><option>全部触点</option></select><pre id="batches"></pre></section>
'''
    body+=f'<section><h2>4. 验证与范围</h2><p>分层、一次性同算法以及上一版完整张量收缩的所有输出一致；{r["matches"]} 条带触点标签的 binding 与 native 一致，全局去重为 {r["global_rule_bindings"]} 条，原规则目标检查通过。native oracle 显式提供所有被测试的 Add 上下文；不能把它们叫做原输入里已经存在的匹配。</p><p class="warn">固定两个模板、16 元素域、实例内输入互异；没有动态 union 更新，没有候选预算或 Top-k。当前 frontier、分组和压缩由 Python 管理，矩阵为 CPU 整数实现，不宣称已成为纯 GPU 数据流。</p><p><a href="report.json">所有阶段与计数</a> · <a href="prepared.json">Bake 的上下文层级</a> · <a href="graft-check.egg">原生验证程序</a></p></section>'
    data=json.dumps({k:v['blocks'] for k,v in r['studies'].items()})
    body+='<script>const batches='+data+';function show(){let key=document.getElementById("mode").selectedIndex?"all":"root";document.getElementById("batches").textContent=JSON.stringify(batches[key],null,2);}document.getElementById("mode").onchange=show;show();</script></html>'
    (out/'index.html').write_text(body)


if __name__=='__main__':
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--output',type=Path,required=True)
    ap.add_argument('--skip-build',action='store_true')
    a=ap.parse_args()
    r=run(a.output.resolve(),a.skip_build)
    print(json.dumps({k:{q:v[q] for q in ('context','trace','layered','eager','matches')} for k,v in r['studies'].items()},indent=2))
