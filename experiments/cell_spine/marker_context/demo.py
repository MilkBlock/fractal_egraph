"""Shallow exact marker coordinates, incremental maintenance and full cost audit.

Vector-state prototype only. No new egglog sort or production runtime is added.
"""
from collections import Counter, defaultdict
from dataclasses import dataclass
from itertools import permutations
import argparse
import html
import json
from pathlib import Path
import subprocess
import sys
from types import SimpleNamespace

ROOT=Path(__file__).resolve().parents[3]
sys.path.insert(0,str(ROOT))
from experiments.cell_spine.layered_context import demo as layers
from experiments.cell_spine.tensor_interaction import demo as tensor
from experiments.cell_spine.factorized_boundary.demo import mm, transpose, native, sexpr
from tools.bench.math_graph_equal import verify

HERE=Path(__file__).resolve().parent


def leaves(t):
    return (t[1],) if t[0]=='Input' else leaves(t[1])+leaves(t[2])


@dataclass(frozen=True)
class MarkerCell:
    template: int
    ports: tuple


class MarkerPlan:
    """Discover marker supports in baked interface terms, not concrete data."""
    def __init__(self,library,policy):
        if policy not in ('pair','whole'):
            raise ValueError(policy)
        self.library,self.policy=library,policy
        self.base_width=library['width']
        self.specs,self.index,self.descriptors=[],{},{}
        def mark(t):
            if t[0]=='Input':
                return t[1]
            if t[0]!='Add':
                raise ValueError('only certified Add interfaces')
            children=[mark(c) for c in t[1:]]
            support=tuple(sorted(leaves(t)))
            if len(set(support))!=len(support):
                raise ValueError('aliased marker support')
            if support not in self.index:
                slot=self.base_width+len(self.specs)
                self.index[support]=slot
                self.specs.append(dict(slot=slot,support=support,children=children,
                    permutations=list(permutations(support))))
            return self.index[support]
        for tid,t in enumerate(library['templates']):
            for local,ch in enumerate(t['channels']):
                k=ch['k']
                if k[0]=='Input':
                    descriptor=[(0,k[1])]
                elif all(c[0]=='Input' for c in k[1:]):
                    descriptor=[(0,mark(k))]
                else:
                    nested=[c for c in k[1:] if c[0]=='Add']
                    outer=[c for c in k[1:] if c[0]=='Input']
                    if len(nested)!=1 or len(outer)!=1 or len(leaves(nested[0]))!=2:
                        raise ValueError('need another marker certificate for this shape')
                    # Whole leaf set alone is insufficient: preserve outer role.
                    descriptor=[(0,mark(k if policy=='whole' else nested[0])),(1,outer[0][1])]
                self.descriptors[tid,local]=descriptor
        self.width=self.base_width+len(self.specs)
        self.weights=[1]*self.base_width+[len(s['support']) for s in self.specs]

    def build(self,base):
        n=len(base.ports[0])
        data=list(base.ports[:self.base_width])
        stats=Counter()
        arity=self.library['templates'][base.template]['arity']
        for spec in self.specs:
            if any(i>=arity for i in spec['support']):
                data.append((0,)*n)
                stats['padding_entries']+=n
            else:
                a,b=[data[i] for i in spec['children']]
                data.append(tuple(x+y for x,y in zip(a,b)))
                stats['marker_scalar_adds']+=n
                stats['marker_rows_built']+=1
                stats['marker_entries_built']+=n
        stats['stored_extra_entries']=len(self.specs)*n
        return MarkerCell(base.template,tuple(data)),dict(stats)

    def update(self,old,target,updates):
        """Explicit input-graft/substitution event. No structure search or Join.

        The unchanged prefix is retained by construction, not rediscovered.
        This is not an arbitrary union handler.
        """
        arity=self.library['templates'][target]['arity']
        previous_arity=self.library['templates'][old.template]['arity']
        if arity<previous_arity or any(i<0 or i>=arity for i in updates):
            raise ValueError('unsupported input mapping')
        if any(i not in updates for i in range(previous_arity,arity)):
            raise ValueError('new input needs an explicit code')
        data=list(old.ports[:self.base_width])
        n=len(data[0])
        for i,v in updates.items():
            # Validate a Sylvester-Hadamard code without an identity-table query.
            if len(v)!=n:
                raise ValueError('invalid input code dimension')
            row=sum((int(v[1<<bit])==-1)<<bit for bit in range(n.bit_length()-1))
            expected=tuple(1 if (row&j).bit_count()%2==0 else -1 for j in range(n))
            if tuple(v)!=expected:
                raise ValueError('invalid input codeword')
            data[i]=tuple(v)
        if len(set(data[:arity]))!=arity:
            raise ValueError('aliased inputs need new Cell analysis')
        stats=Counter(input_events=len(updates),input_code_component_checks=n*len(updates))
        for spec in self.specs:
            available=all(i<arity for i in spec['support'])
            old_available=all(i<previous_arity for i in spec['support'])
            if available and old_available and not set(spec['support']).intersection(updates):
                data.append(old.ports[spec['slot']])
                stats['marker_rows_reused']+=1
            elif available:
                a,b=[data[i] for i in spec['children']]
                data.append(tuple(x+y for x,y in zip(a,b)))
                stats['marker_scalar_adds']+=len(a)
                stats['marker_rows_rebuilt']+=1
            else:
                data.append(old.ports[spec['slot']])
        return MarkerCell(target,tuple(data)),dict(stats)

    def candidates(self,library,prepared,channels):
        result=[]
        for i,a in enumerate(channels):
            for j,b in enumerate(channels):
                alternatives=()
                if library['contractions'][a['recipe']][b['recipe']] is not None:
                    da=self.descriptors[prepared['cell_types'][a['instance']],a['local']]
                    db=self.descriptors[prepared['cell_types'][b['instance']],b['local']]
                    assert len(da)==len(db)
                    conditions=[]
                    for (depth,x),(other_depth,y) in zip(da,db):
                        assert depth==other_depth
                        p,q=sorted((a['instance']*self.width+x,b['instance']*self.width+y))
                        conditions.append((depth,p,q))
                    alternatives=(tuple(sorted(set(conditions))),)
                result.append(dict(left=i,right=j,alternatives=alternatives))
        return result


