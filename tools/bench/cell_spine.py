#!/usr/bin/env python3
"""Bake once, build new Math graphs with fresh bindings; independent native processes."""
import argparse
import copy
import hashlib
import html
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import time

from math_virtual import ROOT, measure
from math_graph_equal import verify
sys.path.insert(0,str(ROOT))
from tools.cell_spine.compiler import commands, cellify
from tools.cell_spine.frontier import RUNTIME_SCHEDULE
from tools.cell_spine.terms import actions, walk

RUNNER = ROOT/'target/release/examples/native_math_baseline'
PRISTINE = ROOT/'out/native-compare/bin/pristine'
SOURCES = ROOT/'experiments/cell_spine/math_tiny'
sha = lambda p: hashlib.sha256(Path(p).read_bytes()).hexdigest()


def prepare(data, library, copies, folder):
    """Fresh literal bindings, no new Bake and no replay of the training graph."""
    seed = data['actions'][0]['action']['expr']
    initial = []
    for i in range(copies):
        expr = copy.deepcopy(seed)
        for e in walk(expr):
            if e['kind']=='call' and e['head']=='Var':
                e['args'][0]['text'] = json.dumps(f'heldout-{i}/'+json.loads(e['args'][0]['text']))
        initial.append(dict(kind='expr',expr=expr))
    environment = '\n'.join(commands(data,{'declaration','rule'}))+'\n'
    input_text = '\n'.join(actions(initial))+'\n'
    schedule = '(run-schedule (saturate (run)))\n'
    (folder/'native-env.egg').write_text(environment)
    (folder/'native-input.egg').write_text(input_text)
    source = folder/'source.egg';source.write_text(environment+input_text+schedule)
    # Charge AST-to-forked-program lowering separately. Common workload generation
    # above is excluded from every runtime; the existing typed AST is the input.
    tick = time.perf_counter()
    compact, export = cellify(dict(signatures=data['signatures'],actions=[{'action':a} for a in initial],checks=[]),library)
    run = RUNTIME_SCHEDULE+'\n'
    body = compact.removeprefix(library)
    assert body.endswith(run)
    (folder/'spine-input.egg').write_text(body.removesuffix(run))
    (folder/'library.egg').write_text(library)
    lowering = time.perf_counter()-tick
    common = dict(constructors=sorted(data['signatures']))
    native = dict(common,environment=str(folder/'native-env.egg'),input=str(folder/'native-input.egg'),run=schedule)
    spine = dict(common,environment=str(folder/'library.egg'),input=str(folder/'spine-input.egg'),run=run)
    manifests = dict(native=native,spine=spine,spine_export=dict(spine,export=export))
    for mode, manifest in manifests.items():
        (folder/f'{mode}.json').write_text(json.dumps(manifest,indent=2)+'\n')
    return source, lowering


