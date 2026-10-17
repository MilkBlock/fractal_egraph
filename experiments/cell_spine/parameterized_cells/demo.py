"""Native-audited parameter families, exact Metal execution, existing math graft.

No GPU speed claim: numerical recursion is a new standalone restricted path;
the math graft runs the existing independently validated Cell backend.
"""
import argparse
from collections import Counter
import hashlib
import html
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0,str(ROOT))
from experiments.cell_spine.factorized_boundary.demo import native
from experiments.cell_spine.parameterized_cells.compiler import FamilyProgram, source_expr, source_fact
from experiments.cell_spine.parameterized_cells.runtime import CPU, GPU, explore, materialize
from tools.bench.math_graph_equal import verify

HERE = Path(__file__).resolve().parent


def parse(path, destination):
    subprocess.run([ROOT/'target/release/examples/lhs_ast',path,destination],check=True)
    return json.loads(destination.read_text())


def probe(program):
    declarations = []; rules = []
    for t in program.templates:
        r = t['raw']; name = 'Observed'+str(t['id'])
        # Inputs/local definitions and every numeric RHS field are checked.
        fields = t['binding_names'][:]
        # Evaluate the original AST, independently of our lowered arithmetic.
        # Repeated fields remain repeated in this oracle, checking every path.
        def raw_fields(x):
            s = program.ast['signatures'][x['head']]
            for a,sort in zip(x['args'],s['input']):
                if sort=='i64': yield source_expr(a)
                else: yield from raw_fields(a)
        fields += list(raw_fields(r['rhs']))
        declarations.append('('+name+' '+' '.join('i64' for _ in fields)+')')
        root = 'observed-root'
        while root in t['binding_names']: root += '-root'
        body = ['(= '+root+' '+source_expr(r['lhs'])+')'] + [source_fact(f) for f in r['conditions']]
        rules.append('(rule ('+' '.join(body)+') (('+name+' '+' '.join(fields)+')) :ruleset observe)')
    return '(datatype Observation '+' '.join(declarations)+')\n(ruleset observe)\n'+'\n'.join(rules)


def seed_text(seeds):
    return '\n'.join('('+f+' '+' '.join(map(str,p))+')' for f,p in seeds)


def audit(out, program, seeds, result, binary):
    names = [n for n,s in program.ast['signatures'].items() if s['kind']=='constructor']
    hit_names = ['Observed'+str(t['id']) for t in program.templates]
    def decoded_fields(tree,output,offset):
        if tree[0]=='value': yield output[offset+tree[1]]
        else:
            for c in tree[2:]: yield from decoded_fields(c,output,offset)
    expected = set()
    for e in result['effects']:
        t = program.templates[e['template']]; offset=t['leaf_offset']
        expected.add((e['template'],tuple(e['output'][:offset])+tuple(decoded_fields(t['rhs'],e['output'],offset))))
    checks = []; observed = None
    for smooth in (False,True):
        label = 'with-smooth-fold' if smooth else 'recurrence-only'
        residual = '\n'.join(program.residual) if smooth else ''
        env = program.declarations+'\n'+'\n'.join(t['source'] for t in program.templates)+'\n'+residual
        src = out/label/'native'; restored = out/label/'compiled'
        native(binary,src,env+'\n'+probe(program),seed_text(seeds),
               '(run-schedule (saturate (run observe)))',names+hit_names)
        rows = json.loads((src/'graph.json').read_text())
        found = {(int(r['op'][len('Observed'):]),tuple(int(v) for _,v in r['args']))
                 for r in rows if r['op'].startswith('Observed')}
        assert found==expected,(label,found-expected,expected-found)
        if observed is not None: assert found==observed
        observed = found
        (src/'math.json').write_text(json.dumps([r for r in rows if r['op'] in names]))
        text = materialize(program,result,seeds)
        native(binary,restored,program.declarations+'\n'+residual,text,constructors=names)
        equal = verify(src/'math.json',restored/'graph.json')
        checks.append(dict(phase=label,logical_bindings=len(found),**equal))
    return dict(checks=checks,observed_templates=sorted({i for i,_ in observed}),
                meaning='logical native bindings, not counts of committed mutations')


