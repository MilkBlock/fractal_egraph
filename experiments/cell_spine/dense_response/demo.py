"""Measure semantic densification, not density created by changing vector basis."""
from collections import defaultdict
from fractions import Fraction
import argparse
import html
import json
from pathlib import Path
import subprocess
import sys

ROOT=Path(__file__).resolve().parents[3]
sys.path.insert(0,str(ROOT))
from experiments.cell_spine.factorized_boundary.demo import Graph, bind, native, sexpr, mm
from experiments.cell_spine.shared_matching import demo as shared
from experiments.cell_spine.tensor_interaction import demo as tensor

HERE=Path(__file__).resolve().parent


def exact_factor(matrix):
    """Rational column rank factorization M = C R; never a numerical tolerance."""
    m,n=len(matrix),len(matrix[0]) if matrix else 0
    reduced=[[Fraction(x) for x in row] for row in matrix]
    pivots=[]
    for col in range(n):
        r=len(pivots)
        pivot=next((i for i in range(r,m) if reduced[i][col]),None)
        if pivot is None:
            continue
        reduced[r],reduced[pivot]=reduced[pivot],reduced[r]
        scale=reduced[r][col]
        reduced[r]=[x/scale for x in reduced[r]]
        for i in range(m):
            if i != r and reduced[i][col]:
                scale=reduced[i][col]
                reduced[i]=[x-scale*y for x,y in zip(reduced[i],reduced[r])]
        pivots.append(col)
        if len(pivots)==m:
            break
    c=[[row[j] for j in pivots] for row in matrix]
    r=reduced[:len(pivots)]
    reconstructed=mm(c,r) if pivots else [[0]*n for _ in range(m)]
    assert reconstructed==matrix
    return dict(rank=len(pivots),pivots=pivots,C=c,R=r)


def apply_response(factor, queries, rows):
    if not factor['rank']:
        return [[0]*len(queries[0]) for _ in range(rows)]
    return mm(factor['C'],mm(factor['R'],queries))


def stats(matrix):
    m,n=len(matrix),len(matrix[0]) if matrix else 0
    nnz=sum(x!=0 for row in matrix for x in row)
    rank=exact_factor(matrix)['rank']
    unique={tuple(row) for row in matrix if any(row)}
    return dict(shape=[m,n],nonzeros=nnz,density=nnz/(m*n) if m*n else 0,
                exact_rank=rank,nonzero_distinct_rows=len(unique),
                dense_multiply_adds=m*n,rank_factor_multiply_adds=m*rank+rank*n,
                shared_row_dot_multiply_adds=len(unique)*n,
                shared_row_nonzero_multiply_adds=sum(sum(x!=0 for x in row) for row in unique),
                nonzero_multiply_adds=nnz)


def trim(matrix):
    if not matrix:
        return [],[],[]
    rows=[i for i,row in enumerate(matrix) if any(row)]
    cols=[j for j in range(len(matrix[0])) if any(row[j] for row in matrix)]
    return [[matrix[i][j] for j in cols] for i in rows],rows,cols


def response_operators(report, directory):
    """Every single-bound-variable mode: B[(rule,slot,anchor),value].

    All other variables are existentially projected out. These are individual
    query modes, NOT independently combinable constraints for a full match.
    """
    operators=[]
    for cell in range(len(report['templates'])):
        requests=[r for r in report['results'] if r['cell']==cell]
        domain=sorted({r['anchor'] for r in requests})
        row_keys=[]
        matrix=[]
        for request in requests:
            p=report['patterns'][request['spine']]
            for slot in range(p['variables']):
                values={b[slot+1] for b in request['bindings']}
                row_keys.append([request['spine'],slot,request['anchor']])
                matrix.append([int(v in values) for v in domain])
        # Independent original-LHS native output, projected only for this query.
        graph=Graph(directory/f'native/t{cell}/graph.json')
        label_to_anchor={v:int(k) for k,v in report['labels'].items()}
        native_values=defaultdict(set)
        for row in graph.rows:
            if not row['op'].startswith('Hit'):
                continue
            pid=int(row['op'][3:])
            values=[label_to_anchor[sexpr(graph.representative(v))] for _,v in row['args']]
            for slot,value in enumerate(values[1:]):
                native_values[pid,slot,values[0]].add(value)
        expected=[[int(v in native_values[tuple(key)]) for v in domain] for key in row_keys]
        assert matrix==expected
        active,rs,cs=trim(matrix)
        factor=exact_factor(matrix)
        # Equality of C R and B already proves every rational query vector;
        # also exercise the runtime with zero, one selected value, and all values.
        query=[[0,int(i==0),1] for i in range(len(domain))]
        direct=mm(matrix,query)
        compressed=apply_response(factor,query,len(matrix))
        assert direct==compressed
        assert [[int(x>0) for x in row] for row in direct]==[[int(x>0) for x in row] for row in compressed]
        operators.append(dict(cell=cell,columns=domain,column_labels=[report['labels'][str(v)] for v in domain],
                              rows=row_keys,matrix=matrix,raw=stats(matrix),active=stats(active),
                              active_matrix=active,active_rows=rs,active_columns=cs,
                              factor=dict(rank=factor['rank'],pivots=factor['pivots'],C=factor['C'],
                                          R=[[str(x) for x in row] for row in factor['R']]),
                              native_equal=True,batch_query_equal=True))
    return operators


