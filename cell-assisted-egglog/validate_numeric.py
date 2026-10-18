#!/usr/bin/env python3
"""Actual native function updates through the shared matrix plan; one run each.
Checks constructor/equality structure and primitive function values separately.
No timing or neural-network-throughput claim.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT))
from tools.bench.math_graph_equal import verify


def primitive_rows(rows, eq):
    names={}
    while True:
        changed=False
        for r in rows:
            if r['sort'] not in eq or any(t in eq and (t,v) not in names for t,v in r['args']):continue
            term=json.dumps([r['op'],[names[t,v] if t in eq else [t,v] for t,v in r['args']]],separators=(',',':'))
            k=(r['sort'],r['result'])
            if k not in names or (len(term),term)<(len(names[k]),names[k]):names[k]=term;changed=True
        if not changed:break
    return sorted((r['op'],json.dumps([names[t,v] if t in eq else [t,v] for t,v in r['args']]),r['sort'],r['result'],r['subsumed']) for r in rows if r['sort'] not in eq)


def main():
    ap=argparse.ArgumentParser(description=__doc__);ap.add_argument('--output',type=Path,required=True);ap.add_argument('--skip-build',action='store_true');args=ap.parse_args();out=args.output.resolve();out.mkdir(parents=True,exist_ok=False)
    if not args.skip_build:subprocess.run(['cargo','build','--offline','--manifest-path','cell-assisted-egglog/Cargo.toml','--target-dir','target/cell-assisted'],cwd=ROOT,check=True)
    worker=out/'matrix-metal';subprocess.run(['swiftc','-O',ROOT/'cell-assisted-egglog/matrix.swift','-o',worker],check=True)
    binary=ROOT/'target/cell-assisted/debug/cell-assisted-egglog';source=ROOT/'cell-assisted-egglog/matrix-numeric/consumer.egg';data=ROOT/'cell-assisted-egglog/matrix-numeric/input.egg';analysis=ROOT/'cell-assisted-egglog/fixtures/analyze'
    base=[binary,'--analysis',analysis,'--consumer-source',source,'--ruleset','','--consumer','cost-improve','--native','single-get','--parts','[{"atoms":[0],"cell":true},{"atoms":[1],"cell":false},{"atoms":[2],"cell":false}]','--input',data,'--rounds','4']
    runs=[('off','off','off'),('matrix-numeric','matrix-numeric','on'),('matrix-numeric-unchecked','matrix-numeric','off')]
    for name,mode,checked in runs:
        with (out/f'{name}.json').open('w') as f:subprocess.run(base+['--mode',mode,'--gpu-worker',worker,'--audit','false','--gpu-check',checked,'--dump',out/f'{name}-graph.json'],cwd=ROOT,check=True,stdout=f)
    native=json.loads((out/'off-graph.json').read_text());actual=json.loads((out/'matrix-numeric-graph.json').read_text())
    primitive={'i64','bool','String','Unit'};eq={r['sort'] for r in native if r['sort'] not in primitive}
    assert primitive_rows(native,eq)==primitive_rows(actual,eq),'primitive result payload mismatch'
    for name,rows in [('native',native),('matrix',actual)]:
        (out/f'{name}-equivalence-rows.json').write_text(json.dumps([r for r in rows if r['sort'] in eq]))
    graph=verify(out/'native-equivalence-rows.json',out/'matrix-equivalence-rows.json',eq_sorts=eq)
    unchecked=json.loads((out/'matrix-numeric-unchecked-graph.json').read_text())
    assert primitive_rows(actual,eq)==primitive_rows(unchecked,eq),'checking changed function values'
    (out/'unchecked-equivalence-rows.json').write_text(json.dumps([r for r in unchecked if r['sort'] in eq]))
    check_modes_graph=verify(out/'matrix-equivalence-rows.json',out/'unchecked-equivalence-rows.json',eq_sorts=eq)
    r=json.loads((out/'matrix-numeric.json').read_text());stats=r['stats'];device=stats.pop('device');assert stats['accepted_bindings']==3 and stats['native_sets']==2;assert not device['numeric']['arithmetic_error']
    assert stats['verification']['enabled'] and stats['verification']['batches']>0 and stats['verification']['proposals']==3 and stats['verification']['failures']==0
    off=json.loads((out/'matrix-numeric-unchecked.json').read_text())['stats']
    assert not off['verification']['enabled'] and off['verification']['batches']==0 and not off['audit_uncovered']
    assert off['accepted_bindings']==3 and off['native_sets']==2
    assert all(c[1]<=1 for c in device['execution_counts'])
    report=dict(scope='real analyzed Cell fragments + native function rows; GPU guards, checked i64 RHS and min/max reductions; native final set/canonicalization; correctness, not speedup',
        equivalence_graph=graph,checked_vs_unchecked=check_modes_graph,unchecked_verification=off['verification'],native_total_rows=len(native),primitive_function_rows=sum(r['sort'] not in eq for r in native),primitive_functions_exact=True,final_function_rows=primitive_rows(actual,eq),stats=stats,
        gpu=dict(device=device['device'],matrix_bytes=device['matrix_bytes'],numeric_state=device['numeric']['values'],max_completions_per_channel=max(c[1] for c in device['execution_counts'])),
        source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),worker_sha256=hashlib.sha256(worker.read_bytes()).hexdigest())
    (out/'report.json').write_text(json.dumps(report,indent=2)+'\n');print(out/'report.json')
if __name__=='__main__':main()