class MarkerBatch(layers.EqualityBatch):
    def __init__(self,cells,width,plan,infer=True):
        super().__init__(cells,width)
        self.plan,self.infer=plan,infer
        self.types=[c.template for c in cells]
        self.specs={s['slot']:s for s in plan.specs}

    def known(self,p,q):
        raw=super().known(p,q)
        if raw is not None or not self.infer:
            return raw
        sp,sq=p%self.width,q%self.width
        if sp not in self.specs or sq not in self.specs:
            return None
        a,b=self.specs[sp],self.specs[sq]
        if len(a['support'])!=len(b['support']):
            return False
        self.stats['marker_inference_attempts']+=1
        unknown=False
        for perm in b['permutations']:
            values=[]
            for x,y in zip(a['support'],perm):
                u,v=sorted((p//self.width*self.width+x,q//self.width*self.width+y))
                values.append(super().known(u,v))
                self.stats['marker_inference_literal_reads']+=1
            if any(v is False for v in values):
                continue
            if all(v is True for v in values):
                self.cache[p,q]=True
                self.stats['marker_equalities_inferred']+=1
                return True
            unknown=True
        if not unknown:
            self.cache[p,q]=False
            self.stats['marker_equalities_inferred']+=1
            return False
        return None

    def resolve(self,requests,depth):
        self.stats['condition_requests']+=len(requests)
        groups=defaultdict(set)
        missing=set()
        for p,q in requests:
            if self.known(p,q) is not None:
                self.stats['known_condition_requests']+=1
                continue
            missing.add((p,q))
            a,b=p%self.width,q%self.width
            size=self.plan.weights[a]
            assert size==self.plan.weights[b]
            # Primitive equality first. Marker batches can reuse its evidence.
            context=() if size==1 else (self.types[p//self.width],self.types[q//self.width])
            groups[size,a,b,context].add((p,q))
        self.stats['missing_unique_conditions']+=len(missing)
        for (size,a,b,context),group in sorted(groups.items()):
            needed={(p,q) for p,q in group if self.known(p,q) is None}
            if not needed:
                self.stats['batches_eliminated_by_existing_evidence']+=1
                continue
            left=sorted({p for p,_ in needed})
            right=sorted({q for _,q in needed})
            products=mm([self.vectors[p] for p in left],transpose([self.vectors[q] for q in right]))
            reused=speculative=0
            for i,p in enumerate(left):
                for j,q in enumerate(right):
                    key=tuple(sorted((p,q)))
                    if self.known(*key) is not None:
                        reused+=1
                    if key not in needed:
                        speculative+=1
                    value=products[i][j]
                    if value<0 or value>self.n*size or value%self.n:
                        raise ValueError('invalid exact leaf-set marker')
                    self.cache[key]=(value==self.n*size)
            count=len(left)*len(right)
            self.stats.update(matrix_calls=1,matrix_cells=count,multiply_adds=count*self.n,
                batch_input_elements=(len(left)+len(right))*self.n,
                speculative_matrix_cells=speculative,already_known_matrix_cells=reused)
            self.blocks.append(dict(depth=depth,slots=[a,b],marker_size=size,context=context,
                shape=[len(left),self.n,len(right)],requested=len(needed),matrix_cells=count,
                speculative_cells=speculative,already_known_cells=reused))


def marker_score(library,prepared,cells,plan,mode):
    local=dict(library,width=plan.width)
    p=dict(prepared,cell_types=[c.template for c in cells])
    return layers.staged(local,p,cells,mode,
        candidate_builder=plan.candidates,
        engine_factory=lambda c,w:MarkerBatch(c,w,plan))


def maintenance(plan,book):
    source=book.encode(0,['x','a','b'],3,4)
    old,old_cost=plan.build(source)
    new_code=tuple(book.h[book.labels.index('c')])
    grown,delta=plan.update(old,1,{3:new_code})
    fresh,cold=plan.build(book.encode(1,['x','a','b','c'],4,4))
    assert grown==fresh
    for field in range(3):
        assert grown.ports[field] is old.ports[field]
    assert grown.ports[4] is old.ports[4]
    replacements=[]
    for slot,value in [(0,'z'),(3,'d'),(1,'e')]:
        code=tuple(book.h[book.labels.index(value)])
        changed,cost=plan.update(grown,1,{slot:code})
        binding=['x','a','b','c']
        binding[slot]=value
        expected,_=plan.build(book.encode(1,binding,4,4))
        assert changed==expected
        replacements.append(dict(slot=slot,value=value,cost=cost))
    return dict(source_build=old_cost,fresh_target_build=cold,append_update=delta,
                append_equal=True,substitutions=replacements)


def run(out,skip_build=False):
    out.mkdir(parents=True,exist_ok=False)
    if not skip_build:
        subprocess.run(['cargo','build','--offline','--release','--example','lhs_ast','--example','native_math_baseline'],cwd=ROOT,check=True)
    binary=ROOT/'target/release/examples/native_math_baseline'
    library=tensor.bake(out/'bake',binary)
    prepared=layers.prepare(library,out/'bake')
    book=tensor.Codebook(tensor.ALPHABET)
    base=[book.encode(t,b,library['templates'][t]['arity'],4) for t,b in tensor.CASES]
    legacy={m:layers.staged(library,prepared,base,m) for m in ('root','all')}
    # Isolate marker effects from the new scheduler's between-batch cache refresh.
    atom_plan=SimpleNamespace(specs=[],weights=[1]*library['width'])
    baseline={m:layers.staged(library,prepared,base,m,
        engine_factory=lambda c,w:MarkerBatch(c,w,atom_plan,infer=False)) for m in ('root','all')}
    for mode in baseline:
        assert baseline[mode]['gate']==legacy[mode]['gate']
    policies={}
    for policy in ('pair','whole'):
        plan=MarkerPlan(library,policy)
        cells,construction=[],Counter()
        for c in base:
            marked,work=plan.build(c)
            cells.append(marked)
            construction.update(work)
        modes={}
        for mode in ('root','all'):
            result=marker_score(library,prepared,cells,plan,mode)
            assert result['gate']==baseline[mode]['gate']
            modes[mode]={k:result[k] for k in ('stats','trace','blocks','state_snapshots')}
        policies[policy]=dict(marker_specs=plan.specs,width=plan.width,
                             construction=dict(construction),modes=modes,
                             maintenance=maintenance(plan,book))
    records=tensor.native_oracle(out,binary,library,base,book,baseline['all'])
    # The specific T0 -> T1 graft is a real closed-state extension, not just a
    # numeric buffer append. Compare complete native constructor/equality state.
    env=tensor.RULES.read_text()
    source=library['templates'][0]['seed']
    target=library['templates'][1]['seed']
    assert target[0]=='Add' and target[1]==source
    inputs=['x','a','b','c']
    native(binary,out/'append-native',env,'(let old '+sexpr(tensor.bind(source,inputs))+')',
           '(let next '+sexpr(tensor.bind(target,inputs))+')\n(run-schedule (saturate (run)))')
    native(binary,out/'fresh-native',env,'(let next '+sexpr(tensor.bind(target,inputs))+')')
    closure=verify(out/'append-native/graph.json',out/'fresh-native/graph.json')
    report=dict(scope='CPU shallow-marker vector prototype; no production egglog sorts, no GPU timing',
        baseline={m:{k:r[k] for k in ('stats','trace')} for m,r in baseline.items()},
        legacy_baseline={m:{k:r[k] for k in ('stats','trace')} for m,r in legacy.items()},
        policies=policies,native_full_binding_equal=True,matches=len(records),
        global_rule_bindings=json.loads((out/'native-validation.json').read_text())['global_rule_bindings'],
        append_native_closure=closure,
        cost_scope='matching MACs and marker scalar additions reported separately; no wall-time claim',
        provenance=dict(head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip()))
    (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    render(out,report)
    return report


def render(out,r):
    esc=html.escape
    def table(rows,headers):
        return '<table><tr>'+''.join('<th>'+esc(str(h))+'</th>' for h in headers)+'</tr>'+''.join('<tr>'+''.join('<td>'+esc(str(x))+'</td>' for x in row)+'</tr>' for row in rows)+'</table>'
    body='''<!doctype html><html lang="zh"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>浅层 Marker · 匹配与维护成本</title><style>body{font:16px system-ui;line-height:1.7;color:#172d43;background:#f5f7fa;max-width:1120px;margin:30px auto;padding:0 20px}section{background:white;padding:22px;border:1px solid #dce4eb;border-radius:12px;margin:16px 0}pre{background:#eef3f7;padding:16px;overflow:auto;font-size:13px}table{border-collapse:collapse;width:100%;font-size:13px}td,th{border:1px solid #dce4eb;padding:8px}.warn{background:#fff1da;padding:16px}select{padding:8px}</style>
<h1>将内部结构暴露为浅层 marker</h1><p>同一批向量实例与因式提取边界。只增加数值 marker 通道，不重新查询内部结构；独立 CPU 原型，不是生产 egglog 新 sort。</p>
<section><h2>1. 两种 marker</h2><pre>Pair marker：M12 = h(p1) + h(p2)
Whole marker：M123 = M12 + h(p3)

两个同长度 marker 的点积 = 16 × 相同叶子的数量。
Pair 相同足以证明 p1+p2 等价。
Whole 相同只证明叶子集合相同，仍需外层 p3 相同。
</pre><p class="warn">(a+b)+c 与 (a+c)+b 的 Whole marker 相同，但结构不同。实现保留外层角色检查，不会提前宣布成功。</p></section>'''
    for mode,label in [('root','仅 root 触点'),('all','全部内部触点')]:
        rows=[]
        legacy=r['legacy_baseline'][mode]['stats']
        baseline=r['baseline'][mode]['stats']
        rows.append(['上一版逐层',legacy.get('multiply_adds',0),legacy.get('matrix_calls',0),0,0])
        rows.append(['相同调度，无 marker',baseline.get('multiply_adds',0),baseline.get('matrix_calls',0),0,0])
        for name,p in r['policies'].items():
            s=p['modes'][mode]['stats']
            rows.append([name if name=='pair' else 'whole（优先读全叶 marker）',s.get('multiply_adds',0),s.get('matrix_calls',0),
                         p['construction'].get('marker_scalar_adds',0),p['construction']['stored_extra_entries']])
        body+=f'<section><h2>2. {label}：完整成本对照</h2>'+table(rows,['方案','匹配矩阵乘加','矩阵批次','冷启动 marker 加法','新增数值槽位'])
        body+='<p>乘加与普通加法单独列出，不作为同等运行时间相加。已有 marker 可复用时不重复支付构造；首次创建必须支付。槽位包括 padding，不是 RSS。</p><p>Whole 状态仍保留 Pair marker，只是该策略优先读取全叶集合；结果较差不表示更多信息降低能力，而表示这批输入下该读取顺序不划算。</p></section>'
    body+='''<section><h2>3. 增量维护</h2><pre>T0 = p0*(p1+p2)
T1 = T0 + p0*p3

T0 → T1：输入 p0,p1,p2 与 M12 保留；只引入 p3。
Pair 方案没有 marker 新计算。
Whole 方案只计算 M123 = 旧 M12 + 新 h(p3)。</pre>'''
    body+=table([[name,p['maintenance']['fresh_target_build'].get('marker_scalar_adds',0),
                  p['maintenance']['append_update'].get('marker_scalar_adds',0),
                  p['maintenance']['append_update'].get('marker_rows_reused',0)] for name,p in r['policies'].items()],
                ['方案','T1 从头构造加法','从 T0 接续加法','直接保留的 marker 行'])
    body+='<p>保持数值相等之外，测试还检查旧 M12 对象直接复用。该 graft 的原生完整闭包与从头构造 T1 一致。</p></section>'
    body+='<section><h2>4. 不浪费已有浅层证据</h2><p>如果原 Input 比较已经证明 marker 的叶子集合相同或不同，就直接复用证据，不再执行 marker 矩阵。推理的布尔检查也计数，不能当作免费。</p>'
    body+=table([[name,mode,p['modes'][mode]['stats'].get('marker_inference_attempts',0),
                  p['modes'][mode]['stats'].get('marker_inference_literal_reads',0),
                  p['modes'][mode]['stats'].get('marker_equalities_inferred',0)]
                 for name,p in r['policies'].items() for mode in ('root','all')],
                ['方案','批次','marker 推断尝试','叶子证据读取','直接推出的 marker 条件'])
    body+='<p>同形 marker 按支持大小、相对槽位和模板对分组；原子条件先计算。批次中的额外矩阵位置和证据读取都在完整报告中。</p></section>'
    body+='<section><h2>5. 逐层状态</h2><select id="policy"><option>pair</option><option>whole</option></select> <select id="mode"><option>root</option><option>all</option></select><pre id="trace"></pre></section>'
    body+=f'<section><h2>6. 验证与限制</h2><p>全部 marker 方案与原逐层输出一致；{r["matches"]} 条带触点标识的完整 binding 与原生一致，全局去重 {r["global_rule_bindings"]} 条。增量更新和从头构造的 marker 数值逐项相同。</p><p class="warn">只支持这两个模板、输入互异的有限域及这一条边界规则。参数替换／已知 graft 的 marker 维护已验证，不等于支持任意 union。frontier 和调度仍由 Python 管理，没有 GPU 或墙钟加速结论。</p><p><a href="report.json">所有成本与阶段</a> · <a href="graft-check.egg">原生边界验证</a></p></section>'
    data=json.dumps({k:{m:p['modes'][m]['trace'] for m in ('root','all')} for k,p in r['policies'].items()})
    body+='<script>const traces='+data+';function show(){let p=document.getElementById("policy").selectedIndex?"whole":"pair",m=document.getElementById("mode").selectedIndex?"all":"root";document.getElementById("trace").textContent=JSON.stringify(traces[p][m],null,2);}document.getElementById("policy").onchange=show;document.getElementById("mode").onchange=show;show();</script></html>'
    (out/'index.html').write_text(body)


if __name__=='__main__':
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--output',type=Path,required=True)
    ap.add_argument('--skip-build',action='store_true')
    a=ap.parse_args()
    r=run(a.output.resolve(),a.skip_build)
    print(json.dumps({name:dict(construction=p['construction'],maintenance=p['maintenance'],
        modes={m:p['modes'][m]['stats'] for m in ('root','all')}) for name,p in r['policies'].items()},indent=2))
