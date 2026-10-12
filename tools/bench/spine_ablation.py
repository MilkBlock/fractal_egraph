#!/usr/bin/env python3
"""Native old/marker-only/joint ablation on one frozen Bake catalog per input.

Reuses a joint_pricing run, but re-emits every library and revalidates every
benchmark graph. No recapture of history for individual variants. Times each
mode once; graph scans and per-rule diagnostics are in separate processes.
"""
import argparse
import copy
import csv
import hashlib
import html
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import time

from math_virtual import ROOT,measure
from math_graph_equal import verify
from eggcc_if import make_input
sys.path.insert(0,str(ROOT))
from tools.cell_spine import compiler as c,markers
from tools.cell_spine.pricing import RuntimePricing,audit,cost
from tools.cell_spine.terms import actions,walk
from experiments.cell_spine.joint_pricing.run import fingerprint

RUNNER=ROOT/'target/release/examples/native_math_baseline'
REFERENCE=ROOT/'experiments/cell_spine/joint_pricing/reference.json'
sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()


def clone_inputs(data,n):
    """Fresh String-bound instances; keep primitive numeric rule conditions."""
    original=[a['action'] for a in data['actions']];result=[]
    for i in range(n):
        mapping={a['name']:f'instance_{i}_{a["name"]}' for a in original if a['kind']=='let'}
        for action in copy.deepcopy(original):
            if action['kind']=='let':action['name']=mapping[action['name']]
            roots=[action['expr']] if action['kind'] in ('expr','let') else action['args']
            for root in roots:
                for expr in walk(root):
                    if expr['kind']=='var':expr['name']=mapping[expr['name']]
                    elif expr['kind']=='literal' and expr['sort']=='String':
                        expr['text']=json.dumps(f'fresh-{i}/'+json.loads(expr['text']))
            result.append(action)
    return result


def prepare_variants(bake,reference,dest):
    saved=json.loads((bake/'report.json').read_text())
    source=ROOT/reference['source']
    if sha(source)!=reference['source_sha256'] or saved['source_sha256']!=sha(source):
        raise ValueError('Bake source mismatch')
    analysis=Path(saved['analysis'])
    if (analysis/'source.egg').read_text().strip()!=source.read_text().strip():
        raise ValueError('captured source mismatch')
    work=dest/'prepare';work.mkdir(parents=True)
    cs,rejected=c.load_candidates(analysis,work)
    for path in cs:path['marker_layouts']=markers.layouts(path)
    edges=c.transition_options(cs);c.price_options(edges,work)
    old=reference['plan'];joint={k:saved['plan'][k] for k in ('selected','implementations','marker_layouts')}
    paths=[dict(id=p['id'],family=p['family'],nodes=p['nodes'],lhs_atoms=p['lhs_atoms']) for p in cs]
    selected=[p for p in cs if p['id'] in old['selected']]
    ls={str(p['family']):next(l for l in p['marker_layouts'] if l['id'] in old['marker_layouts']) for p in selected}
    unique={}
    for e in edges:
        for o in e['options']:
            if o['id'] in old['implementations']:
                unique.setdefault((tuple(o['body']),tuple(o['head'])),{k:o[k] for k in ('id','body','head','requires')})
    for name,value in [('paths',paths),('layouts',ls),('implementations',list(unique.values()))]:
        if fingerprint(value)!=reference[name+'_sha256']:raise ValueError(f'frozen {name} changed')
    data=c.ast(source,work/'input.json');sig=dict(data['signatures']);env=c.commands(data,{'declaration','rule'})
    for path in cs:
        for name,s in path['signatures'].items():
            if name not in sig:
                env.append(f'(constructor {name} ({" ".join(s["input"])}) {s["output"]})');sig[name]=s
    oracle=RuntimePricing(lambda plan,folder:c.prepare_runtime(cs,plan,edges,work,folder),sig,work/'pricing',(1,1,1))
    same_paths=oracle.key(old)==oracle.key(joint)
    if same_paths:
        fixed=dict(joint)
        basis='same path/implementation assignment as supplied joint optimum; reuse its marker optimum'
    else:
        _,value=oracle.optimize(old)
        fixed=dict(old,marker_layouts=value['marker_layouts'])
        basis='fixed old path/implementation assignment, exact marker subproblem solved'
    variants={};metadata={}
    for name,plan in [('old',old),('marker_only',fixed),('joint',joint)]:
        folder=dest/name;folder.mkdir()
        bundle=c.prepare_runtime(cs,plan,edges,work,folder)
        lib,chosen,_,retirement=c.make_library(cs,plan,edges,env,sig,folder,bundle)
        layouts={f:next(l for l in p['marker_layouts'] if l['id'] in plan['marker_layouts']) for f,p in chosen.items()}
        priced=oracle.prices(bundle,layouts)
        rules=json.loads((folder/'marker-runtime.json').read_text())['rules'];audit(rules,priced)
        metrics={k:sum(cost(r)[k] for r in rules) for k in cost(rules[0])}
        shapes=len({n for p in chosen.values() for n in p['nodes']});columns=sum(l['columns'] for l in layouts.values())
        score=shapes+columns+metrics['search_atoms']+metrics['update_actions']
        if name=='joint':
            if score!=saved['plan']['objective'] or lib!=(bake/'library.egg').read_text():
                raise ValueError('joint Bake program/cost changed; rerun joint_pricing before benchmarking')
        (folder/'library.egg').write_text(lib)
        variants[name]=folder/'library.egg'
        metadata[name]=dict(plan=plan,score=score,template_shapes=shapes,marker_columns=columns,runtime_cost=metrics,
            runtime_rules=len(rules),library_bytes=len(lib.encode()),library_lines=len(lib.splitlines()),library_sha256=sha(folder/'library.egg'),
            rules={r['name']:cost(r) for r in rules},cost_audit='exact')
    if metadata['marker_only']['score']>metadata['old']['score'] or metadata['joint']['score']>metadata['marker_only']['score']:
        raise ValueError('nested ablation optimum is inconsistent')
    return data,variants,dict(variants=metadata,marker_only_basis=basis,rejected_paths=rejected,
        bake_report_sha256=sha(bake/'report.json'),bake_analysis=str(analysis),joint_bound=saved['plan']['bound'])


