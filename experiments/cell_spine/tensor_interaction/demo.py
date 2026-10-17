"""Exact partner-independent Cell codes and tensor inner-product interaction.

Two finite baked templates; CPU integer reference, not a GPU performance claim.
"""
from collections import Counter
from dataclasses import dataclass
from itertools import product
import argparse
import html
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0,str(ROOT))
from experiments.cell_spine.factorized_boundary.demo import (
    Family, Graph, SEEDS, bind, ground_key, native, sexpr, template_inputs, mm, transpose)
from experiments.cell_spine.shared_matching.demo import Patterns, class_index, locations

HERE = Path(__file__).resolve().parent
RULES = HERE.parent/'factorized_boundary/rules.egg'
ALPHABET = ['x','a','b','c','z','y','d','e','f','g','h','i','j','k','l','m']
# Same types as the four earlier instances, plus swapped inputs and a wrong-bracketing case.
CASES = [(0,['x','a','b']), (1,['x','a','b','c']), (0,['z','a','b']),
         (1,['y','d','e','f']), (0,['x','b','a']), (1,['y','e','d','f']),
         (1,['z','a','c','b']), (0,['y','d','e'])]


def hadamard(n):
    if n < 1 or n & (n-1):
        raise ValueError('Hadamard dimension must be a power of two')
    h = [[1]]
    while len(h) < n:
        h = [r+r for r in h]+[r+[-x for x in r] for r in h]
    return h


@dataclass(frozen=True)
class EncodedCell:
    template: int
    ports: tuple  # Numeric tensor cores only; no input names or binding tuple.


class Codebook:
    def __init__(self, labels):
        if len(set(labels)) != len(labels):
            raise ValueError('duplicate alphabet entry')
        self.labels = tuple(labels)
        self.h = hadamard(len(labels))
        self.n = len(labels)

    def encode(self, template, binding, arity, width):
        if len(binding) != arity:
            raise ValueError('wrong number of input ports')
        if len(set(binding)) != len(binding):
            raise ValueError('aliased inputs require a different orbit normalization')
        rows = [tuple(self.h[self.labels.index(s)]) for s in binding]
        rows += [(0,)*self.n for _ in range(width-arity)]
        return EncodedCell(template,tuple(rows))

    def decode(self, cell, arity):
        # Fixed orthogonal decoding, not a constructor lookup or a match search.
        scores = mm([list(x) for x in cell.ports[:arity]],transpose(self.h))
        result = []
        for row in scores:
            if row.count(self.n) != 1 or any(x not in (0,self.n) for x in row):
                raise ValueError('invalid exact codeword')
            result.append(self.labels[row.index(self.n)])
        return result


def term_recipe(term):
    """Compile commutative Add symmetry, while preserving the binary tree shape.

    A recipe is a sum of Kronecker products of input code vectors. The local
    domain is Input or Add of DISTINCT input leaves; no associativity assumed.
    """
    if term[0] == 'Input':
        return dict(shape='I',components=[(term[1],)])
    if term[0] != 'Add':
        raise ValueError('no vector codec proved for this interface constructor')
    a,b = [term_recipe(x) for x in term[1:]]
    if a['shape'] > b['shape']:
        a,b = b,a
    components = [x+y for x,y in product(a['components'],b['components'])]
    if a['shape'] == b['shape']:
        components += [y+x for x,y in product(a['components'],b['components'])]
    if any(len(set(c)) != len(c) for c in components):
        raise ValueError('repeated leaves require specialization')
    if len(set(components)) != len(components):
        raise ValueError('duplicate orbit components')
    return dict(shape='A('+a['shape']+','+b['shape']+')',components=components)


