"""Rule-derived shared matching Spines, exact binding gates, native comparison.

Positive constructor rewrites only. CPU reference matrices, not a GPU benchmark.
"""
from collections import Counter, defaultdict, deque
import argparse
import html
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT))
from experiments.cell_spine.factorized_boundary.demo import (
    Family, Graph, SEEDS, native, mm, transpose, template_inputs, sexpr)

HERE = Path(__file__).resolve().parent
RULES = HERE.parent/'factorized_boundary/rules.egg'


def expression(ast, signatures):
    if ast['kind'] == 'var':
        return ('V', ast['name'])
    if ast['kind'] != 'call':
        raise ValueError('only variable/constructor patterns are supported')
    sig = signatures.get(ast['head'], {})
    if sig.get('kind') != 'constructor' or sig.get('output') != 'Math' or any(s != 'Math' for s in sig.get('input', [])):
        raise ValueError('this prototype requires Math -> Math constructor patterns')
    return (ast['head'], *(expression(a, signatures) for a in ast['args']))


def canonical(t):
    names = []
    def walk(n):
        if n[0] == 'V':
            if n[1] not in names:
                names.append(n[1])
            return ('V', names.index(n[1]))
        return (n[0], *(walk(a) for a in n[1:]))
    return walk(t), names


def pattern_text(t):
    return f'v{t[1]}' if t[0] == 'V' else '('+' '.join([t[0], *(pattern_text(a) for a in t[1:])])+')'


def source_text(t):
    return str(t[1]) if t[0] == 'V' else '('+' '.join([t[0], *(source_text(a) for a in t[1:])])+')'


class Patterns:
    """Intern alpha-equivalent LHS subpatterns, preserving repeated variables."""
    def __init__(self):
        self.nodes, self.index, self.rules = [], {}, []

    def add(self, t):
        t, names = canonical(t)
        if t in self.index:
            return self.index[t]
        base = (t[0], *(('V', i) for i in range(len(t)-1)))
        base_id = None if base == t else self.add(base)
        children = []
        for child in t[1:]:
            if child[0] == 'V':
                children.append(dict(variable=child[1]))
            else:
                sub, ports = canonical(child)
                children.append(dict(spine=self.add(sub), ports=ports))
        pid = len(self.nodes)
        self.index[t] = pid
        self.nodes.append(dict(id=pid, pattern=t, text=pattern_text(t), op=t[0],
                               arity=len(t)-1, variables=len(names), base=pid if base_id is None else base_id,
                               primitive=base == t, children=children))
        return pid

    @classmethod
    def from_ast(cls, data):
        out = cls()
        if data['rules']:
            raise ValueError('general rule bodies are not supported by this prototype')
        for r in data['rewrites']:
            if r['conditions'] or r['subsume'] or r['bidirectional']:
                raise ValueError('guard/subsume/bidirectional rewrite needs explicit lowering')
            raw = expression(r['lhs'], data['signatures'])
            _, variables = canonical(raw)
            pid = out.add(raw)
            out.rules.append(dict(command=r['command_ordinal'], spine=pid, source=r['source'], name=r['name'],
                                  lhs=source_text(raw),variables=variables))
        if not out.rules:
            raise ValueError('no rewrite ASTs; rebuild examples/lhs_ast')
        return out

    def probe(self):
        constructors, rules = [], []
        for p in self.nodes:
            n = p['variables']
            constructors.append(f'(Hit{p["id"]} '+ ' '.join(['Math']*(n+1))+')')
            originals = [r for r in self.rules if r['spine'] == p['id']]
            if originals:
                # Oracle uses the original parsed LHS, not the matcher wiring IR.
                for original in originals:
                    anchor = 'spine_anchor'
                    while anchor in original['variables']:
                        anchor += '_'
                    variables = ' '.join(original['variables'])
                    rules.append(f'(rule ((= {anchor} {original["lhs"]})) ((Hit{p["id"]} {anchor} {variables})) :ruleset probe)')
            else:
                variables = ' '.join(f'v{i}' for i in range(n))
                rules.append(f'(rule ((= anchor {p["text"]})) ((Hit{p["id"]} anchor {variables})) :ruleset probe)')
        return '(datatype Match '+ ' '.join(constructors)+')\n(ruleset probe)\n'+'\n'.join(rules)+'\n'


