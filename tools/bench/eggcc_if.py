#!/usr/bin/env python3
"""Unmodified eggcc If min/max guards, frozen Cell library, fresh sparse IR graphs."""
import argparse
import copy
import hashlib
import html
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time
import tomllib

from math_virtual import ROOT, measure
from math_graph_equal import verify
sys.path.insert(0,str(ROOT))
from tools.cell_spine.compiler import commands, cellify
from tools.cell_spine.markers import RUNTIME_SCHEDULE
from tools.cell_spine.terms import actions, text, walk

SOURCE=ROOT/'experiments/cell_spine/eggcc_if_tiny/source.egg'
RUNNER=ROOT/'target/release/examples/native_math_baseline'
sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()


def build_pristine(out, baseline):
    expected=subprocess.check_output(['git','rev-parse','egglog-baseline^{}'],cwd=ROOT,text=True).strip()
    assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=baseline,text=True).strip()==expected
    assert not subprocess.check_output(['git','status','--porcelain','--','egglog'],cwd=baseline)
    folder=out/'pristine-build';(folder/'src').mkdir(parents=True)
    module=ROOT/'examples/support/phased_bench.rs'
    (folder/'src/main.rs').write_text(f'#[path={json.dumps(str(module))}] mod phases;\nfn main() {{ let a:Vec<String>=std::env::args().collect(); phases::run(&a[2],a.get(3).map(String::as_str)); }}\n')
    (folder/'Cargo.toml').write_text('[package]\nname="native_compare"\nversion="0.1.0"\nedition="2024"\n[workspace]\n[dependencies]\n'
        +f'egglog={{path={json.dumps(str(baseline/"egglog"))},default-features=false,features=["graphviz"]}}\n'
        +'serde={version="1",features=["derive"]}\nserde_json="1"\n[profile.release]\nlto="thin"\ncodegen-units=1\n[build-dependencies]\nsyn={version="2",features=["full"]}\n')
    (folder/'build.rs').write_text('fn main() {}\n')
    lock=ROOT/'experiments/native_compare/Cargo.lock';shutil.copyfile(lock,folder/'Cargo.lock')
    subprocess.run(['cargo','build','--offline','--release','--manifest-path',folder/'Cargo.toml','--target-dir',ROOT/'target/native-compare-baseline'],cwd=ROOT,check=True)
    pins=lambda p:{(x['name'],x['version'],x.get('source')) for x in tomllib.loads(p.read_text())['package'] if 'source' in x}
    assert pins(folder/'Cargo.lock')<=pins(lock),'pristine registry versions drifted'
    binary=folder/'pristine';shutil.copyfile(ROOT/'target/native-compare-baseline/release/native_compare',binary);binary.chmod(0o755)
    return binary,dict(commit=expected,source_clean=True,shared_harness_sha256=sha(module),binary_sha256=sha(binary),registry_versions_pinned=True)


def make_input(data, count, period):
    templates=[a['action']['expr'] for a in data['actions']]
    assert len(templates)==3
    memo, initial, kinds = {},[],[]
    def intern(e):
        if e['kind']!='call':return e
        args=[intern(a) for a in e['args']]
        node=dict(e,args=args);key=text(node)
        if key not in memo:
            name=f'node_{len(memo)}';memo[key]=name
            initial.append(dict(kind='let',name=name,expr=node))
        return dict(kind='var',name=memo[key])
    hit=0
    for i in range(count):
        kind=hit%2 if i%period==0 else 2
        if kind!=2:hit+=1
        kinds.append(kind)
        e=copy.deepcopy(templates[kind])
        for node in walk(e):
            if node['kind']=='call' and node['head']=='InFunc':node['args'][0]['text']=json.dumps(f'heldout-if-{i}')
        intern(e)
    return initial,dict(min=kinds.count(0),max=kinds.count(1),miss=kinds.count(2))


