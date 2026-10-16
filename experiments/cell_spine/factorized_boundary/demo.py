"""Exact factorized Spine families; CPU matrix demo, native egglog oracle.

The compact path never enumerates complete candidates. Enumeration is a separate
oracle only. This is a hand-derived four-rule compiler, not the production Bake.
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
    return '(' + t[0] + ' ' + ' '.join(map(sexpr, t[1:])) + ')'


def var(s):
    return ('Var', s)


class Family:
    """Hash-consed union/product circuit. OR branches here are disjoint syntax."""
    def __init__(self):
        self.nodes = []
        self.intern = {}

    def node(self, op, *args):
        key = (op, *args)
        if key not in self.intern:
            self.intern[key] = len(self.nodes)
            self.nodes.append(key)
        return self.intern[key]

    def comm(self, op, a, b):
        return self.node('Or', self.node(op, a, b), self.node(op, b, a))

    def cell(self, x, a, b):
        if len({x, a, b}) != 3:
            raise ValueError('this disjoint-choice schema requires three distinct Var names')
        x, a, b = (self.node('Var', s) for s in (x, a, b))
        factored = self.comm('Mul', x, self.comm('Add', a, b))
        expanded = self.comm('Add', self.comm('Mul', x, a), self.comm('Mul', x, b))
        return self.node('Or', factored, expanded)

    @lru_cache(None)
    def count(self, n):
        op, *args = self.nodes[n]
        if op == 'Var':
            return 1
        if op == 'Or':
            return sum(self.count(a) for a in args)
        return self.count(args[0]) * self.count(args[1])

    def enumerate(self, n):
        """Validation only. Never called by annotation/projection/matrix scoring."""
        op, *args = self.nodes[n]
        if op == 'Var':
            return [('Var', args[0])]
        if op == 'Or':
            return [t for a in args for t in self.enumerate(a)]
        return [(op, a, b) for a, b in product(self.enumerate(args[0]), self.enumerate(args[1]))]


class Graph:
    """Read-only native canonical constructor rows, never constructs new terms."""
    def __init__(self, path):
        self.rows = json.loads(Path(path).read_text())
        self.lookup = {(r['op'], tuple(v for _, v in r['args'])): r['result']
                       for r in self.rows if r['sort'] == 'Math'}
        self.terms = {}
        # Small acyclic ground fixture: choose a stable shortest ground name.
        changed = True
        while changed:
            changed = False
            for r in self.rows:
                if r['sort'] != 'Math':
                    continue
                if r['op'] == 'Var':
                    t = ('Var', r['args'][0][1])
                elif all(v in self.terms for _, v in r['args']):
                    t = (r['op'], *(self.terms[v] for _, v in r['args']))
                else:
                    continue
                old = self.terms.get(r['result'])
                if old is None or (len(sexpr(t)), sexpr(t)) < (len(sexpr(old)), sexpr(old)):
                    self.terms[r['result']] = t
                    changed = True

    def value(self, t):
        args = (t[1],) if t[0] == 'Var' else tuple(self.value(a) for a in t[1:])
        return self.lookup[t[0], args]

    def label(self, v):
        return sexpr(self.terms[v])


class Compile:
    def __init__(self, family, graph):
        self.f, self.g = family, graph
        self.visited = set()

    @lru_cache(None)
    def value(self, n):
        self.visited.add(n)
        op, *args = self.f.nodes[n]
        if op == 'Var':
            return self.g.lookup['Var', (args[0],)]
        values = tuple(self.value(a) for a in args)
        if op == 'Or':
            if len(set(values)) != 1:
                raise ValueError('choice alternatives are not in one native eclass')
            return values[0]
        return self.g.lookup[op, values]

    @lru_cache(None)
    def root_mul(self, n):
        """Project root Mul(k,u), preserving the joint (k,u) binding."""
        self.value(n)  # Certifies all represented choices in the native closure.
        op, *args = self.f.nodes[n]
        if op == 'Or':
            out = Counter()
            for a in args:
                out.update(self.root_mul(a))
            return out
        if op != 'Mul':
            return Counter()
        key = tuple(self.g.label(self.value(a)) for a in args)
        return Counter({key: self.f.count(args[0]) * self.f.count(args[1])})


def transpose(m):
    return list(map(list, zip(*m)))


def mm(a, b):
    """Exact integer matrix multiplication on CPU; no GPU performance claim."""
    assert a and b and len(a[0]) == len(b)
    return [[sum(x*y for x, y in zip(row, col)) for col in zip(*b)] for row in a]


def matrix_search(projections):
    # Full binding columns retain k,u correlation; only k connects the two sides.
    keys = sorted({key for p in projections for key in p})
    q = [[p[key] for key in keys] for p in projections]
    compatibility = [[int(a[0] == b[0]) for b in keys] for a in keys]
    scores = mm(mm(q, compatibility), transpose(q))
    # A smaller exact factorization K = F F^T gives V V^T.
    factors = sorted({a for a, _ in keys})
    f = [[int(a == k) for k in factors] for a, _ in keys]
    v = mm(q, f)
    assert mm(v, transpose(v)) == scores
    records = []
    for i, left in enumerate(projections):
        for j, right in enumerate(projections):
            for (k, u), lc in left.items():
                for (k2, w), rc in right.items():
                    if k == k2:
                        records.append(dict(left=i, right=j, factor=k, u=u, v=w,
                                            spine_pairs=lc*rc,
                                            target=f'(Mul {k} (Add {u} {w}))'))
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
    proc = subprocess.run([str(binary), 'bench', str(directory/'phases.json'),
                           str(directory/'graph.json')], cwd=ROOT, text=True, capture_output=True)
    (directory/'stderr.txt').write_text(proc.stderr)
    if proc.returncode:
        raise RuntimeError(proc.stderr)
    result = json.loads(proc.stdout)
    (directory/'result.json').write_text(json.dumps(result, indent=2))
    return result


CELLS = [('x', 'a', 'b'), ('x', 'c', 'd'), ('y', 'a', 'b'), ('z', 'e', 'f')]


def run(out, binary):
    out.mkdir(parents=True, exist_ok=False)
    env = (HERE/'rules.egg').read_text()
    upstream = (ROOT/'egglog/tests/math-microbenchmark.egg').read_text()
    assert all(line in upstream for line in env.splitlines() if line.startswith('(rewrite'))
    seeds = [('Mul', var(x), ('Add', var(a), var(b))) for x, a, b in CELLS]
    inputs = '\n'.join(f'(let c{i} {sexpr(t)})' for i, t in enumerate(seeds))
    native(binary, out/'parents', env, inputs)
    graph = Graph(out/'parents/graph.json')
    family = Family()
    roots = [family.cell(*c) for c in CELLS]
    compiler = Compile(family, graph)
    projections = [compiler.root_mul(r) for r in roots]
    compact = matrix_search(projections)
    # Save compact computation BEFORE enumerating any complete Spine.
    (out/'compact.json').write_text(json.dumps(dict(nodes=family.nodes, roots=roots,
        counts=[family.count(r) for r in roots], **compact), indent=2))

    # Independent full candidate enumeration, kept out of the compact algorithm.
    flat = [family.enumerate(r) for r in roots]
    by_class = {}
    for row in graph.rows:
        by_class.setdefault(row['result'], []).append(row)

    @lru_cache(None)
    def native_terms(value):
        """Independent exhaustive extraction, only for this acyclic oracle."""
        result = set()
        for row in by_class[value]:
            if row['op'] == 'Var':
                result.add(('Var', row['args'][0][1]))
            else:
                result.update((row['op'], *args) for args in product(
                    *(native_terms(v) for _, v in row['args'])))
        return result

    for seed, candidates in zip(seeds, flat):
        assert set(candidates) == native_terms(graph.value(seed))
    for candidates, projection in zip(flat, projections):
        assert len(set(candidates)) == len(candidates) == 12
        expected = Counter(tuple(graph.label(graph.value(t)) for t in expr[1:])
                           for expr in candidates if expr[0] == 'Mul')
        assert expected == projection
    brute = [[sum(1 for a in left for b in right if a[0] == b[0] == 'Mul'
                  and graph.value(a[1]) == graph.value(b[1]))
              for right in flat] for left in flat]
    assert brute == compact['scores']

    # Every candidate alone must regenerate exactly the same full native Cell.
    checks = []
    for i, candidates in enumerate(flat):
        baseline = out/f'equivalence/c{i}/baseline'
        native(binary, baseline, env, f'(let root {sexpr(seeds[i])})')
        for j, t in enumerate(candidates):
            dst = out/f'equivalence/c{i}/s{j}'
            native(binary, dst, env, f'(let root {sexpr(t)})')
            result = verify(baseline/'graph.json', dst/'graph.json')
            assert result == dict(exact_equal=True, enodes=13, eclasses=7)
            checks.append(dict(cell=i, candidate=j, **result))

    # Actual egglog LHS query, BEFORE new grafts themselves are saturated.
    # Add(c_i,c_j) is an explicit supplied coarse fact, never inferred from overlap.
    probe = '''
(datatype Match (Hit i64 i64 Math Math Math))
(relation Graft (i64 i64 Math))
(ruleset probe)
(rule ((Graft i j root) (= root (Add l r))
       (= l (Mul k u)) (= r (Mul k v)))
      ((Hit i j k u v)) :ruleset probe)
'''
    grafts = '\n'.join(f'(Graft {i} {j} (Add c{i} c{j}))' for i in range(4) for j in range(4))
    native(binary, out/'native-boundary', env+probe, inputs, grafts+'\n(run-schedule (run probe))',
           ['Var', 'Add', 'Mul', 'Hit'])
    observed = Graph(out/'native-boundary/graph.json')
    hits = {(int(r['args'][0][1]), int(r['args'][1][1]),
             *(observed.label(v) for _, v in r['args'][2:]))
            for r in observed.rows if r['op'] == 'Hit'}
    expected_hits = {(r['left'], r['right'], r['factor'], r['u'], r['v']) for r in compact['records']}
    assert hits == expected_hits

    # Execute the ORIGINAL factoring rule too, then check every predicted target.
    checks_egg = '\n'.join(f'(check (= (Add c{r["left"]} c{r["right"]}) {r["target"]}))'
                           for r in compact['records'])
    full = env+'\n'+inputs+'\n(run-schedule (saturate (run)))\n'
    full += '\n'.join(f'(let graft_{i}_{j} (Add c{i} c{j}))' for i in range(4) for j in range(4))
    full += '\n(run 1)\n'+checks_egg+'\n'
    (out/'graft-check.egg').write_text(full)
    p = subprocess.run([str(binary), str(out/'graft-check.egg')], cwd=ROOT, capture_output=True, text=True)
    (out/'graft-check.log').write_text(p.stdout+p.stderr)
    assert p.returncode == 0, p.stderr

    def nodes(t):
        return 1 if t[0] == 'Var' else 1+sum(nodes(a) for a in t[1:])
    term_nodes, term_index = [], {}
    def term_intern(t):
        if t not in term_index:
            encoded = [t[0], t[1]] if t[0] == 'Var' else [t[0], *(term_intern(a) for a in t[1:])]
            term_index[t] = len(term_nodes)
            term_nodes.append(encoded)
        return term_index[t]
    term_roots = [[term_intern(t) for t in c] for c in flat]
    term_bytes = len(json.dumps(dict(nodes=term_nodes,roots=term_roots),separators=(',', ':')).encode())
    packed_bytes = len(json.dumps(dict(nodes=family.nodes, roots=roots), separators=(',', ':')).encode())
    flat_bytes = len(json.dumps(flat, separators=(',', ':')).encode())
    report = dict(scope='standalone exact CPU factorization/matrix prototype; native egglog correctness oracle; not GPU or runtime acceleration',
        cells=[sexpr(t) for t in seeds], candidates_per_cell=[len(c) for c in flat],
        expanded_tree_nodes=sum(nodes(t) for c in flat for t in c), shared_dag_nodes=len(family.nodes),
        ordinary_term_hashcons_nodes=len(term_nodes), ordinary_term_hashcons_json_bytes=term_bytes,
        factorized_json_bytes=packed_bytes, flat_json_bytes=flat_bytes,
        compact_full_candidates_enumerated=0, validation_candidates_enumerated=sum(map(len, flat)),
        naive_candidate_pairs=sum(map(len, flat))**2,
        full_root_binding_records=sum(map(len, projections)), native_distinct_bindings=len(hits),
        positive_cell_pairs=sum(v>0 for row in compact['scores'] for v in row),
        closure_checks=checks, complete_native_term_family=True,
        brute_force_equal=True, native_binding_equal=True,
        native_graft_checks=len(compact['records']), **compact,
        provenance=dict(rules_sha256=hashlib.sha256(env.encode()).hexdigest(),
                        native_binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
                        head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),
                        kernel_changes_preserved=True))
    (out/'report.json').write_text(json.dumps(report, indent=2)+'\n')
    (out/'candidates.json').write_text(json.dumps([[sexpr(t) for t in c] for c in flat],indent=2)+'\n')
    render(out, report, flat)
    return report


def render(out, report, flat):
    esc = html.escape
    def table(matrix, rownames, colnames):
        return '<table><tr><th></th>'+''.join(f'<th>{esc(str(c))}</th>' for c in colnames)+'</tr>'+''.join(
            '<tr><th>'+esc(str(name))+'</th>'+''.join(f'<td class="{ "yes" if x else "no" }">{x}</td>' for x in row)+'</tr>'
            for name,row in zip(rownames,matrix))+'</table>'
    names = [f'C{i}' for i in range(4)]
    cards = ''.join(f'<article><h3>C{i}: {esc(x)} × ({esc(a)} + {esc(b)})</h3>'
        f'<pre>F = Mul({x}, Add({a},{b}) | Add({b},{a}))\n  | Mul(Add({a},{b}) | Add({b},{a}), {x})\n\n'
        f'P = Mul({x},{a}) | Mul({a},{x})\nQ = Mul({x},{b}) | Mul({b},{x})\nE = Add(P,Q) | Add(Q,P)\n\nSpines = F | E\n数量 = 2×2 + 2×2×2 = 12</pre>'
        '<details><summary>验证专用：展开这 12 个 Spine</summary><pre>'+esc('\n'.join(map(sexpr,flat[i])))+'</pre></details></article>'
        for i,(x,a,b) in enumerate(CELLS))
    body = '''<!doctype html><html lang="zh"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>因子化 Spine · 不展开的边界矩阵</title><style>
body{font:16px system-ui;background:#f5f7fa;color:#172d43;max-width:1120px;margin:30px auto;padding:0 20px;line-height:1.7}
section,article{background:white;border:1px solid #dae2ea;border-radius:12px;padding:20px;margin:16px 0}
.grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(330px,1fr));gap:16px}pre{background:#eef3f7;padding:16px;overflow:auto;font-size:13px}
table{border-collapse:collapse;width:100%;font-size:13px}th,td{padding:10px;border:1px solid #dce4eb;text-align:center}.yes{background:#d9f2e6}.no{color:#8998a8}
button{cursor:pointer;padding:9px 16px}select{padding:8px}small{color:#607080}summary{cursor:pointer}code{color:#137468}</style>
<h1>多个 Cell，共享 Spine，不展开计算边界交互</h1>
<p>四条真实 math microbenchmark 规则。独立 CPU 矩阵原型＋原生 egglog 正确性对照；没有 GPU 性能结论，也没有接管生产 Bake。</p>
'''
    body += '<section><h2>1. 每个 Cell 有 12 个完整 Spine</h2><p>这是整个候选集合的无损 union/product DAG，不是算子频率摘要。| 表示替代选择；同一构造器的参数组合表示乘积。</p><div class="grid">'+cards+'</div></section>'
    body += f'<section><h2>2. 共享存储</h2><p>48 个 Spine 展开为 <b>{report["expanded_tree_nodes"]}</b> 个树节点；普通 term hashcons 为 <b>{report["ordinary_term_hashcons_nodes"]}</b> 个节点；因子化 DAG 为 <b>{report["shared_dag_nodes"]}</b> 个节点（含 Choice）。</p><p>紧凑 JSON：逐树 {report["flat_json_bytes"]} 字节 → 普通 hashcons {report["ordinary_term_hashcons_json_bytes"]} 字节 → 因子化 {report["factorized_json_bytes"]} 字节。因子化相比普通 hashcons 的额外存储收益在这个超小例子里有限。不是 RSS，也不是整个分析库大小。</p></section>'
    body += '<section><h2>3. 先声明真实的边界上下文</h2><pre>新 coarse fact：Add(root(Ci), root(Cj))\n原规则：Add(Mul(k,u), Mul(k,v)) → Mul(k, Add(u,v))</pre><p>没有外层 Add，仅有两个 Cell 共享 k，不算规则已启用。此例只处理 root Mul 交互；不是全部内部触点。</p><p>直接在 DAG 上计算 Mul(k,u) 投影：Choice 相加，构造器子选择数相乘。保留完整 (k,u) 元组，未展开完整 Spine。</p><h3>V：每个 Cell 提供各个公因子的候选数</h3>'
    body += table(report['V'],names,report['factor_columns'])
    body += '<p>完整投影 Q 保留 (k,u)；F 将它映射到公因子 k。K = F Fᵀ，V = QF，交互 S = QKQᵀ = VVᵀ。残余 u 不会丢失，具体 graft 从 Q 的联合 binding 恢复。</p><details><summary>查看 Q 和 binding 坐标</summary>'+table(report['Q'],names,['k='+k+'; u='+u for k,u in report['binding_columns']])+'</details></section>'
    body += '<section><h2>4. 不展开的矩阵结果 S = VVᵀ</h2>'+table(report['scores'],names,names)
    body += '<p>数值表示候选 Spine 对数量，不是不同规则 match 数，也不是 committed mutation。相同 binding 的多个 Spine 归为一条规则实例。</p><label>左 Cell <select id="left">'+''.join(f'<option>{n}</option>' for n in names)+'</select></label> <label>右 Cell <select id="right">'+''.join(f'<option>{n}</option>' for n in names)+'</select></label><pre id="explain"></pre></section>'
    body += f'<section><h2>5. 三层独立核对</h2><ul><li>48 个 Spine 各自重新 saturation，均与对应 Cell 完整闭包相同：13 e-node / 7 e-class。</li><li>穷举 2,304 个候选对，只用于验证，与矩阵逐项相同。</li><li>原生 egglog 实际查询得到 {report["native_distinct_bindings"]} 个不同 binding，与压缩投影完全一致；再用原始规则执行并检查目标。</li></ul><p>紧凑计算路径展开候选数：0。校验路径：48。两个阶段明确分开。</p><p><a href="report.json">完整结果</a> · <a href="compact.json">因子 DAG 与矩阵</a> · <a href="graft-check.egg">可执行 graft .egg</a></p></section>'
    data=json.dumps(dict(records=report['records'],scores=report['scores']))
    body += '<script>const data='+data+';function show(){let i=document.getElementById("left").selectedIndex,j=document.getElementById("right").selectedIndex;let rs=data.records.filter(r=>r.left===i&&r.right===j);document.getElementById("explain").textContent=`候选对 ${data.scores[i][j]}；不同 binding ${rs.length}\\n`+rs.map(r=>`k=${r.factor}\\nu=${r.u}\\nv=${r.v}\\n→ ${r.target}\\n该 binding 对应 ${r.spine_pairs} 对 Spine`).join("\\n\\n");}document.getElementById("left").onchange=show;document.getElementById("right").onchange=show;document.getElementById("right").selectedIndex=1;show();</script></html>'
    (out/'index.html').write_text(body)


if __name__ == '__main__':
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--output', type=Path, required=True)
    ap.add_argument('--skip-build', action='store_true')
    args = ap.parse_args()
    if not args.skip_build:
        subprocess.run(['cargo','build','--offline','--release','--example','native_math_baseline'], cwd=ROOT,check=True)
    result = run(args.output.resolve(), ROOT/'target/release/examples/native_math_baseline')
    print(json.dumps({k:result[k] for k in ['shared_dag_nodes','expanded_tree_nodes','factorized_json_bytes',
        'flat_json_bytes','scores','native_distinct_bindings','native_graft_checks']},indent=2))
