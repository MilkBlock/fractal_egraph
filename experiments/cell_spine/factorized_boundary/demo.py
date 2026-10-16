"""Parameterized RipenInput templates, shared candidate circuits, exact matrices.

Standalone CPU experiment; native egglog certifies closure and boundary matches.
Full candidate expansion is confined to the independent validation/baseline path.
"""
from collections import Counter
from functools import lru_cache
from itertools import product
import argparse
import hashlib
import html
import json
from pathlib import Path
import subprocess
import sys

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
sys.path.insert(0, str(ROOT))
from tools.bench.math_graph_equal import verify


def sexpr(t):
    if t[0] == 'Var':
        return '(Var ' + json.dumps(t[1]) + ')'
    if t[0] == 'Input':
        return f'(RipenInput {t[1]})'
    return '(' + t[0] + ' ' + ' '.join(map(sexpr, t[1:])) + ')'


def erase(t):
    """First-occurrence parameter renaming, not general slot-symmetry discovery."""
    bindings = []
    def visit(node):
        if node[0] == 'Var':
            if node[1] not in bindings:
                bindings.append(node[1])
            return ('Input', bindings.index(node[1]))
        return (node[0], *(visit(a) for a in node[1:]))
    return visit(t), bindings


def bind(t, bindings):
    if t[0] == 'Input':
        return ('Var', bindings[t[1]])
    return (t[0], *(bind(a, bindings) for a in t[1:]))


def ground_key(t):
    """Exact key for this experiment's interface domain: Vars and binary sums.

    No associativity, distribution, unknown union or arbitrary Expr canonicalizer
    is claimed. Native validation checks this restricted key against real classes.
    """
    if t[0] == 'Var':
        return t
    if t[0] != 'Add':
        raise ValueError('interface key outside certified Var/Add domain')
    return ('Add', *sorted((ground_key(a) for a in t[1:]), key=sexpr))


def json_bytes(value):
    return len(json.dumps(value, separators=(',', ':')).encode())


class Graph:
    """Read-only native graph, for Bake and independent verification only."""
    def __init__(self, path):
        self.rows = json.loads(Path(path).read_text())
        self.by_class, self.lookup = {}, {}
        for r in self.rows:
            if r['sort'] == 'Math':
                self.by_class.setdefault(r['result'], []).append(r)
                self.lookup[r['op'], tuple(v for _, v in r['args'])] = r['result']

    def value(self, t):
        if t[0] in ('Input', 'Var'):
            return self.lookup['RipenInput' if t[0] == 'Input' else 'Var', (str(t[1]),)]
        return self.lookup[t[0], tuple(self.value(a) for a in t[1:])]

    @lru_cache(None)
    def representative(self, value):
        choices = []
        for r in self.by_class[value]:
            if r['op'] == 'RipenInput':
                choices.append(('Input', int(r['args'][0][1])))
            elif r['op'] == 'Var':
                choices.append(('Var', r['args'][0][1]))
            else:
                choices.append((r['op'], *(self.representative(v) for _, v in r['args'])))
        return min(choices, key=lambda t: (len(sexpr(t)), sexpr(t)))

    @lru_cache(None)
    def enumerate_terms(self, value):
        """Validation only. Fixture graphs must be acyclic."""
        result = set()
        for r in self.by_class[value]:
            if r['op'] in ('Var', 'RipenInput'):
                result.add(('Var', r['args'][0][1]) if r['op'] == 'Var'
                           else ('Input', int(r['args'][0][1])))
            else:
                result.update((r['op'], *a) for a in product(
                    *(self.enumerate_terms(v) for _, v in r['args'])))
        return result