def interaction_blocks(out, library, vectors, interaction):
    """Separate possible grafts from actual Add contexts in the unmodified graph."""
    book=tensor.Codebook(tensor.ALPHABET)
    cells=[tensor.EncodedCell(c['template'],tuple(tuple(v) for v in c['ports'])) for c in vectors]
    bindings=[book.decode(c,library['templates'][c.template]['arity']) for c in cells]
    inputs='\n'.join(f'(let c{i} {sexpr(bind(library["templates"][c.template]["seed"],bindings[i]))})'
                     for i,c in enumerate(cells))
    # No Graft or synthetic Add facts are inserted in this oracle.
    original=library['source_rule']
    probe=f'''\n(datatype Match (Hit Math Math Math Math))
(ruleset probe)
(rule ((= anchor {original['lhs']}))
      ((Hit anchor {' '.join(original['variables'])})) :ruleset probe)
'''
    binary=ROOT/'target/release/examples/native_math_baseline'
    native(binary,out/'existing-only',tensor.RULES.read_text()+probe,inputs,
           '(run-schedule (run probe))',['Var','Add','Mul','Hit'])
    graph=Graph(out/'existing-only/graph.json')
    keys=[]
    for c in interaction['channels']:
        instance=c['instance']
        t=library['templates'][cells[instance].template]
        channel=t['channels'][c['local']]
        port=interaction['ports'][c['port']]
        keys.append(tuple(graph.value(bind(expr,bindings[instance])) for expr in
                          (port['term'],channel['k'],channel['u'])))
    # Deduplicate actual canonical constructor facts, not just instance labels.
    unique=sorted(set(keys))
    positions={key:[] for key in unique}
    for i,key in enumerate(keys):
        positions[key].append(i)
    representatives=[positions[key][0] for key in unique]
    raw=interaction['gate']
    potential=[[raw[i][j] for j in representatives] for i in representatives]
    for i,a in enumerate(keys):
        for j,b in enumerate(keys):
            assert raw[i][j]==int(a[1]==b[1])
    context=[[int(('Add',(a[0],b[0])) in graph.lookup) for b in unique] for a in unique]
    existing=[[p*e for p,e in zip(pr,er)] for pr,er in zip(potential,context)]
    predicted=set()
    for i,a in enumerate(unique):
        for j,b in enumerate(unique):
            if existing[i][j]:
                root=graph.lookup['Add',(a[0],b[0])]
                predicted.add((root,a[1],a[2],b[2]))
    observed={tuple(v for _,v in row['args']) for row in graph.rows if row['op']=='Hit'}
    assert predicted==observed
    assert len(predicted)==sum(map(sum,existing))
    groups=defaultdict(list)
    for i,key in enumerate(unique):
        groups[key[1]].append(i)
    blocks=[]
    for key,indices in groups.items():
        p=[[potential[i][j] for j in indices] for i in indices]
        e=[[existing[i][j] for j in indices] for i in indices]
        assert all(x==1 for row in p for x in row)
        blocks.append(dict(factor=sexpr(graph.representative(key)),indices=indices,
                           potential=stats(p),existing=stats(e),existing_matrix=e,
                           members=[sexpr(graph.representative(unique[i][0])) for i in indices]))
    blocks.sort(key=lambda b:(-len(b['indices']),b['factor']))
    assert sum(b['potential']['nonzeros'] for b in blocks)==sum(map(sum,potential))
    active,rs,cs=trim(existing)
    (out/'existing-probe.egg').write_text(tensor.RULES.read_text()+probe+inputs+
                                        '\n(run-schedule (saturate (run)))\n(run-schedule (run probe))\n')
    return dict(raw_channels=len(keys),unique_channels=len(unique),raw_potential=stats(raw),
                potential=stats(potential),existing=stats(existing),active_existing=stats(active),
                active_rows=rs,active_columns=cs,canonical_facts=[list(k) for k in unique],
                potential_matrix=potential,existing_matrix=existing,context_matrix=context,blocks=blocks,
                native_existing_matches=len(observed),native_equal=True,
                synthetic_graft_facts_added=0)