def manifests(data,initial,variants,folder):
    folder.mkdir();env='\n'.join(c.commands(data,{'declaration','rule'}))+'\n'
    (folder/'native-env.egg').write_text(env);(folder/'native-input.egg').write_text('\n'.join(actions(initial))+'\n')
    schedule=c.commands(data,{'schedule'})
    if len(schedule)!=1:raise ValueError('one schedule expected')
    common=dict(constructors=sorted(data['signatures']))
    result={'native':dict(common,environment=str(folder/'native-env.egg'),input=str(folder/'native-input.egg'),run=schedule[0])}
    for name,path in variants.items():
        lib=path.read_text()
        tick=time.perf_counter()
        runtime,export=c.cellify(dict(signatures=data['signatures'],actions=[{'action':a} for a in initial],checks=[]),lib)
        body=runtime.removeprefix(lib);run=markers.RUNTIME_SCHEDULE+'\n'
        if not body.endswith(run):raise ValueError('runtime schedule drift')
        (folder/f'{name}-input.egg').write_text(body.removesuffix(run))
        (folder/f'{name}-runtime.egg').write_text(runtime)
        m=dict(common,environment=str(path),input=str(folder/f'{name}-input.egg'),run=run)
        result[name]=m;result[name+'_export']=dict(m,export=export)
        result[name]['cellify_seconds']=time.perf_counter()-tick
    for mode,m in result.items():(folder/f'{mode}.json').write_text(json.dumps(m,indent=2)+'\n')
    return result


def run_case(fixture,label,data,initial,variants,metadata,folder):
    ms=manifests(data,initial,variants,folder)
    eq_sorts=sorted({s['output'] for s in data['signatures'].values()})
    ref=measure([RUNNER,'bench-probes',folder/'native.json',folder/'native-graph.json'],folder/'verify-native')
    checked={'native':ref['result']};shapes={};equivalence={}
    for name in variants:
        m=measure([RUNNER,'bench-probes',folder/f'{name}_export.json',folder/f'{name}-graph.json'],folder/f'verify-{name}')
        equivalence[name]=verify(folder/'native-graph.json',folder/f'{name}-graph.json',eq_sorts)
        v=m['result']['verification']
        if v['original_before_export']['enodes']!=0:raise ValueError('original facts materialized during Cell inference')
        if set(v['rule_matches'])-set(metadata['variants'][name]['rules']):raise ValueError('unexpected rule executed')
        if v['spine']['enodes']!=v['raw_spine']['enodes']+v['canonical_spine']['enodes']:
            raise ValueError('raw/canonical count mismatch')
        # Diagnostic frequency-weighted syntax, not a Join/probe/mutation count.
        weighted={k:sum(n*metadata['variants'][name]['rules'][r][k] for r,n in v['rule_matches'].items())
                  for k in ('search_atoms','update_actions')}
        shapes[name]=dict(v,frequency_weighted_syntax=weighted)
        checked[name]=checked[name+'_export']=m['result']
    timings={}
    for mode in ms:
        measured=measure([RUNNER,'bench',folder/f'{mode}.json'],folder/f'time-{mode}')
        if measured['result']['matches']!=checked[mode]['matches']:raise ValueError('timed/verified match count changed')
        timings[mode]=measured
    return dict(fixture=fixture,label=label,verification=equivalence,shapes=shapes,
                native_shape=ref['result']['verification']['restored'],
                native_storage=ref['result']['verification'],
                native_probes=ref['result']['verification']['run_probe_counters'],timings=timings,
                cellify_seconds={n:ms[n]['cellify_seconds'] for n in variants})