def bake(out,binary,parser=None):
    out.mkdir(parents=True)
    subprocess.run([parser or ROOT/'target/release/examples/lhs_ast',RULES,out/'rules.json'],cwd=ROOT,check=True)
    ast = json.loads((out/'rules.json').read_text())
    patterns = Patterns.from_ast(ast)
    selected = [p for p in patterns.nodes if p['text']=='(Add (Mul v0 v1) (Mul v0 v2))']
    if len(selected) != 1:
        raise ValueError('experiment requires the factoring interaction')
    env = RULES.read_text().replace('(Var String)','(RipenInput i64)')
    templates,_ = template_inputs(SEEDS)
    family,roots = Family(),[]
    for i,t in enumerate(templates):
        native(binary,out/f't{i}',env,f'(let root {sexpr(t)})',constructors=['RipenInput','Add','Mul'])
        g = Graph(out/f't{i}/graph.json')
        roots.append(family.from_graph(g,g.value(t)))
    index = class_index(family,roots)
    descriptions = []
    recipes, recipe_index = [],{}
    for i,root in enumerate(roots):
        ports,channels = [],[]
        for anchor,path in sorted(locations(index,root).items()):
            projection = family.root_mul(anchor)
            if not projection:
                continue
            port = len(ports)
            ports.append(dict(anchor=anchor,term=family.representative(anchor),
                              location='root' if anchor==root else 'non-root',path=path))
            for (k,u),multiplicity in sorted(projection.items()):
                rec = term_recipe(k)
                key = (rec['shape'],tuple(rec['components']))
                if key not in recipe_index:
                    recipe_index[key] = len(recipes)
                    recipes.append(rec)
                channels.append(dict(port=port,k=k,u=u,recipe=recipe_index[key],
                                     local_spine_multiplicity=multiplicity))
        descriptions.append(dict(seed=templates[i],root=root,arity=3+i,
                                 candidate_count=family.count(root),ports=ports,channels=channels))
    # Every pairwise recipe is compiled here, with no concrete instance binding.
    contractions = []
    for a in recipes:
        row=[]
        for b in recipes:
            if a['shape'] != b['shape']:
                row.append(None)
            else:
                if len(a['components']) != len(b['components']):
                    raise ValueError('different orbit normalization')
                row.append(dict(terms=[list(zip(x,y)) for x,y in product(a['components'],b['components'])],
                                divisor=len(a['components'])))
        contractions.append(row)
    library=dict(templates=descriptions,recipes=recipes,contractions=contractions,width=4,
                 candidate_circuit=dict(nodes=family.nodes,roots=roots),
                 source_rule=next(r for r in patterns.rules if r['spine']==selected[0]['id']))
    (out/'library.json').write_text(json.dumps(library,indent=2)+'\n')
    return library


def layout(library,cells):
    """Structural batch layout only: no port values, comparisons or queries."""
    ports,channels = [],[]
    for instance,cell in enumerate(cells):
        t=library['templates'][cell.template]
        start=len(ports)
        ports.extend(dict(instance=instance,local=p,**port) for p,port in enumerate(t['ports']))
        channels.extend(dict(instance=instance,port=start+c['port'],recipe=c['recipe'],local=j)
                        for j,c in enumerate(t['channels']))
    return ports,channels