def compact(result):
    return dict(instances=len(result['instances']),logical_bindings=len(result['effects']),
                child_occurrences=result['child_occurrences'],waves=result['waves'],pending=result['pending'],
                by_family=dict(Counter(f for f,_ in result['instances'])))


def expression(e):
    if e[0]=='slot': return 'p'+str(e[1])
    if e[0]=='lit': return str(e[1])
    return '('+expression(e[1])+' '+e[0]+' '+expression(e[2])+')'


def render(out, report, program):
    esc = lambda x: html.escape(str(x))
    cards = []
    for t in program.templates:
        guard = ' and '.join(map(expression,t['guards'])) or 'true'
        children = '\n'.join(c['family']+'('+', '.join(map(expression,c['parameters']))+') at RHS '+str(c['path'])
                             for c in t['children']) or 'terminal'
        cards.append('<article><h3>'+esc(t['name'])+'</h3><pre>'+esc(t['source'])+'</pre><p>条件：'+esc(guard)+
                     '</p><pre>'+esc(children)+'</pre><details><summary>共享计算 DAG、参数与接线</summary><pre>'+esc(json.dumps({k:v for k,v in t.items() if k!='raw'},indent=2))+'</pre></details></article>')
    rows = []
    for p in report['phases']:
        r = p['summary']; c = p['audit']['checks'][-1]
        rows.append('<tr>'+''.join('<td>'+esc(v)+'</td>' for v in [p['name'],p['seeds'],r['instances'],r['logical_bindings'],r['child_occurrences'],c['enodes'],c['eclasses'],bool(p['gpu'])])+'</tr>')
    text = '''<!doctype html><html lang="zh"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>参数化 Cell 重复方式</title>
<style>body{font:16px system-ui;line-height:1.8;background:#f3f6fa;color:#193248;max-width:1100px;margin:30px auto;padding:0 20px}section,article{background:white;border:1px solid #dce5ee;border-radius:12px;padding:20px;margin:16px 0}pre{background:#eef3f7;padding:14px;overflow:auto;font-size:13px}table{border-collapse:collapse;width:100%;font-size:14px}td,th{border:1px solid #dce5ee;padding:8px}a{color:#146c9d}</style>
<h1>Cell 重复：保留条件、参数更新、子问题连接</h1><section><p>输入名字相同不是数值相同；共享一步程序也不代表完整闭包相同。这里直接从原规则提取参数计算。</p><pre>规则 AST + 原生逻辑匹配证据
  ↓ 保留算术、条件与每个 RHS 位置
共享参数 DAG + 子 Cell 接线
  ↓ 同一程序，换数值批量执行
Metal 精确整数计算 → CPU 合并相同子问题 → 下一批</pre><p>CPU 负责去重、接续与保留 effect；数值更新和条件在 GPU 上执行。没有深度截断，也不把观察到重复当作已证明无限递归。</p></section>
<section><h2>原生核对</h2><table><tr><th>阶段</th><th>输入</th><th>实例</th><th>逻辑绑定</th><th>子问题引用</th><th>e-node</th><th>e-class</th><th>GPU 一致</th></tr>'''+''.join(rows)+'''</table><p>分别在不执行 Add 常量折叠，以及执行原生 Add 常量折叠后，比较完整事实及等价分区。还原程序不包含任何递归 rewrite，不能用原规则补出遗漏。</p></section>
<section><h2>自动提取的模板</h2>'''+''.join(cards)+'''</section><section><h2>跨规则接续</h2><pre>'''+esc(json.dumps(report['composition'],indent=2))+'''</pre><p>保留前后两条 rewrite 的 effect。合成的参数更新不允许删除中间节点。</p></section>
<section><h2>支持范围</h2><p>这是独立实验，尚未接入通用 egglog runtime。数值路径支持 i64 字段在 int32 精确范围内的加减乘和比较；越界标为 pending。一般函数表、set、外部隐藏 union 和任意邻居发现仍未覆盖。原有 math graft 的矩阵后端单独回归，不冒充已经和数值路径融成一个完整 GPU 引擎。</p></section>
<p><a href="report.json">完整证据</a> · <a href="families.json">参数模板</a> · <a href="rules.egg">原始规则</a> · <a href="held-out-effects.egg">新数值的全部保留 effect</a></p></html>'''
    (out/'index.html').write_text(text)