def class_index(family, roots):
    """Constructor facts over shared symbolic eclass coordinates; no term expansion."""
    classes = set(roots)
    for op, *args in family.nodes:
        if op in ('Add','Mul'):
            classes.update(args)
    index = defaultdict(list)
    for c in sorted(classes):
        alts = family.nodes[c][1:] if family.nodes[c][0] == 'Or' else [c]
        for a in alts:
            op, *args = family.nodes[a]
            if op != 'Input':
                index[op,c].append((c,*args))
    return {key: tuple(sorted(set(rows))) for key, rows in index.items()}


def locations(index, root):
    """One short path per eclass anchor, not exponentially many term paths."""
    paths, todo = {root: []}, deque([root])
    by_root = defaultdict(list)
    for (op, c), rows in index.items():
        by_root[c].extend((op, row) for row in rows)
    while todo:
        c = todo.popleft()
        for op, row in sorted(by_root[c]):
            for pos, child in enumerate(row[1:]):
                if child not in paths:
                    paths[child] = paths[c]+[f'{op}.arg{pos}']
                    todo.append(child)
    return paths


class Matcher:
    """Same algorithm in all ablations; only memoization scope changes."""
    def __init__(self, patterns, index, cache=True):
        self.patterns, self.index, self.cached = patterns, index, cache
        self.memo, self.stats, self.gates, self.events = {}, Counter(), [], []

    def evaluate(self, pid, anchor):
        key = (pid,anchor)
        self.stats['requests'] += 1
        if self.cached and key in self.memo:
            self.stats['cache_hits'] += 1
            self.stats['cached_binding_rows_returned'] += len(self.memo[key])
            self.events.append(dict(spine=pid,anchor=anchor,reused=True,rows=len(self.memo[key])))
            return self.memo[key]
        self.stats['evaluations'] += 1
        p = self.patterns.nodes[pid]
        if p['primitive']:
            rows = self.index.get((p['op'],anchor), ())
            self.stats['constructor_lookups'] += 1
            self.stats['constructor_rows_read'] += len(rows)
            result = rows
        else:
            base = self.evaluate(p['base'],anchor)
            working = [(r[1:], (None,)*p['variables']) for r in base]
            for pos, child in enumerate(p['children']):
                if not working:
                    break
                if 'variable' in child:
                    v = child['variable']
                    later = []
                    for args, bindings in working:
                        if bindings[v] is not None:
                            self.stats['scalar_alias_checks'] += 1
                            if bindings[v] != args[pos]:
                                continue
                        b = list(bindings)
                        b[v] = args[pos]
                        later.append((args,tuple(b)))
                    working = later
                    continue
                right = sorted({r for c in {a[pos] for a,_ in working}
                                for r in self.evaluate(child['spine'],c)})
                if not right:
                    working = []
                    break
                shared = [(j,v) for j,v in enumerate(child['ports']) if working[0][1][v] is not None]
                left_keys = [(args[pos], *(b[v] for _,v in shared)) for args,b in working]
                right_keys = [(r[0], *(r[j+1] for j,_ in shared)) for r in right]
                keys = sorted(set(left_keys+right_keys))
                left_matrix = [[int(k == col) for col in keys] for k in left_keys]
                right_matrix = [[int(k == col) for col in keys] for k in right_keys]
                gate = mm(left_matrix,transpose(right_matrix))
                self.stats['join_matrix_calls'] += 1
                self.stats['key_tuples_built'] += len(left_keys)+len(right_keys)
                self.stats['matrix_multiply_adds'] += len(working)*len(right)*len(keys)
                self.stats['matrix_pair_slots'] += len(working)*len(right)
                self.stats['accepted_pair_slots'] += sum(map(sum,gate))
                self.gates.append(dict(spine=pid,anchor=anchor,child_position=pos,
                    child_spine=child['spine'],child_ports=child['ports'],
                    equality_columns=['parent.arg'+str(pos)+' = child.root']+
                        [f'parent.v{v} = child.v{j}' for j,v in shared],
                    keys=keys,left_keys=left_keys,right_keys=right_keys,
                    L=left_matrix,R=right_matrix,gate=gate))
                later = []
                for i,(args,binding) in enumerate(working):
                    for j,r in enumerate(right):
                        if gate[i][j]:
                            b = list(binding)
                            for k,v in enumerate(child['ports']):
                                b[v] = r[k+1]
                            later.append((args,tuple(b)))
                working = later
            result = tuple(sorted({(anchor,*b) for _,b in working}))
        self.stats['binding_rows_computed'] += len(result)
        self.events.append(dict(spine=pid,anchor=anchor,reused=False,rows=len(result)))
        if self.cached:
            self.memo[key] = result
        return result