class Family:
    """Shared typed sum/product circuit extracted from native template eclasses."""
    def __init__(self):
        self.nodes, self.intern = [], {}

    def node(self, op, *args):
        key = (op, *args)
        if key not in self.intern:
            self.intern[key] = len(self.nodes)
            self.nodes.append(key)
        return self.intern[key]

    def from_graph(self, graph, root):
        memo, active = {}, set()
        def visit(c):
            if c in memo:
                return memo[c]
            if c in active:
                raise ValueError('cyclic closure needs a recursive representation')
            active.add(c)
            alternatives = []
            for row in sorted(graph.by_class[c], key=lambda r: (r['op'], str(r['args']))):
                if row['op'] == 'RipenInput':
                    n = self.node('Input', int(row['args'][0][1]))
                elif row['op'] in ('Add', 'Mul'):
                    n = self.node(row['op'], *(visit(v) for _, v in row['args']))
                else:
                    raise ValueError('template must contain RipenInput, never concrete Var')
                alternatives.append(n)
            alternatives = sorted(set(alternatives))
            memo[c] = alternatives[0] if len(alternatives) == 1 else self.node('Or', *alternatives)
            active.remove(c)
            return memo[c]
        return visit(root)

    @lru_cache(None)
    def count(self, n):
        op, *args = self.nodes[n]
        if op == 'Input':
            return 1
        if op == 'Or':
            return sum(self.count(a) for a in args)
        return self.count(args[0]) * self.count(args[1])

    @lru_cache(None)
    def representative(self, n):
        op, *args = self.nodes[n]
        if op == 'Input':
            return ('Input', args[0])
        if op == 'Or':
            return min((self.representative(a) for a in args), key=lambda t: (len(sexpr(t)), sexpr(t)))
        return (op, *(self.representative(a) for a in args))

    @lru_cache(None)
    def root_mul(self, n):
        """Symbolic joint (factor,residual) projection, compiled once per type."""
        op, *args = self.nodes[n]
        if op == 'Or':
            result = Counter()
            for a in args:
                result.update(self.root_mul(a))
            return result
        if op != 'Mul':
            return Counter()
        return Counter({tuple(self.representative(a) for a in args):
                        self.count(args[0]) * self.count(args[1])})

    def enumerate(self, n):
        """Validation and ordinary-hashcons baseline only."""
        op, *args = self.nodes[n]
        if op == 'Input':
            return [('Input', args[0])]
        if op == 'Or':
            return [t for a in args for t in self.enumerate(a)]
        return [(op, a, b) for a, b in product(self.enumerate(args[0]), self.enumerate(args[1]))]

    def reachable(self, root):
        found = set()
        def visit(n):
            if n in found:
                return
            found.add(n)
            if self.nodes[n][0] != 'Input':
                for a in self.nodes[n][1:]:
                    visit(a)
        visit(root)
        return found

    def egraph_encoding(self, roots):
        """Stronger baseline: the SAME sharing as enodes grouped into eclasses."""
        classes = set(roots)
        for op, *args in self.nodes:
            if op in ('Add', 'Mul'):
                classes.update(args)
        class_ids = {c: i for i, c in enumerate(sorted(classes))}
        enodes, groups, owners = [], [], {}
        for c in sorted(classes):
            alts = self.nodes[c][1:] if self.nodes[c][0] == 'Or' else [c]
            group = []
            for n in alts:
                assert n not in owners, 'different classes share a constructor: rebuild required'
                owners[n] = c
                op, *args = self.nodes[n]
                encoded = [op, *args] if op == 'Input' else [op, *(class_ids[a] for a in args)]
                group.append(len(enodes))
                enodes.append(encoded)
            groups.append(group)
        return dict(enodes=enodes, eclasses=groups, roots=[class_ids[r] for r in roots])


def instantiate(projection, bindings):
    # Distinct variables are the certified instance domain for this experiment.
    if len(set(bindings)) != len(bindings):
        raise ValueError('aliased inputs require specialization and candidate deduplication')
    out = Counter()
    for (k, u), count in projection.items():
        out[tuple(sexpr(ground_key(bind(t, bindings))) for t in (k, u))] += count
    return out


def transpose(m):
    return list(map(list, zip(*m)))


def mm(a, b):
    assert a and b and len(a[0]) == len(b)
    return [[sum(x*y for x, y in zip(row, col)) for col in zip(*b)] for row in a]


def matrix_search(projections):
    keys = sorted({key for p in projections for key in p})
    if not keys:
        return dict(binding_columns=[], Q=[[] for _ in projections], K=[], factor_columns=[],
                    F=[], V=[[] for _ in projections], scores=[[0]*len(projections) for _ in projections], records=[])
    q = [[p[key] for key in keys] for p in projections]
    factors = sorted({k for k, _ in keys})
    f = [[int(k == factor) for factor in factors] for k, _ in keys]
    v = mm(q, f)
    scores = mm(v, transpose(v))
    compatibility = mm(f, transpose(f))
    assert mm(mm(q, compatibility), transpose(q)) == scores
    records = []
    for i, left in enumerate(projections):
        for j, right in enumerate(projections):
            for (k, u), lc in left.items():
                for (k2, w), rc in right.items():
                    if k == k2:
                        records.append(dict(left=i, right=j, factor=k, u=u, v=w,
                                            spine_pairs=lc*rc, target=f'(Mul {k} (Add {u} {w}))'))
    return dict(binding_columns=keys, Q=q, K=compatibility, factor_columns=factors,
                F=f, V=v, scores=scores, records=records)