def render(report,out):
    lines=['# Spine / marker 联合选择：原生单次消融','',
        '每个输入使用同一份历史、同一份 Cell 候选。old 固定旧选择；marker_only 只改 marker；joint 使用联合选择。',
        'native 是同一当前内核的普通 egglog 执行，不是 pristine。每种模式计时一次，完整图比较另起进程。','',
        '| 输入 | 变体 | 库 KiB | 推理规则数 | 结构成本 |','|---|---|---:|---:|---:|']
    for name,t in report['training'].items():
        for mode,v in t['variants'].items():lines.append(f'| {name} | {mode} | {v["library_bytes"]/1024:.2f} | {v["runtime_rules"]} | {v["score"]} |')
    lines+=['','| 输入/规模 | 模式 | 安装 ms | 输入 ms | 推理 ms | 导出 ms | 总 ms | RSS MiB | 逻辑 match |',
            '|---|---|---:|---:|---:|---:|---:|---:|---:|']
    for case in report['cases']:
        for mode,m in case['timings'].items():
            r=m['result'];values=[f'{r[k]*1000:.3f}' for k in ('setup_seconds','input_seconds','run_seconds','export_seconds','seconds')]
            lines.append('| '+' | '.join([f'{case["fixture"]}/{case["label"]}',mode,*values,f'{m["peak_rss_bytes"]/2**20:.2f}',str(r['matches'])])+' |')
    lines+=['','| 输入/规模 | 模式 | 等效节点/类 | raw 节点 | canonical 节点 | summary 活跃/已 subsume | 全部物理表行 |',
            '|---|---|---:|---:|---:|---:|---:|']
    for case in report['cases']:
        for mode,v in case['shapes'].items():
            e=case['verification'][mode]
            lines.append(f'| {case["fixture"]}/{case["label"]} | {mode} | {e["enodes"]}/{e["eclasses"]} | {v["raw_spine"]["enodes"]} | {v["canonical_spine"]["enodes"]} | {v["active_summaries"]}/{v["subsumed_summaries"]} | {v["native_total_rows_before_export"]} |')
    if all('native_probes' in c for c in report['cases']):
        lines+=['','| 输入/规模 | 模式 | 索引句柄 | 索引枚举行 | 单键返回行 | 候选行合计 |',
                '|---|---|---:|---:|---:|---:|']
        for case in report['cases']:
            for mode,probes in [('native',case['native_probes'])]+[(n,v['run_probe_counters']) for n,v in case['shapes'].items()]:
                lines.append('| '+' | '.join([f'{case["fixture"]}/{case["label"]}',mode]+[str(probes[k]) for k in
                    ('probes','  candidate_rows enumerated (for_each)','  candidate_rows from lookups','candidate_rows')])+' |')
    lines+=['','口径：安装、输入、推理、完整导出分开；RSS 包括库与前端。spine 模式的等效图由独立完整导出证明。',
        '所有表行包括 let、辅助表和 subsumed 行；不是字节数。逻辑 match 不是成功写入，也不是搜索候选数。',
        '报告中的按 match 加权语法项只用于解释频繁路径；不能拿它当 Join 工作量。诊断进程另记录已有原生 probe counters。',
        '当前内核已有无条件 atomic 探针计数；各组使用相同内核，时间不是 pristine / 无插桩的生产基准。',
        'Bake/求解和本次准备耗时单列，不计入运行时加速。微秒级差异不能靠单次测量确认。',
        '所有生成程序仍限定在已 Bake 的转移；不能把本结果当作未见过拓扑的完整覆盖。']
    (out/'README.md').write_text('\n'.join(lines)+'\n')
    rows=[]
    for case in report['cases']:
        for mode,m in case['timings'].items():
            name=mode.removesuffix('_export');native=name=='native';r=m['result']
            v=case.get('native_storage',{}) if native else case['shapes'][name]
            probes=case.get('native_probes',{}) if native else v.get('run_probe_counters',{})
            rows.append(dict(fixture=case['fixture'],input=case['label'],mode=mode,
                equivalent_enodes=case['native_shape']['enodes'],equivalent_eclasses=case['native_shape']['eclasses'],
                **{k.replace('_seconds','_ms'):round(r[k]*1000,6) for k in ('setup_seconds','input_seconds','run_seconds','export_seconds')},
                total_ms=round(r['seconds']*1000,6),rss_mib=round(m['peak_rss_bytes']/2**20,6),logical_matches=r['matches'],
                raw_spine_enodes=v.get('raw_spine',{}).get('enodes'),canonical_spine_enodes=v.get('canonical_spine',{}).get('enodes'),
                active_summaries=v.get('active_summaries'),subsumed_summaries=v.get('subsumed_summaries'),
                all_table_rows_before_export=v.get('native_total_rows_before_export'),
                probes=probes.get('probes'),candidate_rows=probes.get('candidate_rows')))
    with (out/'results.csv').open('w') as f:
        writer=csv.DictWriter(f,fieldnames=list(rows[0]),lineterminator='\n');writer.writeheader();writer.writerows(rows)
    (out/'index.html').write_text('<!doctype html><meta charset="utf-8"><title>Spine ablation</title><style>body{max-width:1500px;margin:24px auto;font:15px system-ui}pre{white-space:pre-wrap;line-height:1.65}a{color:#245a9c}</style><h1>Spine / marker 原生消融</h1><p><a href="report.json">完整数据</a> · <a href="README.md">表格</a></p><pre>'+html.escape('\n'.join(lines))+'</pre>')


