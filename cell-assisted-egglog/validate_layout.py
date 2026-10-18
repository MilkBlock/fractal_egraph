#!/usr/bin/env python3
"""One correctness/work ablation per variant, not a throughput benchmark.
Uses actual analyzed eggcc Single/Get Cells, GPU numeric consumer and native checks.
"""
import argparse
import json
from pathlib import Path
import subprocess
import sys

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT))
from tools.bench.math_graph_equal import verify
from validate_numeric import primitive_rows

def main():
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--output',type=Path,required=True)
    ap.add_argument('--layout-python',type=Path,required=True)
    ap.add_argument('--solvers',default='scip,gurobi')
    ap.add_argument('--skip-build',action='store_true')
    a=ap.parse_args();out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
    if not a.skip_build:subprocess.run(['cargo','build','--offline','--manifest-path','cell-assisted-egglog/Cargo.toml','--target-dir','target/cell-assisted'],cwd=ROOT,check=True)
    worker=out/'matrix-metal';subprocess.run(['swiftc','-O',ROOT/'cell-assisted-egglog/matrix.swift','-o',worker],check=True)
    binary=ROOT/'target/cell-assisted/debug/cell-assisted-egglog'
    source=ROOT/'cell-assisted-egglog/matrix-numeric/consumer.egg'
    lines=[]
    for i in range(12):
        lines += [f'(Single (Leaf {i}))', f'(set (Best (Leaf {100+i})) 100)']
        lines += [f'(Offer (Leaf {100+i}) (Single (Leaf {i})) {v})' for v in (3,5,200)]
    data=out/'input.egg';data.write_text('\n'.join(lines)+'\n')
    base=[str(binary),'--analysis',str(ROOT/'cell-assisted-egglog/fixtures/analyze'),'--consumer-source',str(source),'--ruleset','','--consumer','cost-improve','--native','single-get','--parts','[{"atoms":[0],"cell":true},{"atoms":[1],"cell":false},{"atoms":[2],"cell":false}]','--input',str(data),'--rounds','4','--gpu-worker',str(worker),'--audit','false']
    configs=[('native','off','cartesian','legacy','off','scip'),
        ('cartesian-gram','matrix-numeric','cartesian','legacy','on','scip'),
        ('marker-gram','matrix-numeric','indexed','legacy','on','scip'),
        ('marker-paired','matrix-numeric','indexed','paired','on','scip')]
    configs += [(f'marker-{s}','matrix-numeric','indexed','solver','on',s) for s in a.solvers.split(',')]
    report=dict(scope='real native Cell/event submission, strict per-batch checks; work counts, not end-to-end speedup',cases=[])
    expected=None;eq=None
    for name,mode,marker,layout,checked,solver in configs:
        graph=out/f'{name}-graph.json'
        cmd=base+['--mode',mode,'--marker-layout',marker,'--matrix-layout',layout,'--gpu-check',checked,'--layout-python',str(a.layout_python),'--layout-solver',solver,'--dump',str(graph)]
        with (out/f'{name}.json').open('w') as f:subprocess.run(cmd,cwd=ROOT,check=True,stdout=f)
        rows=json.loads(graph.read_text())
        if expected is None:
            expected=rows;eq={r['sort'] for r in rows if r['sort'] not in {'i64','bool','String','Unit'}}
        assert primitive_rows(rows,eq)==primitive_rows(expected,eq),name
        filtered=out/f'{name}-eq.json';filtered.write_text(json.dumps([r for r in rows if r['sort'] in eq]))
        same=verify(out/'native-eq.json',filtered,eq_sorts=eq)
        if mode=='off':report['native_graph']=same;continue
        stats=json.loads((out/f'{name}.json').read_text())['stats']
        assert stats['accepted_bindings']==24 and stats['native_sets']==12,name
        assert stats['verification']['failures']==0 and stats['verification']['batches']>0,name
        assert all(c[1]<=1 for c in stats['device']['execution_counts']),name
        rounds=stats['round_work']
        work=[l['work'] for r in rounds for s in r['levels'] for l in s]
        def total(k):return sum(w[k] for w in work)
        plan=stats['physical_layout']['plan']
        report['cases'].append(dict(name=name,exact_graph=same['exact_equal'],primitive_values_exact=True,
            channels=stats['channels'],accepted_bindings=stats['accepted_bindings'],native_sets=stats['native_sets'],
            cartesian_combinations_bound=stats['cartesian_combinations_bound'],markers=stats['marker_work'],
            matrix_calls=total('matrix_calls'),matrix_positions=total('matrix_positions'),matrix_macs=total('matrix_macs'),
            paired_comparisons=sum(r['layout_work']['paired_comparisons'] for r in rounds),
            paired_components=sum(r['layout_work']['paired_components'] for r in rounds),
            packed_components=sum(r['layout_work']['packed_components'] for r in rounds),
            host_count_reads=sum(r['host_count_reads'] for r in rounds),
            final_gpu_buffer_bytes=stats['device']['matrix_bytes'],verification=stats['verification'],
            selected_methods=[b['method'] for b in plan['blocks']],solver_calls=stats['physical_layout']['solver_calls'],
            solver_report=plan['report'].get('solver'),reference_objective=plan['report'].get('objective')))
        print(name,'verified',flush=True)
    (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(out/'report.json')

if __name__=='__main__':main()