def native(binary, directory, environment, inputs, export='', constructors=None):
    directory.mkdir(parents=True)
    (directory/'env.egg').write_text(environment)
    (directory/'input.egg').write_text(inputs)
    manifest = dict(environment=str(directory/'env.egg'), input=str(directory/'input.egg'),
                    run='(run-schedule (saturate (run)))', export=export,
                    constructors=constructors or ['Var', 'Add', 'Mul'])
    (directory/'phases.json').write_text(json.dumps(manifest, indent=2))
    proc = subprocess.run([str(binary), 'bench', str(directory/'phases.json'), str(directory/'graph.json')],
                          cwd=ROOT, text=True, capture_output=True, timeout=60)
    (directory/'stderr.txt').write_text(proc.stderr)
    if proc.returncode:
        raise RuntimeError(proc.stderr)
    result = json.loads(proc.stdout)
    (directory/'result.json').write_text(json.dumps(result, indent=2))
    return result


def small(x, a, b):
    return ('Mul', ('Var', x), ('Add', ('Var', a), ('Var', b)))


def big(x, a, b, c):
    return ('Add', small(x, a, b), ('Mul', ('Var', x), ('Var', c)))


SEEDS = [small('x','a','b'), big('x','a','b','c'), small('z','a','b'), big('y','d','e','f')]


def template_inputs(seeds):
    templates, instances = [], []
    for seed in seeds:
        erased, bindings = erase(seed)
        if erased not in templates:
            templates.append(erased)
        instances.append(dict(template=templates.index(erased), binding=bindings))
    return templates, instances


def term_hashcons(candidates):
    nodes, index = [], {}
    def visit(t):
        if t not in index:
            encoded = list(t) if t[0] == 'Input' else [t[0], *(visit(a) for a in t[1:])]
            index[t] = len(nodes)
            nodes.append(encoded)
        return index[t]
    roots = [[visit(t) for t in group] for group in candidates]
    return dict(nodes=nodes, roots=roots)