def render(report, out):
    lines=['# Cell spine 建立新图：单次实测','',
           '训练：每种形式只 Bake 一个样例。测试：全新的变量名字，多个互不相交的实例；没有重新 Bake 测试输入。',
           '主对照使用同一当前内核；pristine 是已校验哈希的历史基线二进制。',
           '计时进程不导出诊断图、不采集 trace。完整图判等在另一进程执行。','',
           '| 输入/份数 | 变体 | 安装 ms | 输入 ms | 推理 ms | 导出 ms | 总执行 ms | 峰值 RSS MiB |',
           '|---|---|---:|---:|---:|---:|---:|---:|']
    for case in report['cases']:
        for mode,m in case['timings'].items():
            r=m['result']
            phases=[f'{r[k]*1000:.3f}' if k in r else '—' for k in ('setup_seconds','input_seconds','run_seconds','export_seconds')]
            lines.append('| '+ ' | '.join([f'{case["workload"]}/{case["copies"]}',mode,*phases,f'{r["seconds"]*1000:.3f}',f'{m["peak_rss_bytes"]/2**20:.2f}'])+' |')
    lines += ['', '## 图规模与前端转换', '',
              '| 输入/份数 | 等价节点 / 类 | spine 节点 | Summary 活跃 / 总行 | Cell 前端 ms |',
              '|---|---:|---:|---:|---:|']
    for case in report['cases']:
        v=case['verification']; shape=case['shapes']
        lines.append(f'| {case["workload"]}/{case["copies"]} | {v["enodes"]:,} / {v["eclasses"]:,} | {shape["spine"]["enodes"]:,} | {shape["active_summaries"]:,} / {shape["active_summaries"]+shape["subsumed_summaries"]:,} | {case["cellify_seconds"]*1000:.3f} |')
    lines.append('')
    lines += ['## 口径','',
              '- `spine` 停在紧凑 Cell 图，语义由独立的完整导出判等确认。`spine_export` 实际生成完整 Math 图。',
              '- 总执行包含 EGraph 新建、规则/库解析安装、输入插入、运行及指定的导出；不含磁盘读取、引擎析构。各项另有记录。',
              '- setup 是命令边界，不保证覆盖全部编译成本：egglog 的延迟准备可能进入 run。',
              '- Cell 前端转换从已有 typed AST 开始；没有将源程序的额外解析成本算作免费加速。',
              '- Bake/Gurobi/编译器验证单独记录，不算运行期加速。RSS 包含已加载库，库文件字节也单独报告。',
              '- 每种情况只跑一次，未取平均；规模较小时安装和进程噪声可能占主导。',
              '- 这是四条算术规则上的独立新绑定，不是完整 math microbench、跨 Cell 稠密交互或任意输入泛化证明。','',
              '## 训练开销','']
    for name, training in report['training'].items():
        lines.append(f'- {name}: 离线采集/Bake/Gurobi/验证合计 {training["measurement"]["observed_wall_seconds"]:.3f} s；库 {training["library_bytes"]:,} B。')
    (out/'README.md').write_text('\n'.join(lines)+'\n')
    # Small standalone table; no server or published-site deployment.
    rows=[]
    for c in report['cases']:
        for mode,m in c['timings'].items():
            r=m['result'];cols=[f'{c["workload"]} / {c["copies"]}',mode]+[f'{r[k]*1000:.3f}' if k in r else '—' for k in ('setup_seconds','input_seconds','run_seconds','export_seconds','seconds')]+[f'{m["peak_rss_bytes"]/2**20:.2f}']
            rows.append('<tr>'+''.join('<td>'+html.escape(x)+'</td>' for x in cols)+'</tr>')
    (out/'index.html').write_text('<!doctype html><meta charset="utf-8"><title>Cell 新图 bench</title><style>body{font:16px system-ui;margin:30px auto;max-width:1200px;color:#173047}table{border-collapse:collapse;width:100%}td,th{padding:10px;border-bottom:1px solid #dce3eb;text-align:right}pre{white-space:pre-wrap;background:#f5f7fa;padding:20px}</style><h1>Bake 一次，用 Cell 建立新图</h1><p>新绑定；独立进程；单次测量；完整状态另行判等。单位 ms，内存 MiB。</p><table><tr>'+''.join('<th>'+x+'</th>' for x in ['输入/份数','变体','安装','输入','推理','导出','总执行','峰值 RSS'])+'</tr>'+''.join(rows)+'</table><pre>'+html.escape('\n'.join(lines[lines.index('## 口径'):]))+'</pre><a href="report.json">完整数据与命令</a>')


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--copies',type=int,nargs='+',default=[1,1000])
    p.add_argument('--skip-build',action='store_true')
    p.add_argument('--no-pristine',action='store_true')
    a=p.parse_args()
    if any(n<1 for n in a.copies):p.error('copies must be positive')
    out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
    if not a.skip_build:
        subprocess.run(['cargo','build','--offline','--release','--bin','egg_layout','--example','native_math_baseline',
                        '--example','lhs_ast','--example','spine_validate','--example','spine_stages'],cwd=ROOT,check=True)
    os.environ['RAYON_NUM_THREADS']='1'
    pristine=None
    if not a.no_pristine:
        evidence=json.loads((ROOT/'experiments/native_compare/results/final.json').read_text())
        tag=subprocess.check_output(['git','rev-parse','egglog-baseline'],cwd=ROOT,text=True).strip()
        assert evidence['baseline_tag']==tag and sha(PRISTINE)==evidence['binary_sha256']['pristine'],'pristine provenance mismatch'
        pristine=dict(binary=str(PRISTINE),sha256=sha(PRISTINE),tag=tag,evidence='experiments/native_compare/results/final.json')
    report=dict(schema='cell-spine-new-graphs-bench/v1',platform=platform.platform(),threads=1,
        commit=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),
        dirty=bool(subprocess.check_output(['git','status','--porcelain'],cwd=ROOT)),binary_sha256=sha(RUNNER),
        environment={k:v for k,v in os.environ.items() if k.startswith(('EGG_LAYOUT_','RAYON_'))},
        pristine=pristine,external_process_guard=dict(seconds=30,rss_bytes=4*1024**3),training={},cases=[])
    def save(): (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    for name,file in [('factored','source.egg'),('expanded','expanded.egg')]:
        source=SOURCES/file;train=out/f'bake-{name}'
        measurement=measure([sys.executable,'-m','tools.cell_spine.compiler','--source',source,'--mode','symbolic',
                             '--output',train,'--skip-build'],out/f'bake-{name}')
        compiled=json.loads((train/'report.json').read_text());assert compiled['status']=='compiled'
        library=(train/'library.egg').read_text()
        report['training'][name]=dict(measurement=measurement,source_sha256=sha(source),library_sha256=sha(train/'library.egg'),library_bytes=len(library.encode()),
                                     families=len(compiled['plan']['selected']),consumer_rules=len(compiled['transitions']))
        data=json.loads((train/'work/input.json').read_text())
        save()
        for copies in a.copies:
            folder=out/f'{name}-{copies}';folder.mkdir()
            raw,lowering=prepare(data,library,copies,folder)
            # Verification has its own processes; no graph scan inflates timed RSS.
            reference=measure([RUNNER,'bench',folder/'native.json',folder/'native-graph.json'],folder/'verify-native')
            encoded=measure([RUNNER,'bench',folder/'spine_export.json',folder/'spine-graph.json'],folder/'verify-spine')
            equality=verify(folder/'native-graph.json',folder/'spine-graph.json')
            assert equality['enodes']==13*copies and equality['eclasses']==7*copies
            if pristine:
                measure([PRISTINE,raw,folder/'pristine-graph.json'],folder/'verify-pristine')
                verify(folder/'pristine-graph.json',folder/'native-graph.json')
            case=dict(workload=name,copies=copies,source_sha256=sha(raw),cellify_seconds=lowering,verification=equality,
                      shapes=encoded['result']['verification'],timings={})
            for mode in ('native','spine','spine_export'):
                m=measure([RUNNER,'bench',folder/f'{mode}.json'],folder/f'time-{mode}')
                expected=reference if mode=='native' else encoded
                assert m['result']['matches']==expected['result']['matches'],'timed/verified run drift'
                case['timings'][mode]=m
            if pristine:case['timings']['pristine']=measure([PRISTINE,raw],folder/'time-pristine')
            assert sha(train/'library.egg')==report['training'][name]['library_sha256'],'library changed during new-graph runs'
            report['cases'].append(case);save()
            print(name,copies,{k:round(v['result']['seconds']*1000,3) for k,v in case['timings'].items()},flush=True)
    report['cross_input_equalities']={str(n):verify(out/f'factored-{n}/native-graph.json',out/f'expanded-{n}/native-graph.json') for n in a.copies}
    save()
    render(report,out)
    print(out/'index.html')


if __name__=='__main__':main()