def check_branches(graph, count, period):
    """Independently check each expected branch result, including near misses."""
    rows=json.loads(Path(graph).read_text())
    index={(r['op'],tuple(v for _,v in r['args'])):r['result'] for r in rows}
    by_result={}
    for r in rows:by_result.setdefault((r['op'],r['result']),r)
    seen=set();counts=dict(min=0,max=0,miss=0)
    for row in rows:
        if row['op']!='If':continue
        pred,inputs,_,_= [v for _,v in row['args']]
        context=by_result['Arg',inputs]['args'][1][1]
        name=by_result['InFunc',context]['args'][0][1]
        n=int(name.removeprefix('heldout-if-'));seen.add(n)
        output=index.get(('Get',(row['result'],'0')))
        if n%period:
            assert output is None, 'near-miss If unexpectedly produced an output'
            counts['miss']+=1
        else:
            kind='min' if (n//period)%2==0 else 'max'
            op=index[('Smin' if kind=='min' else 'Smax',())]
            a=index['Get',(inputs,'0')];b=index['Get',(inputs,'1')]
            assert output==index['Bop',(op,a,b)], 'wrong min/max equivalence'
            counts[kind]+=1
    assert seen==set(range(count)), 'missing If instances'
    return counts


def render(report,out):
    lines=['# eggcc If 超小子集：新图单次 bench','',
           '使用 If→min、If→max、Single→Get 三条真实规则；仅合并 ruleset 标签。原有索引、分支上下文及类型连接没有删除。',
           '单个训练程序含 min/max/不匹配各一个；测试换新 InFunc 名字，不重新 Bake。输入按 DAG 的 let 节点写出，避免重复展开源码。','',
           '| 分布/If 数 | 变体 | 安装 ms | 输入 ms | 推理 ms | 导出 ms | 总 ms | 峰值 RSS MiB |',
           '|---|---|---:|---:|---:|---:|---:|---:|']
    rows=[]
    for c in report['cases']:
        for mode,m in c['timings'].items():
            r=m['result'];cols=[f'{c["distribution"]}/{c["ifs"]}',mode]+[f'{r[k]*1000:.3f}' for k in ('setup_seconds','input_seconds','run_seconds','export_seconds','seconds')]+[f'{m["peak_rss_bytes"]/2**20:.2f}']
            lines.append('| '+' | '.join(cols)+' |');rows.append('<tr>'+''.join('<td>'+html.escape(x)+'</td>' for x in cols)+'</tr>')
    lines+=['','## 图规模与稀疏度','','| 分布/If 数 | min/max/未命中 | Expr 节点/类 | 全 sort 节点/类 | 原生 match | 净新增节点/match | 活跃/总 Summary | 前端 ms |','|---|---|---:|---:|---:|---:|---:|---:|']
    for c in report['cases']:
        v=c['verification'];e=v['sorts']['Expr'];d=c['shapes'];m=c['timings']['native']['result']['matches']
        added=v['enodes']-c['initial']['enodes']
        lines.append(f'| {c["distribution"]}/{c["ifs"]} | {c["kinds"]["min"]}/{c["kinds"]["max"]}/{c["kinds"]["miss"]} | {e["enodes"]}/{e["eclasses"]} | {v["enodes"]}/{v["eclasses"]} | {m} | {added/m:.3f} | {d["active_summaries"]}/{d["active_summaries"]+d["subsumed_summaries"]} | {c["cellify_seconds"]*1000:.3f} |')
    lines+=['','## 口径','',
      '- `spine` 停在紧凑图；`spine_export` 完整展开。另一个验证进程比较每张表的 typed 行和双射 e-class，包含 Expr 以外的全部 sort。',
      '- min/max/未命中是合成 IR 的结构条件，不是 Free Join 探针计数。净新增节点/match 是最终行差额除逻辑匹配数，不是逐次已提交 mutation。',
      '- 每项只运行一次；计时进程不扫描完整图、不生成 SVG、不采集 trace。RSS 包含已安装库。',
      '- 1000 个 If 的输入包含约 14,000 个命名 let；输入时间与 RSS 含这些前端/全局绑定开销，不能当作纯节点存储成本。',
      '- 前端转换从已有 AST 开始，另计；输入阶段含 parser 和 let/fact 插入。Bake、Gurobi、训练验证、构建不计入引擎执行时间。',
      '- 只覆盖六个 equality sort 的正向 constructor/equality 子集，不是完整 eggcc pass；没有带上类型分析、subst、set/merge、循环或其他 If 优化。',
      f'- 训练/求解/验证合计 {report["training"]["measurement"]["observed_wall_seconds"]:.3f} s；库 {report["training"]["library_bytes"]:,} B；{report["training"]["families"]} 个 Cell 族；{report["training"]["consumer_rules"]} 条消费者规则。',
      ('- 当前内核和 pristine 使用同一份 phased Rust harness、单线程和 System allocator；pristine 源码与 immutable tag 验证一致。' if report['pristine'] else '- 本次未运行 pristine；native 对照为同一当前内核。')]
    (out/'README.md').write_text('\n'.join(lines)+'\n')
    (out/'index.html').write_text('<!doctype html><meta charset="utf-8"><title>eggcc If Cell bench</title><style>body{font:16px system-ui;color:#173047;margin:30px auto;max-width:1200px}table{border-collapse:collapse;width:100%}td,th{padding:10px;border-bottom:1px solid #dde3ed;text-align:right}pre{white-space:pre-wrap;background:#f4f6f9;padding:18px}</style><h1>eggcc If：Cell 建立稀疏新图</h1><p>真实 min/max 规则，保留多 sort 和完整条件。单位 ms / MiB。每项单次测量。</p><table><tr>'+''.join('<th>'+x+'</th>' for x in ['分布/If数','模式','安装','输入','推理','导出','总计','RSS'])+'</tr>'+''.join(rows)+'</table><pre>'+html.escape('\n'.join(lines[lines.index('## 图规模与稀疏度'):]))+'</pre><a href="report.json">完整数据</a>')


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True)
    p.add_argument('--ifs',type=int,nargs='+',default=[10,1000]);p.add_argument('--skip-build',action='store_true')
    p.add_argument('--baseline',type=Path,default=Path('/Users/mineralsteins/.codex/worktrees/native-baseline/egg_layout'))
    p.add_argument('--no-pristine',action='store_true');a=p.parse_args()
    if any(n<1 for n in a.ifs):p.error('positive if counts required')
    out=a.output.resolve();out.mkdir(parents=True,exist_ok=False);os.environ['RAYON_NUM_THREADS']='1'
    if not a.skip_build:subprocess.run(['cargo','build','--offline','--release','--bin','egg_layout','--example','native_math_baseline','--example','spine_validate','--example','lhs_ast'],cwd=ROOT,check=True)
    pristine,provenance=build_pristine(out,a.baseline.resolve()) if not a.no_pristine else (None,None)
    train=out/'bake';measurement=measure([sys.executable,'-m','tools.cell_spine.compiler','--source',SOURCE,'--mode','symbolic','--output',train,'--skip-build'],out/'training')
    compiled=json.loads((train/'report.json').read_text());assert compiled['status']=='compiled'
    data=json.loads((train/'work/input.json').read_text());library=(train/'library.egg').read_text()
    eq_sorts=sorted({s['output'] for s in data['signatures'].values()})
    report=dict(schema='eggcc-if-cell-bench/v1',commit=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),
        dirty=bool(subprocess.check_output(['git','status','--porcelain'],cwd=ROOT)),binary_sha256=sha(RUNNER),pristine=provenance,threads=1,
        source_sha256=sha(SOURCE),eq_sorts=eq_sorts,training=dict(measurement=measurement,library_bytes=len(library.encode()),library_sha256=sha(train/'library.egg'),families=len(compiled['plan']['selected']),consumer_rules=len(compiled['transitions'])),cases=[])
    def save():(out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    save()
    for distribution,period in [('all_match',1),('sparse',10)]:
        for count in a.ifs:
            folder=out/f'{distribution}-{count}';folder.mkdir()
            initial,kinds=make_input(data,count,period)
            environment='\n'.join(commands(data,{'declaration','rule'}))+'\n'
            (folder/'native-env.egg').write_text(environment);(folder/'native-input.egg').write_text('\n'.join(actions(initial))+'\n')
            tick=time.perf_counter()
            compact,export=cellify(dict(signatures=data['signatures'],actions=[{'action':x} for x in initial],checks=[]),library)
            run=RUNTIME_SCHEDULE+'\n';body=compact.removeprefix(library);assert body.endswith(run)
            (folder/'spine-input.egg').write_text(body.removesuffix(run));lowering=time.perf_counter()-tick
            manifests=dict(native=dict(environment=str(folder/'native-env.egg'),input=str(folder/'native-input.egg'),run='(run-schedule (saturate if-tiny))'),spine=dict(environment=str(train/'library.egg'),input=str(folder/'spine-input.egg'),run=run))
            manifests['spine_export']=dict(manifests['spine'],export=export)
            for mode,m in manifests.items():(folder/f'{mode}.json').write_text(json.dumps(dict(m,constructors=sorted(data['signatures']))))
            ref=measure([RUNNER,'bench',folder/'native.json',folder/'native-graph.json'],folder/'verify-native')
            encoded=measure([RUNNER,'bench',folder/'spine_export.json',folder/'spine-graph.json'],folder/'verify-spine')
            equality=verify(folder/'native-graph.json',folder/'spine-graph.json',eq_sorts)
            assert check_branches(folder/'native-graph.json',count,period)==kinds
            assert check_branches(folder/'spine-graph.json',count,period)==kinds
            if pristine:
                measure([pristine,'bench',folder/'native.json',folder/'pristine-graph.json'],folder/'verify-pristine')
                verify(folder/'pristine-graph.json',folder/'native-graph.json',eq_sorts)
            assert ref['result']['matches']==2*count+kinds['min']+kinds['max']
            initial_shape=ref['result']['verification']['initial_original']
            case=dict(distribution=distribution,ifs=count,kinds=kinds,branch_results_verified=True,initial=initial_shape,verification=equality,shapes=encoded['result']['verification'],cellify_seconds=lowering,timings={})
            for mode in ('native','spine','spine_export'):
                m=measure([RUNNER,'bench',folder/f'{mode}.json'],folder/f'time-{mode}')
                assert m['result']['matches']==(ref if mode=='native' else encoded)['result']['matches']
                case['timings'][mode]=m
            if pristine:case['timings']['pristine']=measure([pristine,'bench',folder/'native.json'],folder/'time-pristine')
            report['cases'].append(case);save();print(distribution,count,{k:round(v['result']['seconds']*1000,3) for k,v in case['timings'].items()},flush=True)
    assert sha(train/'library.egg')==report['training']['library_sha256']
    render(report,out);print(out/'index.html')


if __name__=='__main__':main()