def score(library,cells):
    """The complete interaction kernel: vectors + precompiled numeric wiring.

    No Graph, AST, constructor index, tuple-key construction, matching, or codeword
    decoding is performed here. One Gram matrix feeds compressed tensor dots.
    """
    width=library['width']
    n=len(cells[0].ports[0])
    vectors=[list(v) for c in cells for v in c.ports]
    gram=mm(vectors,transpose(vectors))
    if any(x % n for row in gram for x in row):
        raise ValueError('invalid exact vector Gram')
    normalized=[[x//n for x in row] for row in gram]
    ports,channels=layout(library,cells)
    gate=[[0]*len(channels) for _ in channels]
    aggregate=[[0]*len(ports) for _ in ports]
    stats=Counter(gram_multiply_adds=len(vectors)**2*n,gram_calls=1,
                  tuple_keys_constructed=0,constructor_queries=0,spine_candidates_expanded=0)
    for i,a in enumerate(channels):
        for j,b in enumerate(channels):
            plan=library['contractions'][a['recipe']][b['recipe']]
            if plan is None:
                stats['static_shape_zero_pairs']+=1
                continue
            numerator=0
            for term in plan['terms']:
                value=1
                for x,y in term:
                    value*=normalized[a['instance']*width+x][b['instance']*width+y]
                    stats['contraction_scalar_products']+=1
                numerator+=value
                stats['contraction_terms']+=1
            if numerator % plan['divisor']:
                raise ValueError('orbit assumptions violated')
            value=numerator//plan['divisor']
            if value not in (0,1):
                raise ValueError('not a unique semantic connection')
            gate[i][j]=value
            aggregate[a['port']][b['port']]+=value
    return dict(ports=ports,channels=channels,gate=gate,aggregate=aggregate,stats=dict(stats))


def decode_records(library,cells,codebook,result):
    bindings=[codebook.decode(c,library['templates'][c.template]['arity']) for c in cells]
    def term_at(channel,field):
        instance=channel['instance']
        t=library['templates'][cells[instance].template]
        return bind(t['channels'][channel['local']][field],bindings[instance])
    records=[]
    for i,a in enumerate(result['channels']):
        for j,b in enumerate(result['channels']):
            if result['gate'][i][j]:
                # Output construction only. The match was decided numerically.
                k,u,v=term_at(a,'k'),term_at(a,'u'),term_at(b,'u')
                records.append(dict(left=a['port'],right=b['port'],channels=[i,j],
                                    k=k,u=u,v=v,target=('Mul',k,('Add',u,v))))
    return bindings,records


def kron(a,b):
    return [x*y for x in a for y in b]


def dense_feature(recipe,ports):
    """Validation only: materialize tensor coordinates, never full Spine candidates."""
    result=None
    for component in recipe['components']:
        vector=[1]
        for slot in component:
            vector=kron(vector,ports[slot])
        result=vector if result is None else [a+b for a,b in zip(result,vector)]
    return result


def validate_dense(library,cells,result):
    features=[dense_feature(library['recipes'][c['recipe']],cells[c['instance']].ports)
              for c in result['channels']]
    n=len(cells[0].ports[0])
    for i,a in enumerate(result['channels']):
        ar=library['recipes'][a['recipe']]
        for j,b in enumerate(result['channels']):
            br=library['recipes'][b['recipe']]
            expected=0
            if ar['shape']==br['shape']:
                value=sum(x*y for x,y in zip(features[i],features[j]))
                norm=len(ar['components'])*n**len(ar['components'][0])
                assert value % norm == 0
                expected=value//norm
            assert result['gate'][i][j]==expected
    return sum(map(len,features))


def native_oracle(out,binary,library,cells,codebook,result):
    bindings,records=decode_records(library,cells,codebook,result)
    source=RULES.read_text()+'\n'
    inputs='\n'.join(f'(let c{i} {sexpr(bind(library["templates"][c.template]["seed"],bindings[i]))})'
                     for i,c in enumerate(cells))
    port_lines='\n'.join(f'(Port {i} {sexpr(bind(p["term"],bindings[p["instance"]]))})'
                         for i,p in enumerate(result['ports']))
    port_terms=[sexpr(bind(p['term'],bindings[p['instance']])) for p in result['ports']]
    grafts='\n'.join(f'(Graft {i} {j} (Add {a} {b}))' for i,a in enumerate(port_terms) for j,b in enumerate(port_terms))
    probe='''
(datatype Match (Hit i64 i64 Math Math Math))
(relation Port (i64 Math))
(relation Graft (i64 i64 Math))
(ruleset probe)
(rule ((Port i left) (Port j right) (Graft i j root)
       (= root (Add left right)) (= left (Mul k u)) (= right (Mul k v)))
      ((Hit i j k u v)) :ruleset probe)
'''
    native(binary,out/'native',source+probe,inputs,
           port_lines+'\n'+grafts+'\n(run-schedule (run probe))',['Var','Add','Mul','Hit'])
    g=Graph(out/'native/graph.json')
    def label(t):
        return sexpr(ground_key(t))
    actual={(int(r['args'][0][1]),int(r['args'][1][1]),
             *(label(g.representative(v)) for _,v in r['args'][2:])) for r in g.rows if r['op']=='Hit'}
    expected={(r['left'],r['right'],label(r['k']),label(r['u']),label(r['v'])) for r in records}
    assert len(expected)==len(records), 'duplicate semantic matches'
    assert actual==expected, (len(actual),len(expected),list(actual-expected)[:3],list(expected-actual)[:3])
    # Port tags intentionally distinguish repeated instances. Also report the
    # actual untagged rule bindings so aliases do not inflate the result claim.
    unscoped=set()
    for r in records:
        ends=[]
        for key in ('left','right'):
            p=result['ports'][r[key]]
            ends.append(bind(p['term'],bindings[p['instance']]))
        unscoped.add((g.value(('Add',*ends)),g.value(r['k']),g.value(r['u']),g.value(r['v'])))
    (out/'native-validation.json').write_text(json.dumps(dict(
        port_scoped_bindings=len(actual),global_rule_bindings=len(unscoped)),indent=2)+'\n')
    checks='\n'.join(f'(check (= (Add {port_terms[r["left"]]} {port_terms[r["right"]]}) {sexpr(r["target"])}))' for r in records)
    executable=source+probe+inputs+'\n(run-schedule (saturate (run)))\n'+port_lines+'\n'+grafts+'\n(run 1)\n'+checks+'\n'
    (out/'graft-check.egg').write_text(executable)
    proc=subprocess.run([str(binary),str(out/'graft-check.egg')],cwd=ROOT,text=True,capture_output=True,timeout=60)
    (out/'graft-check.log').write_text(proc.stdout+proc.stderr)
    assert proc.returncode==0,proc.stderr
    return records


def run(out,skip_build=False):
    out.mkdir(parents=True,exist_ok=False)
    if not skip_build:
        subprocess.run(['cargo','build','--offline','--release','--example','lhs_ast','--example','native_math_baseline'],cwd=ROOT,check=True)
    binary=ROOT/'target/release/examples/native_math_baseline'
    library=bake(out/'bake',binary)
    book=Codebook(ALPHABET)
    cells=[book.encode(t,b,library['templates'][t]['arity'],library['width']) for t,b in CASES]
    result=score(library,cells)
    dense_entries=validate_dense(library,cells,result)
    records=native_oracle(out,binary,library,cells,book,result)
    # Roundtrip is independent of the interaction query: all input ports survive.
    assert [book.decode(c,library['templates'][c.template]['arity']) for c in cells]==[b for _,b in CASES]
    roots={p['instance']:i for i,p in enumerate(result['ports']) if p['location']=='root'}
    root_matrix=[[result['aggregate'][roots[i]][roots[j]] for j in range(len(cells))] for i in range(len(cells))]
    regions=Counter((result['ports'][r['left']]['location'],result['ports'][r['right']]['location']) for r in records)
    report=dict(scope='exact finite-domain TPR CPU prototype; numerical pairing only; no GPU speed or general union claim',
                alphabet=ALPHABET,dimension=book.n,template_count=len(library['templates']),instance_count=len(cells),
                cases=[dict(template=t,binding=b) for t,b in CASES],
                candidate_counts=[t['candidate_count'] for t in library['templates']],
                recipe_count=len(library['recipes']),
                encoded_numeric_entries=sum(len(v) for c in cells for v in c.ports),
                dense_validation_feature_entries=dense_entries,
                port_count=len(result['ports']),channel_count=len(result['channels']),
                kernel_stats=result['stats'],root_matrix=root_matrix,
                native_full_binding_equal=True,dense_tensor_equal=True,encoding_roundtrip=True,
                matches=len(records),matches_by_location={'/'.join(k):v for k,v in sorted(regions.items())},
                global_rule_bindings=json.loads((out/'native-validation.json').read_text())['global_rule_bindings'],
                original_rule_target_checks=len(records),
                preparation=dict(native_bakes=2,compiled_before_concrete_inputs=True,
                                 all_recipe_pairs=sum(len(r) for r in library['contractions']),
                                 codebook_entries=book.n**2,
                                 input_codewords_selected=sum(len(b) for _,b in CASES),
                                 padding_scalar_entries=sum((library['width']-len(b))*book.n for _,b in CASES)),
                provenance=dict(head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip()))
    (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    (out/'vectors.json').write_text(json.dumps([dict(template=c.template,ports=c.ports) for c in cells],indent=2)+'\n')
    (out/'interaction.json').write_text(json.dumps(dict(**result,records=[dict(r,k=sexpr(r['k']),u=sexpr(r['u']),v=sexpr(r['v']),target=sexpr(r['target'])) for r in records]),indent=2)+'\n')
    render(out,report,library,result,records)
    return report


def render(out,r,library,result,records):
    esc=html.escape
    def table(rows,headers):
        return '<table><tr>'+''.join('<th>'+esc(str(h))+'</th>' for h in headers)+'</tr>'+''.join('<tr>'+''.join('<td>'+esc(str(x))+'</td>' for x in row)+'</tr>' for row in rows)+'</table>'
    body='''<!doctype html><html lang="zh"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Spine 向量交互 · 精确 TPR</title><style>body{font:16px system-ui;line-height:1.7;color:#172d43;background:#f5f7fa;max-width:1120px;margin:30px auto;padding:0 20px}section{background:white;padding:22px;border:1px solid #dce4eb;border-radius:12px;margin:16px 0}pre{background:#eef3f7;padding:16px;overflow:auto;font-size:13px}table{border-collapse:collapse;width:100%;font-size:13px}td,th{border:1px solid #dce4eb;padding:9px;text-align:left}.warn{background:#fff1da;padding:16px}select{padding:8px}</style>
<h1>先编码整个 Cell，再直接计算向量交互</h1><p>有限域、精确整数 CPU 原型。配对阶段不查询 constructor、不构造 binding 键、不运行原匹配器。</p>
<section><h2>1. 完整表示，不是匹配之后生成的 one-hot</h2><pre>Z(T,b) = 模板块 T 中的 [Spine 候选向量 χT ⊗ h(b0) ⊗ … ⊗ h(bp)]
χT：共享的因子化 Spine 候选电路，Bake 一次。
h(a)：16 维 ±1 Hadamard 向量；h(a)·h(b) = 16 [a=b]。
每个实例只保存模板块编号和输入向量核；不保存另一份实际 binding 元组。
它是完整向量的因子化表示，不物化指数维坐标。
</pre><p>输入变量、节点角色与所有候选的结构由模板块和向量核共同确定；可以精确解码模板与全部输入。不是一个任意学习的相似度 embedding。</p>'''
    body+=table([[f'I{i}',f'T{t}',', '.join(b)] for i,(t,b) in enumerate(CASES)],['实例','模板','仅为阅读显示的原输入'])+'</section>'
    body+='''<section><h2>2. Bake 编译出的边界投影</h2><pre>Var(a)       → h(a)
Add(a,b)     → h(a)⊗h(b) + h(b)⊗h(a)
Add(Add(a,b),c) → [h(a)⊗h(b) + h(b)⊗h(a)] ⊗ h(c)
不同树形放在不同的正交结构通道；没有添加结合律。
</pre><p>上式末项对外层不同孩子形状使用固定角色顺序，因此外层 Add 交换不重复存储。相同形状孩子的交换通过张量叠加保留。配方由 root/non-root 的 Mul(k,u) 接口自动编译。</p>'''
    body+=table([[i,p['shape'],str(p['components'])] for i,p in enumerate(library['recipes'])],['配方','结构通道','张量积使用的输入位置'])+'</section>'
    body+='''<section><h2>3. 配对内核</h2><pre>一次矩阵乘法：G = Hports Hportsᵀ / 16
压缩张量点积：Σ(每对分量) Π(角色位置) G[左端口,右端口]
除以固定的对称轨道大小，得到精确 0/1。
不比较实际 ID，不查询事实表，不组装 (root,k) 键。
</pre><p>构造器查询被前移到 Bake。运行时所有比较都来自预编码向量及固定的投影配方；CPU 参考实现用循环调度数值收缩，不声称已经实现高效 GPU 内核。</p>'''
    body+=table([[k,v] for k,v in r['kernel_stats'].items()],['配对阶段实际计数','值'])+'</section>'
    body+='<section><h2>4. root 与内部触点都参与</h2><p>每次交互仍以外界提供 Add(左触点,右触点) 为前提。表中是 root/root 的不同 binding 数，非完整候选对计数。</p>'
    body+=table([[f'I{i}',*row] for i,row in enumerate(r['root_matrix'])],['左 / 右',*[f'I{i}' for i in range(len(CASES))]])
    body+='<p>I0 与 I4 只是交换输入，结果一致；I1 与 I6 有同样的 a,b,c 叶子，但括号不同且外部因子不同，结果为 0，不能把树形丢成集合。</p>'
    body+=table([[k,v] for k,v in r['matches_by_location'].items()],['位置组合','原生确认的 binding'])
    opts=''.join(f'<option>{i}: I{p["instance"]}/{p["location"]} {esc(sexpr(p["term"]))}</option>' for i,p in enumerate(result['ports']))
    body+=f'<p>左触点 <select id="left">{opts}</select> 右触点 <select id="right">{opts}</select></p><pre id="details"></pre></section>'
    body+=f'<section><h2>5. 验证与边界</h2><p>完整向量编码往返通过；压缩张量收缩与显式张量内积逐项一致；{r["matches"]} 个带触点对标识的 binding 与原生匹配一致，去掉触点标签后为 {r["global_rule_bindings"]} 个不同的原规则 binding。原始 rewrite 的目标等价检查全部通过。</p><p>实际保存 {r["encoded_numeric_entries"]} 个输入编码数值；显式边界张量仅用于验证，共 {r["dense_validation_feature_entries"]} 个数值。不是存储/RSS 优化结论。</p><p class="warn">仅两个 Bake 模板、有限 16 元素输入域、每个实例输入互异，且只支持本规则的 Var/Add 接口域。没有证明任意规则族都有低维编码；不支持运行时新 union 的增量更新。没有 GPU 加速结论。</p><p><a href="report.json">报告</a> · <a href="vectors.json">实际向量核</a> · <a href="bake/library.json">Bake 配方</a> · <a href="interaction.json">全部交互证据</a> · <a href="graft-check.egg">原始规则执行验证</a></p></section>'
    rs=[dict(left=x['left'],right=x['right'],k=sexpr(x['k']),u=sexpr(x['u']),v=sexpr(x['v']),target=sexpr(x['target'])) for x in records]
    root_ports={p['instance']:i for i,p in enumerate(result['ports']) if p['location']=='root'}
    body+='<script>const rows='+json.dumps(rs)+';function show(){let i=document.getElementById("left").selectedIndex,j=document.getElementById("right").selectedIndex;let found=rows.filter(r=>r.left===i&&r.right===j);document.getElementById("details").textContent=JSON.stringify({matches:found.length,bindings:found},null,2);}document.getElementById("left").onchange=show;document.getElementById("right").onchange=show;'
    body+=f'document.getElementById("left").selectedIndex={root_ports[0]};document.getElementById("right").selectedIndex={root_ports[1]};show();</script></html>'
    (out/'index.html').write_text(body)


if __name__=='__main__':
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--output',type=Path,required=True)
    ap.add_argument('--skip-build',action='store_true')
    a=ap.parse_args()
    r=run(a.output.resolve(),a.skip_build)
    print(json.dumps(r,indent=2))