def all_requests(patterns,index,roots):
    requests = []
    for cell,root in enumerate(roots):
        for anchor,path in sorted(locations(index,root).items()):
            for rule,r in enumerate(patterns.rules):
                requests.append(dict(cell=cell,rule=rule,spine=r['spine'],anchor=anchor,
                                     location='root' if anchor == root else 'non-root',path=path))
    return requests


def ablation(patterns,index,requests,mode):
    engine, stats, outputs, engines = None, Counter(), [], []
    for r in requests:
        if engine is None or (mode == 'per_cell' and r['cell'] != previous):
            engine = Matcher(patterns,index,cache=mode != 'none')
            engines.append(engine)
        previous = r['cell']
        outputs.append(engine.evaluate(r['spine'],r['anchor']))
    for e in engines:
        stats.update(e.stats)
    return dict(stats=dict(stats),outputs=outputs), engines


def unified_matrix(requests,results):
    evaluations = sorted({(r['spine'],r['anchor']) for r in requests})
    witnesses = sorted({(r['spine'],*b) for r,rows in zip(requests,results) for b in rows})
    values = {(r['spine'],r['anchor']): {(r['spine'],*b) for b in rows} for r,rows in zip(requests,results)}
    dispatch = [[int((r['spine'],r['anchor']) == e) for e in evaluations] for r in requests]
    answers = [[int(w in values[e]) for w in witnesses] for e in evaluations]
    matrix = mm(dispatch,answers)
    assert [sum(row) for row in matrix] == [len(rows) for rows in results]
    return dict(evaluations=evaluations,witnesses=witnesses,dispatch=dispatch,answers=answers,matrix=matrix)


def native_matches(path,patterns,family):
    g = Graph(path)
    mapping = {c:family.from_graph(g,c) for c in g.by_class}
    matches = defaultdict(set)
    for row in g.rows:
        if row['op'].startswith('Hit'):
            pid = int(row['op'][3:])
            matches[pid].add(tuple(mapping[v] for _,v in row['args']))
    return matches