def run(out, binary):
    out.mkdir(parents=True, exist_ok=False)
    env = (HERE/'rules.egg').read_text()
    upstream = (ROOT/'egglog/tests/math-microbenchmark.egg').read_text()
    assert all(line in upstream for line in env.splitlines() if line.startswith('(rewrite'))
    template_env = env.replace('(Var String)', '(RipenInput i64)')
    templates, instances = template_inputs(SEEDS)
    family, roots, graphs, shapes = Family(), [], [], []
    for i, template in enumerate(templates):
        directory = out/f'bake/t{i}'
        result = native(binary, directory, template_env, f'(let root {sexpr(template)})',
                        constructors=['RipenInput','Add','Mul'])
        graph = Graph(directory/'graph.json')
        roots.append(family.from_graph(graph, graph.value(template)))
        graphs.append(graph)
        shapes.append(result['verification']['restored'])
    assert len(set(roots)) == len(templates) == 2
    symbolic = [family.root_mul(r) for r in roots]
    projections = [instantiate(symbolic[i['template']], i['binding']) for i in instances]
    compact = matrix_search(projections)
    encoding = dict(nodes=family.nodes, roots=roots)
    egraph_encoding = family.egraph_encoding(roots)
    # Saved before the validation/baseline code is allowed to expand candidates.
    (out/'compact.json').write_text(json.dumps(dict(templates=encoding, instances=instances,
        symbolic_projections=[[[sexpr(k),sexpr(u),n] for (k,u),n in p.items()] for p in symbolic],
        **compact), indent=2)+'\n')
    (out/'ripsat-baseline.json').write_text(json.dumps(egraph_encoding, indent=2)+'\n')

    # Validation only: enumerate each PARAMETERIZED template once, never per name.
    flat = [family.enumerate(r) for r in roots]
    closure_checks = []
    for i, (template, candidates, graph) in enumerate(zip(templates, flat, graphs)):
        assert len(candidates) == len(set(candidates)) == family.count(roots[i])
        assert set(candidates) == graph.enumerate_terms(graph.value(template))
        for j, candidate in enumerate(candidates):
            directory = out/f'validation/t{i}/s{j}'
            native(binary, directory, template_env, f'(let root {sexpr(candidate)})',
                   constructors=['RipenInput','Add','Mul'])
            check = verify(out/f'bake/t{i}/graph.json', directory/'graph.json')
            closure_checks.append(dict(template=i, candidate=j, **check))
    baseline = term_hashcons(flat)
    per_template = []
    for i, (template, graph) in enumerate(zip(templates, graphs)):
        isolated = Family()
        root = isolated.from_graph(graph, graph.value(template))
        per_template.append(dict(nodes=isolated.nodes, roots=[root]))
    instance_terms = [[bind(t, inst['binding']) for t in flat[inst['template']]] for inst in instances]
    inputs = '\n'.join(f'(let c{i} {sexpr(t)})' for i,t in enumerate(SEEDS))
    source = env+'\n'+inputs+'\n(run-schedule (saturate (run)))\n'
    (out/'source.egg').write_text(source)
    native(binary, out/'parents', env, inputs)
    actual = Graph(out/'parents/graph.json')
    brute = []
    for left in instance_terms:
        brute.append([sum(1 for a,b in product(left,right) if a[0] == b[0] == 'Mul'
                          and actual.value(a[1]) == actual.value(b[1])) for right in instance_terms])
    assert brute == compact['scores']

    probe = '''
(datatype Match (Hit i64 i64 Math Math Math))
(relation Graft (i64 i64 Math))
(ruleset probe)
(rule ((Graft i j root) (= root (Add l r))
       (= l (Mul k u)) (= r (Mul k v)))
      ((Hit i j k u v)) :ruleset probe)
'''
    n = len(instances)
    grafts = '\n'.join(f'(Graft {i} {j} (Add c{i} c{j}))' for i in range(n) for j in range(n))
    native(binary, out/'native-boundary', env+probe, inputs, grafts+'\n(run-schedule (run probe))',
           ['Var','Add','Mul','Hit'])
    observed = Graph(out/'native-boundary/graph.json')
    hits = {(int(r['args'][0][1]), int(r['args'][1][1]),
             *(sexpr(ground_key(observed.representative(v))) for _,v in r['args'][2:]))
            for r in observed.rows if r['op'] == 'Hit'}
    expected = {(r['left'],r['right'],r['factor'],r['u'],r['v']) for r in compact['records']}
    assert hits == expected
    checks = '\n'.join(f'(check (= (Add c{r["left"]} c{r["right"]}) {r["target"]}))' for r in compact['records'])
    graft_source = source+'\n'.join(f'(let graft_{i}_{j} (Add c{i} c{j}))' for i in range(n) for j in range(n))
    graft_source += '\n(run 1)\n'+checks+'\n'
    (out/'graft-check.egg').write_text(graft_source)
    proc = subprocess.run([str(binary),str(out/'graft-check.egg')], cwd=ROOT,text=True,capture_output=True,timeout=60)
    (out/'graft-check.log').write_text(proc.stdout+proc.stderr)
    assert proc.returncode == 0, proc.stderr
    storage = dict(parameterized_term_hashcons_bytes=json_bytes(baseline),
                   parameterized_term_hashcons_nodes=len(baseline['nodes']),
                   independent_factorized_bytes=json_bytes(per_template),
                   independent_factorized_nodes=sum(len(p['nodes']) for p in per_template),
                   shared_factorized_bytes=json_bytes(encoding), shared_factorized_nodes=len(family.nodes),
                   ripsat_egraph_bytes=json_bytes(egraph_encoding), ripsat_enodes=len(egraph_encoding['enodes']),
                   ripsat_eclasses=len(egraph_encoding['eclasses']), binding_bytes=json_bytes(instances),
                   compiled_projection_bytes=json_bytes([[[sexpr(k),sexpr(u),v] for (k,u),v in p.items()] for p in symbolic]))
    report = dict(scope='standalone parameterized CPU prototype; native closure oracle; no production/GPU acceleration',
                  template_count=len(templates), instance_count=n, templates=[sexpr(t) for t in templates],
                  instances=instances, template_candidates=list(map(len,flat)), template_shapes=shapes,
                  shared_nodes=len(family.reachable(roots[0]) & family.reachable(roots[1])),
                  smaller_template_is_subcircuit=roots[0] in family.reachable(roots[1]),
                  storage=storage, structural_sharing_same_as_ripsat=True,
                  symbolic_projection_compilations=len(symbolic), compact_candidates_enumerated=0,
                  validation_candidates_enumerated=sum(map(len,flat)), validation_instance_candidate_pairs=sum(map(len,instance_terms))**2,
                  complete_native_term_family=True, closure_checks=closure_checks,
                  native_binding_equal=True, native_distinct_bindings=len(hits), native_graft_checks=len(expected),
                  **compact,
                  provenance=dict(head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),
                                  rules_sha256=hashlib.sha256(env.encode()).hexdigest(),
                                  native_binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest()))
    (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    (out/'candidates.json').write_text(json.dumps([[sexpr(t) for t in c] for c in flat],indent=2)+'\n')
    render(out, report, flat)
    return report


def render(out, r, flat):
    esc = html.escape
    def table(m, rownames, colnames):
        return '<table><tr><th></th>'+''.join('<th>'+esc(str(c))+'</th>' for c in colnames)+'</tr>'+''.join(
            '<tr><th>'+esc(str(name))+'</th>'+''.join(f'<td class="{ "yes" if x else "no" }">{x}</td>' for x in row)+'</tr>'
            for name,row in zip(rownames,m))+'</table>'
    s = r['storage']
    body = '''<!doctype html><html lang="zh"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>修正版 · RipenInput 模板因子化</title><style>
body{font:16px system-ui;background:#f5f7fa;color:#172d43;max-width:1120px;margin:30px auto;padding:0 20px;line-height:1.7}
section{background:white;border:1px solid #dae2ea;border-radius:12px;padding:20px;margin:16px 0}pre{background:#eef3f7;padding:16px;overflow:auto;font-size:13px}
table{border-collapse:collapse;width:100%;font-size:13px}th,td{padding:10px;border:1px solid #dce4eb;text-align:center}.yes{background:#d9f2e6}.no{color:#8998a8}
select{padding:8px}summary{cursor:pointer}.warn{background:#fff1da;border-left:4px solid #cf8b18;padding:15px}</style>
<h1>先擦除字母，再因子化 Spine 候选</h1>
<p class="warn">修正上一版：x(a+b)、x(c+d)、y(a+b)、z(e+f) 是一个模板的四个实例。旧版 1,217→938 B 不能证明相对 RipenInput 参数化的增益。本页已替换为两个真实不同的模板。</p>
<p>独立 CPU 原型；native egglog 完整闭包与边界匹配验证。不是 GPU 性能报告，也没有接管生产 Bake。</p>
<section><h2>1. 模板只保存一次，binding 单独计账</h2>
<pre>T0 = p0 * (p1 + p2)
T1 = p0 * (p1 + p2) + p0 * p3
p_i = RipenInput(i)</pre>
'''
    body += table([[r['template_candidates'][i],sh['enodes'],sh['eclasses']] for i,sh in enumerate(r['template_shapes'])],
                  ['T0（3 个输入）','T1（4 个输入）'],['完整 Spine 数','e-node','e-class'])
    body += '<p>不是通过改字母伪造模板：模板直接以 RipenInput 节点运行原生 saturation。T0 整个闭包出现在 T1 内部。</p>'
    body += table([[f'T{i["template"]}',', '.join(i['binding'])] for i in r['instances']],
                  [f'I{i}' for i in range(r['instance_count'])],['模板','实际 binding'])+'</section>'
    body += '''<section><h2>2. 因子化语法来自实际闭包</h2><pre>S = Add(p1,p2) | Add(p2,p1)
P = Mul(p0,p1) | Mul(p1,p0)
Q = Mul(p0,p2) | Mul(p2,p0)
T0 = Mul(p0,S) | Mul(S,p0) | Add(P,Q) | Add(Q,P)  # 12

Z = Mul(p0,p3) | Mul(p3,p0)
H = Add(S,p3) | Add(p3,S)                         # 4
T1 = Add(T0,Z) | Add(Z,T0) | Mul(p0,H) | Mul(H,p0)
   = 12×2 + 2×12 + 4 + 4 = 56</pre>
<p>T1 引用共享的 T0，两个实例类型各编译一次。上面是简写；完整电路来自 native e-class 的 OR 与构造器孩子的乘积，不再手写枚举 56 条候选。</p>'''
    for i,candidates in enumerate(flat):
        body += f'<details><summary>验证专用：T{i} 的 {len(candidates)} 个候选</summary><pre>'+esc('\n'.join(map(sexpr,candidates)))+'</pre></details>'
    body += '</section><section><h2>3. 公平的存储对照</h2>'
    body += table([[s['parameterized_term_hashcons_nodes'],s['parameterized_term_hashcons_bytes']],
                   [s['independent_factorized_nodes'],s['independent_factorized_bytes']],
                   [s['shared_factorized_nodes'],s['shared_factorized_bytes']],
                   [str(s['ripsat_enodes'])+' enode / '+str(s['ripsat_eclasses'])+' eclass',s['ripsat_egraph_bytes']]],
                  ['RipenInput + 全部候选普通 hashcons','两个模板各自因子化','两个模板联合因子化','直接共享 RipSat egraph'],['结构数量','序列化字节'])
    body += f'<p>四份实例 binding：<b>{s["binding_bytes"]} B</b>，每种方案都加同样的这部分。符号边界投影另外占 {s["compiled_projection_bytes"]} B。不是 RSS。</p>'
    body += '<p class="warn">联合因子化与直接共享 RipSat egraph 的逻辑共享相同；不能声称比现有 e-class 表示又发现了新压缩。两种序列化字节差异只是布局。新增验证的是：可直接在既有共享表示上编译并计算边界矩阵。</p></section>'
    body += '''<section><h2>4. 模板投影一次，实例只代入 binding</h2><pre>T0：Mul(k,u) → (p0, p1+p2):2，(p1+p2, p0):2
T1：Mul(k,u) → (p0, (p1+p2)+p3):4，((p1+p2)+p3, p0):4

外部 coarse fact：Add(root(Ii), root(Ij))
边界规则：Add(Mul(k,u), Mul(k,v)) → Mul(k, Add(u,v))
V = QF；K = FFᵀ；S = QKQᵀ = VVᵀ</pre>
<p>模板投影只包含 RipenInput。下面矩阵才使用实际 binding；必须保留具体身份，否则会把所有实例错误地连在一起。Q 保留联合 (k,u)，V 仅用于检查公因子。</p>'''
    names=[f'I{i}/T{inst["template"]}' for i,inst in enumerate(r['instances'])]
    body += table(r['V'],names,r['factor_columns'])+'<h3>S：Spine 候选对数量</h3>'+table(r['scores'],names,names)
    options=''.join(f'<option>{esc(n)}</option>' for n in names)
    body += f'<p>左实例 <select id="left">{options}</select> 右实例 <select id="right">{options}</select></p><pre id="explain"></pre>'
    body += '<p>数值不是不同 logical match 数，更不是 committed mutation。此例只检查根上的 Mul 触点；不覆盖任意非 root、重复输入、动态 union 或 guard。</p></section>'
    body += f'<section><h2>5. 验证</h2><p>两个参数模板的全部 {r["validation_candidates_enumerated"]} 个 Spine 逐个 native saturation，完整 constructor 行与等价分区一致。验证路径另穷举 {r["validation_instance_candidate_pairs"]:,} 个实例候选对；矩阵逐项相同。原生查询得到 {r["native_distinct_bindings"]} 个完整 binding，原规则执行后所有目标检查通过。</p><p>紧凑计算路径展开候选数：0。模板与实例存储测试防止再把重命名当成新模板。</p><p><a href="source.egg">实例源码</a> · <a href="compact.json">参数化电路与矩阵</a> · <a href="ripsat-baseline.json">RipSat 基线</a> · <a href="report.json">完整结果</a> · <a href="graft-check.egg">执行检查</a></p></section>'
    body += '<script>const data='+json.dumps(dict(records=r['records'],scores=r['scores']))+';function show(){let i=document.getElementById("left").selectedIndex,j=document.getElementById("right").selectedIndex;let rs=data.records.filter(r=>r.left===i&&r.right===j);document.getElementById("explain").textContent=`候选对 ${data.scores[i][j]}；不同 binding ${rs.length}\\n`+rs.map(r=>`k=${r.factor}\\nu=${r.u}\\nv=${r.v}\\n→ ${r.target}\\n该 binding 对应 ${r.spine_pairs} 对 Spine`).join("\\n\\n");}document.getElementById("left").onchange=show;document.getElementById("right").onchange=show;document.getElementById("right").selectedIndex=1;show();</script></html>'
    (out/'index.html').write_text(body)


if __name__ == '__main__':
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--output',type=Path,required=True)
    ap.add_argument('--skip-build',action='store_true')
    args=ap.parse_args()
    if not args.skip_build:
        subprocess.run(['cargo','build','--offline','--release','--example','native_math_baseline'],cwd=ROOT,check=True)
    report=run(args.output.resolve(),ROOT/'target/release/examples/native_math_baseline')
    print(json.dumps({k:report[k] for k in ['template_count','instance_count','template_candidates','storage','scores','native_distinct_bindings']},indent=2))