def diagnose_existing(out):
    """Add counters without repeating or replacing single-run timing evidence."""
    report=json.loads((out/'report.json').read_text())
    report['diagnostic_runner_sha256']=sha(RUNNER)
    for case in report['cases']:
        folder=out/case['fixture']/case['label']
        for mode in ('native','old','marker_only','joint'):
            manifest=folder/('native.json' if mode=='native' else mode+'_export.json')
            graph=folder/f'diagnostic-{mode}-graph.json'
            m=measure([RUNNER,'bench-probes',manifest,graph],folder/f'diagnostic-{mode}')
            if m['result']['matches']!=case['timings'][mode]['result']['matches']:
                raise ValueError('diagnostic/timing match drift')
            v=m['result']['verification']
            case.setdefault('diagnostic_measurements',{})[mode]=m
            if mode=='native':case['native_probes']=v['run_probe_counters'];case['native_storage']=v
            else:
                sorts=list(case['verification'][mode]['sorts'])
                if verify(folder/'diagnostic-native-graph.json',graph,sorts)!=case['verification'][mode]:
                    raise ValueError('diagnostic graph drift')
                for key in ('rule_matches','native_tables_before_export','spine'):
                    if v[key]!=case['shapes'][mode][key]:raise ValueError(f'diagnostic {key} drift')
                case['shapes'][mode]['run_probe_counters']=v['run_probe_counters']
        (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
        render(report,out)
    print(out/'index.html')


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--bake-root',type=Path,help='output of experiments/cell_spine/joint_pricing/run.py')
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--copies',type=int,default=1000)
    p.add_argument('--diagnose',action='store_true',help='add counters to an existing output without rerunning timed measurements')
    a=p.parse_args()
    if a.copies<1:p.error('copies must be positive')
    out=a.output.resolve();os.environ['RAYON_NUM_THREADS']='1'
    if a.diagnose:
        diagnose_existing(out);return
    if a.bake_root is None:p.error('--bake-root is required unless --diagnose is used')
    out.mkdir(parents=True,exist_ok=False)
    report=dict(schema='spine-joint-native-ablation/v1',threads=1,platform=platform.platform(),
        commit=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),
        dirty=bool(subprocess.check_output(['git','status','--porcelain'],cwd=ROOT)),runner_sha256=sha(RUNNER),
        harness_sha256=sha(ROOT/'examples/support/phased_bench.rs'),
        scope='Actual current native runtime; no engine edits for this experiment; not pristine',
        reuse_bake=str(a.bake_root.resolve()),external_guard=dict(seconds=30,process_group_rss_bytes=4*1024**3),training={},cases=[])
    def save():(out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    for reference in json.loads(REFERENCE.read_text())['cases']:
        name=reference['fixture'];dest=out/name;tick=time.perf_counter()
        data,variants,metadata=prepare_variants(a.bake_root.resolve()/name,reference,dest)
        metadata['prepare_seconds']=time.perf_counter()-tick;report['training'][name]=metadata;save()
        fresh=(make_input(data,a.copies,10)[0] if name=='eggcc_if' else clone_inputs(data,a.copies))
        cases=[('source',[a['action'] for a in data['actions']]),(f'fresh-{a.copies}',fresh)]
        for label,initial in cases:
            case=run_case(name,label,data,initial,variants,metadata,dest/label)
            report['cases'].append(case);save();render(report,out)
            print(name,label,{k:round(v['result']['run_seconds']*1000,3) for k,v in case['timings'].items()},flush=True)
    print(out/'index.html')


if __name__=='__main__':main()