def run(out,skip_build=False):
    out.mkdir(parents=True,exist_ok=False)
    if not skip_build:
        subprocess.run(['cargo','build','--offline','--release','--example','lhs_ast','--example','native_math_baseline'],cwd=ROOT,check=True)
    matching=shared.run(out/'matching',skip_build=True)
    tensor.run(out/'tensor',skip_build=True)
    operators=response_operators(matching,out/'matching')
    library=json.loads((out/'tensor/bake/library.json').read_text())
    vectors=json.loads((out/'tensor/vectors.json').read_text())
    interaction=json.loads((out/'tensor/interaction.json').read_text())
    blocks=interaction_blocks(out,library,vectors,interaction)
    report=dict(scope='semantic density and exact rational rank audit; CPU/native correctness; not a GPU benchmark',
                response_query='fix one LHS variable, existentially remove the others, return matching anchors',
                responses=operators,interaction=blocks,
                outcome='potential complete blocks are rank one; existing-context and query-response matrices are not rich dense operators',
                provenance=dict(head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip()))
    (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    render(out,report)
    return report


def render(out,r):
    esc=html.escape
    def table(rows,headers):
        return '<table><tr>'+''.join('<th>'+esc(str(h))+'</th>' for h in headers)+'</tr>'+''.join('<tr>'+''.join('<td>'+esc(str(v))+'</td>' for v in row)+'</tr>' for row in rows)+'</table>'
    def values(s):
        return ['×'.join(map(str,s['shape'])),s['nonzeros'],f'{100*s["density"]:.2f}%',s['exact_rank']]
    i=r['interaction']
    body='''<!doctype html><html lang="zh"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Cell 接口：真实稠密性与精确秩</title><style>body{font:16px system-ui;line-height:1.7;color:#172d43;background:#f5f7fa;max-width:1120px;margin:30px auto;padding:0 20px}section{background:white;padding:22px;border:1px solid #dce4eb;border-radius:12px;margin:16px 0}pre{background:#eef3f7;padding:16px;overflow:auto;font-size:13px}table{border-collapse:collapse;width:100%;font-size:13px}td,th{border:1px solid #dce4eb;padding:8px}.warn{background:#fff1da;padding:16px}select{padding:8px}.matrix{overflow:auto}.on{background:#bde7cc}.off{color:#a9b3bd}</style>
<h1>稠密化实测：区分候选、实际匹配与重复信息</h1>
<p>相同两个 math 模板，四条原规则。审计语义矩阵，而不是 Hadamard 编码后的 ±1 密度；所有匹配均与 native 核对，秩使用有理数精确消元。</p>
<section><h2>1. 候选全密块，不等于图里真的存在匹配</h2><pre>可接：两个 Mul 有相同公因子 k
实际：还必须已经存在 Add(左 Mul 根,右 Mul 根)
本次实际匹配检查没有添加任何新的 Add / Graft fact。</pre>'''
    body+=table([['带重复实例标签的候选',*values(i['raw_potential'])],
                 ['canonical 去重后的候选',*values(i['potential'])],
                 ['有实际 Add 上下文',*values(i['existing'])],
                 ['实际矩阵去掉全零行列',*values(i['active_existing'])]],['矩阵','形状','非零数','密度','精确秩'])
    body+=f'<p class="warn">{i["unique_channels"]} 个不同接口之间，{i["potential"]["nonzeros"]} 个潜在组合中仅 {i["native_existing_matches"]} 个已经具备上下文。不能将剩余组合当成合法的当前 rule apply。</p></section>'
    body+='<section><h2>2. 固定公因子的块</h2>'
    body+=table([[b['factor'],b['potential']['shape'][0],f'{100*b["potential"]["density"]:.0f}%',b['potential']['exact_rank'],
                  b['existing']['nonzeros'],f'{100*b["existing"]["density"]:.1f}%',b['existing']['exact_rank']] for b in i['blocks']],
                ['公因子','边长','候选密度','候选秩','实际非零','实际密度','实际秩'])
    body+='<p>每个候选块都是 J = 1·1ᵀ，秩为 1。应使用广播／因子化表示，不应将它视为值得做大量 GEMM 的丰富计算。</p><select id="block">'+''.join(f'<option>{esc(b["factor"])}</option>' for b in i['blocks'])+'</select><pre id="block-info"></pre><div id="block-matrix"></div></section>'
    body+='''<section><h2>3. 消去内部 binding 后的接口响应</h2><pre>B[(规则 r, 被固定变量 j, 输出锚点 a), 输入值 v]
  = 是否存在完整 LHS binding，使第 j 个变量为 v，匹配锚点为 a。
Y = B Q；取非零得到匹配锚点。
</pre><p>自动覆盖四条规则的所有单变量输入模式，不只挑选好看的模式。其他变量仅作存在量化；不能用不同模式分别成立来拼接一个完整 binding。</p>'''
    body+=table([[f'T{o["cell"]}/完整域',*values(o['raw'])] for o in r['responses']]+
                [[f'T{o["cell"]}/去零行列',*values(o['active'])] for o in r['responses']],
                ['响应','形状','非零数','密度','精确秩'])
    body+='<p>完整域保留所有规则/位置/变量模式和所有 e-class 输入值。去零后的密度单独报告，避免改变分母造成假稠密。</p><select id="response">'+''.join(f'<option>T{o["cell"]}</option>' for o in r['responses'])+'</select><div class="matrix" id="response-matrix"></div></section>'
    body+='<section><h2>4. 精确低秩是否省计算</h2>'
    body+=table([[f'T{o["cell"]}',o['active']['dense_multiply_adds'],o['active']['rank_factor_multiply_adds'],
                  o['active']['nonzero_multiply_adds'],o['active']['nonzero_distinct_rows'],o['active']['shared_row_dot_multiply_adds'],o['active']['shared_row_nonzero_multiply_adds']]
                for o in r['responses']],['去零响应','稠密 Bq','精确 C(Rq)','稀疏非零乘加','不同非零行','共享行点积','共享行稀疏点积'])
    body+='<p>C R = B 已逐元素精确验证。这里的计数仅是每个查询列的算术量，不包含索引、搬运与同步，也不是运行时间。共享行还需分发结果。展示矩阵与 Bake 准备不计入这些内核计数。</p></section>'
    body+='<section><h2>5. 本次结论</h2><p>确实出现条件独立的候选稠密块，但全为秩 1；加入真实上下文后又变稀疏。单变量接口响应也没有呈现丰富的高密度工作。这个超小例子目前支持共享、广播和稀疏执行，没有提供接近神经网络 GEMM 效率的证据。</p><p>没有否定更大 Cell 或其他接口可能出现真正的稠密算子。本实验限定的查询输出是匹配锚点，不是任意后续 effect。</p><p><a href="report.json">完整矩阵、精确分解与结果</a> · <a href="existing-probe.egg">只检查既有上下文的原生程序</a></p></section>'
    response_tables=[]
    for o in r['responses']:
        rows=[]
        for pos,source in enumerate(o['active_rows']):
            spine,slot,anchor=o['rows'][source]
            rows.append([f'S{spine}/v{slot}/a{anchor}',*o['active_matrix'][pos]])
        response_tables.append(table(rows,['规则/输入变量/锚点',*[o['column_labels'][j] for j in o['active_columns']]]))
    data=json.dumps(dict(blocks=i['blocks'],responses=r['responses']))
    body+='<script>const data='+data+';const responseTables='+json.dumps(response_tables)+';function grid(m){return "<table>"+m.map(row=>"<tr>"+row.map(v=>`<td class="${v?"on":"off"}">${v||"·"}</td>`).join("")+"</tr>").join("")+"</table>";}function showBlock(){let b=data.blocks[document.getElementById("block").selectedIndex];document.getElementById("block-info").textContent=b.members.join("\\n");document.getElementById("block-matrix").innerHTML=grid(b.existing_matrix);}function showResponse(){document.getElementById("response-matrix").innerHTML=responseTables[document.getElementById("response").selectedIndex];}document.getElementById("block").onchange=showBlock;document.getElementById("response").onchange=showResponse;showBlock();showResponse();</script></html>'
    (out/'index.html').write_text(body)


if __name__=='__main__':
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--output',type=Path,required=True)
    ap.add_argument('--skip-build',action='store_true')
    a=ap.parse_args()
    r=run(a.output.resolve(),a.skip_build)
    print(json.dumps(dict(responses=[dict(cell=o['cell'],raw=o['raw'],active=o['active']) for o in r['responses']],
                         interaction={k:r['interaction'][k] for k in ('unique_channels','potential','existing','active_existing','native_existing_matches')}),indent=2))