def run(out, skip_build=False):
    out.mkdir(parents=True,exist_ok=False)
    if not skip_build:
        subprocess.run(['cargo','build','--offline','--release','--example','lhs_ast','--example','native_math_baseline'],cwd=ROOT,check=True)
    binary = ROOT/'target/release/examples/native_math_baseline'
    subprocess.run([ROOT/'target/release/examples/lhs_ast', RULES, out/'rules-ast.json'],cwd=ROOT,check=True)
    ast = json.loads((out/'rules-ast.json').read_text())
    patterns = Patterns.from_ast(ast)
    env = RULES.read_text().replace('(Var String)','(RipenInput i64)')
    templates,_ = template_inputs(SEEDS)
    family, roots, graphs = Family(), [], []
    for i,t in enumerate(templates):
        native(binary,out/f'bake/t{i}',env,f'(let root {sexpr(t)})',constructors=['RipenInput','Add','Mul'])
        g = Graph(out/f'bake/t{i}/graph.json')
        roots.append(family.from_graph(g,g.value(t)))
        graphs.append(g)
    index = class_index(family,roots)
    requests = all_requests(patterns,index,roots)
    studies, engine_groups = {}, {}
    for mode in ('none','per_cell','global'):
        studies[mode],engine_groups[mode] = ablation(patterns,index,requests,mode)
    assert studies['none']['outputs'] == studies['per_cell']['outputs'] == studies['global']['outputs']
    engine = engine_groups['global'][0]
    unified = unified_matrix(requests,studies['global']['outputs'])
    results = [dict(**r,bindings=rows) for r,rows in zip(requests,studies['global']['outputs'])]
    native_counts = []
    constructors = ['RipenInput','Add','Mul']+[f'Hit{p["id"]}' for p in patterns.nodes]
    for cell,t in enumerate(templates):
        directory = out/f'native/t{cell}'
        native(binary,directory,env+patterns.probe(),f'(let root {sexpr(t)})',
               '(run-schedule (run probe))',constructors)
        observed = native_matches(directory/'graph.json',patterns,family)
        expected = defaultdict(set)
        for r in results:
            if r['cell'] == cell:
                expected[r['spine']].update(r['bindings'])
        assert dict(observed) == {p:b for p,b in expected.items() if b}
        native_counts.append({str(p):len(b) for p,b in sorted(observed.items())})
    # Actual new boundary: keep cached internal results, add ONE fresh Add anchor.
    boundary = family.node('Add',*roots)
    assert ('Add',boundary) not in index
    extended = dict(index)
    extended['Add',boundary] = ((boundary,*roots),)
    before = Counter(engine.stats)
    old_gate_count = len(engine.gates)
    engine.index = extended
    boundary_rows = {p['id']:engine.evaluate(p['id'],boundary) for p in patterns.nodes}
    boundary_delta = Counter(engine.stats)-before
    fresh = Matcher(patterns,extended,cache=False)
    assert boundary_rows == {p['id']:fresh.evaluate(p['id'],boundary) for p in patterns.nodes}
    inputs = '\n'.join(f'(let c{i} {sexpr(t)})' for i,t in enumerate(templates))
    native(binary,out/'native/boundary',env+patterns.probe(),inputs,
           '(let boundary (Add c0 c1))\n(run-schedule (run probe))',constructors)
    observed = native_matches(out/'native/boundary/graph.json',patterns,family)
    for pid,rows in boundary_rows.items():
        assert set(rows) == {r for r in observed[pid] if r[0] == boundary}
    # Persist actual generated probes for source inspection.
    (out/'probe.egg').write_text(env+patterns.probe()+inputs+'\n(run-schedule (saturate (run)))\n'
                                  '(let boundary (Add c0 c1))\n(run-schedule (run probe))\n')
    wire_counts = Counter()
    for p in patterns.nodes:
        if not p['primitive']:
            wire_counts[p['base']] += 1
            for child in p['children']:
                if 'spine' in child:
                    wire_counts[child['spine']] += 1
    labels = {str(c):sexpr(family.representative(c)) for c in locations(extended,boundary)}
    report = dict(scope='exact CPU shared matching-Spine prototype; native static LHS oracle; no runtime/GPU speedup claim',
                  source=str(RULES.relative_to(ROOT)),templates=[sexpr(t) for t in templates],roots=roots,
                  patterns=patterns.nodes,rules=patterns.rules,static_fragment_references=dict(wire_counts),
                  request_count=len(requests),results=results,labels=labels,
                  ablations={mode:data['stats'] for mode,data in studies.items()},
                  logical_matches_per_template=native_counts,native_full_binding_equal=True,
                  unified_shape=[len(unified['matrix']),len(unified['witnesses'])],
                  unique_match_bindings=len(unified['witnesses']),
                  template_qualified_bindings=sum(len(r['bindings']) for r in results),
                  candidate_spines_enumerated=0,
                  preparation=dict(native_template_bakes=len(graphs),
                                   native_constructor_rows_loaded=sum(len(g.rows) for g in graphs),
                                   shared_matching_index_rows=sum(len(rows) for rows in index.values())),
                  join_gates=engine.gates[:old_gate_count],
                  boundary=dict(anchor=boundary,bindings=boundary_rows,cached_stats=dict(boundary_delta),
                                uncached_stats=dict(fresh.stats),gates=engine.gates[old_gate_count:],native_equal=True),
                  provenance=dict(head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip()))
    (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    (out/'unified-matrix.json').write_text(json.dumps(unified,indent=2)+'\n')
    render(out,report,unified)
    return report


def render(out,r,u):
    esc=html.escape
    def table(rows,headers):
        return '<table><tr>'+''.join('<th>'+esc(str(h))+'</th>' for h in headers)+'</tr>'+''.join(
            '<tr>'+''.join('<td>'+esc(str(v))+'</td>' for v in row)+'</tr>' for row in rows)+'</table>'
    body='''<!doctype html><html lang="zh"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>共同匹配 Spine · 计算复用</title><style>
body{font:16px system-ui;background:#f5f7fa;color:#172d43;max-width:1200px;margin:30px auto;padding:0 20px;line-height:1.7}
section{background:white;border:1px solid #dae2ea;border-radius:12px;padding:20px;margin:16px 0}pre{background:#eef3f7;padding:16px;overflow:auto;font-size:13px}
table{border-collapse:collapse;width:100%;font-size:13px}th,td{padding:9px;border:1px solid #dce4eb;text-align:left}.scroll{overflow:auto}.yes{background:#c4ead5}select{padding:8px}summary{cursor:pointer}.warn{background:#fff1da;padding:14px}
</style><h1>从原规则自动提取共同匹配 Spine</h1>
<p>这次统计的是结构查询、binding 接线与矩阵计算的复用，不报告存储压缩率。RipSat egraph 只用于分析；匹配单元来自四条 rewrite 的 LHS。</p>
<section><h2>1. 自动提取的公共结构</h2><pre>T0 = p0*(p1+p2)
T1 = p0*(p1+p2)+p0*p3</pre>'''
    body+=table([[f'S{p["id"]}',p['text'],'primitive' if p['primitive'] else f'base S{p["base"]}',
                  json.dumps(p['children'],ensure_ascii=False)] for p in r['patterns']],['匹配 Spine','模式','根结构','孩子 binding 接线'])
    body+='<p>变量按首次出现重新编号，但重复变量不被擦掉。例如第二个 Mul 的第一个变量必须与第一个 Mul 相同。</p></section>'
    body+='<section><h2>2. 跨 Cell、root 与内部位置</h2><p>位置按可达 e-class 锚点区分，显示一条最短路径；不展开所有候选项路径。</p>'
    visible=[(i,x) for i,x in enumerate(r['results']) if x['bindings']]
    body+=table([[f'T{x["cell"]}',x['location'],'.'.join(x['path']) or 'root',f'S{x["spine"]}',
                  r['labels'][str(x['anchor'])],len(x['bindings'])] for _,x in visible],['Cell','位置','路径','Spine','锚点','完整 binding 数'])
    body+='</section><section><h2>3. 相同算法，三种共享范围</h2>'
    metrics=['evaluations','constructor_lookups','constructor_rows_read','join_matrix_calls','key_tuples_built','matrix_multiply_adds','binding_rows_computed','cache_hits']
    body+=table([[key,*[r['ablations'][m].get(key,0) for m in ('none','per_cell','global')]] for key in metrics],['实际执行计数','不缓存','Cell 内共享','跨 Cell 共享'])
    body+='<p class="warn">这是本原型的同算法消融，不是原生 egglog Join 探针数，也不是实测加速倍数。稠密 one-hot 矩阵有额外开销；未证明 GPU 性能。</p></section>'
    body+='<section><h2>4. 统一结果矩阵 U = A B</h2><p>A 把每个 (Cell,位置,规则) 请求指向共同求值；B 保存共同求值的完整 binding。U 只是共享结果的分发，真正的匹配在下方 L Rᵀ 门中完成。</p>'
    body+=f'<p>全部 {r["unified_shape"][0]} 个请求 × {r["unified_shape"][1]} 个不同匹配 binding；下面只显示非空行。相同列出现在不同 Cell 行中，表示共享了同一完整结果。</p><div class="scroll"><table><tr><th>请求</th>'
    body+=''.join(f'<th>b{j}</th>' for j in range(len(u['witnesses'])))+'</tr>'
    for i,x in visible:
        body+=f'<tr><th>T{x["cell"]}/{x["location"]}/a{x["anchor"]}/S{x["spine"]}</th>'
        body+=''.join(f'<td class="{ "yes" if v else "" }">{v or "·"}</td>' for v in u['matrix'][i])+'</tr>'
    body+='</table></div><p><a href="unified-matrix.json">完整矩阵及列 binding</a></p></section>'
    gates=r['join_gates']+r['boundary']['gates']
    body+='<section><h2>5. 实际用于匹配的矩阵门</h2><select id="gate">'+''.join(
        f'<option>S{g["spine"]} @ a{g["anchor"]} / child {g["child_position"]}</option>' for g in gates)+'</select><pre id="gate-detail"></pre><p>每个键包含 child root 与所有已绑定的共享变量。不能将两个条件分别存在误当成同一 binding 成立。</p></section>'
    body+='<section><h2>6. 新边界只查询一次新 Add</h2><pre>新增 coarse fact：Add(root(T0), root(T1))</pre>'
    body+=table([[k,r['boundary']['uncached_stats'].get(k,0),r['boundary']['cached_stats'].get(k,0)] for k in metrics],['实际执行计数','从头匹配新边界','复用已有匹配 Spine'])
    body+='<p>这是已有闭包之间的新匹配，尚未重新 saturation。完整 binding 已与原生查询核对；不声称此缓存支持任意更新或 union。</p></section>'
    body+=f'<section><h2>7. 原生核对</h2><p>模板限定的完整 binding 共 {r["template_qualified_bindings"]} 条，共享后不同结果 {r["unique_match_bindings"]} 条。每个规则、每个锚点、每个变量值逐项比较，包含全部非 root 位置；新边界另核验。未枚举完整候选 Spine。</p><p><a href="report.json">完整报告</a> · <a href="probe.egg">自动生成的原生查询</a> · <a href="rules-ast.json">egglog parser 导出的 AST</a></p></section>'
    data=json.dumps(gates)
    body+='<script>const gates='+data+';const labels='+json.dumps(r['labels'])+';function show(){const g=gates[document.getElementById("gate").selectedIndex];document.getElementById("gate-detail").textContent=JSON.stringify({constraints:g.equality_columns,coordinates:g.keys,coordinate_terms:g.keys.map(k=>k.map(v=>labels[v])),L:g.L,R:g.R,"L * R^T":g.gate},null,2);}document.getElementById("gate").onchange=show;show();</script></html>'
    (out/'index.html').write_text(body)


if __name__ == '__main__':
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--output',type=Path,required=True)
    ap.add_argument('--skip-build',action='store_true')
    args=ap.parse_args()
    result=run(args.output.resolve(),args.skip_build)
    print(json.dumps({k:result[k] for k in ['ablations','unified_shape','template_qualified_bindings','unique_match_bindings','boundary']},indent=2))