def run(out, gpu=True, skip_build=False, math=True):
    out.mkdir(parents=True,exist_ok=False)
    if not skip_build:
        subprocess.run(['cargo','build','--offline','--release','--example','lhs_ast','--example','native_math_baseline'],cwd=ROOT,check=True)
    source = (HERE/'rules.egg').read_text(); (out/'rules.egg').write_text(source)
    ast = parse(out/'rules.egg',out/'ast.json'); program = FamilyProgram(ast)
    binary = ROOT/'target/release/examples/native_math_baseline'
    backend = None
    if gpu:
        executable = out/'parameter-gpu'
        subprocess.run(['swiftc','-O',HERE/'gpu.swift','-o',executable],check=True)
        backend = GPU(program,executable)
    phases = []; allowed = None
    try:
        for name,seeds in [('bake',[('Fib',[5]),('StepA',[6]),('StepB',[6])]),
                           ('held-out',[('Fib',[8]),('StepA',[11]),('StepB',[10])]),
                           ('shared-subproblems',[('Fib',[9]),('Fib',[8]),('Fib',[5]),('StepA',[3]),('StepB',[2])])]:
            result = explore(program,seeds,CPU(program),allowed)
            assert result['complete']
            checked = audit(out/name,program,seeds,result,binary)
            if allowed is None: allowed = set(checked['observed_templates'])
            gpu_info = None
            if backend:
                device_result = explore(program,seeds,backend,allowed)
                assert device_result==result,'CPU/GPU parameter DAG disagreement'
                gpu_info = dict(device=backend.device,exact_equal=True)
            phases.append(dict(name=name,seeds=seeds,summary=compact(result),audit=checked,gpu=gpu_info))
            (out/(name+'-effects.egg')).write_text(program.declarations+'\n'+materialize(program,result,seeds))
            (out/(name+'-dag.json')).write_text(json.dumps(result,indent=2))
    finally:
        if backend: backend.close()
    # Choose an actual cross-family pair from the discovered family graph.
    first = next(t for t in program.templates if any(c['family']!=t['family'] for c in t['children']))
    child = first['children'][0]
    second = next(t for t in program.templates if t['family']==child['family'] and t['children'])
    composition = program.compose(first['id'],0,second['id'])
    math_report = None
    if math:
        from experiments.cell_spine.transition_program.demo import run as math_run
        m = math_run(out/'math-graft',gpu=gpu,skip_build=True)
        math_report = dict(native=m['native']['matches'],heldout=m['heldout']['native']['matches'],gpu=m['gpu'],
                           note='existing shared-moment Cell backend; independently audited, not the numeric interpreter')
    report = dict(scope='standalone parameterized Cell family prototype; not generic egglog acceleration',
                  phases=phases,composition=composition,recursive_components=program.recursive_components(),
                  baked_templates=sorted(allowed),math_graft=math_report,
                  gpu_dispatches=backend.dispatches if backend else 0,
                  provenance=dict(base_head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),
                                  source_sha256=hashlib.sha256(source.encode()).hexdigest(),environment=program.environment,
                                  implementation_sha256={n:hashlib.sha256((HERE/n).read_bytes()).hexdigest()
                                                         for n in ('compiler.py','runtime.py','gpu.swift','demo.py')},
                                  native_binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest()))
    (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    (out/'families.json').write_text(json.dumps(program.report(),indent=2)+'\n')
    render(out,report,program)
    return report


if __name__=='__main__':
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--output',type=Path,required=True)
    ap.add_argument('--skip-build',action='store_true')
    ap.add_argument('--cpu-only',action='store_true')
    ap.add_argument('--skip-math',action='store_true',help='reuse the separately tested math backend without rerunning it')
    a = ap.parse_args(); r = run(a.output.resolve(),not a.cpu_only,a.skip_build,not a.skip_math)
    print(json.dumps({k:v for k,v in r.items() if k!='math_graft'},indent=2))
