#!/usr/bin/env python3
"""Fresh analyze -> fork-native/CPU/Metal -> exact full-graph and binding checks.
Single executions only; these tiny controls are not throughput benchmarks.
"""
import argparse, hashlib, json, os, subprocess, sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'tools/bench'))
from math_graph_equal import verify

def run(cmd,**kwargs):
    try:
        return subprocess.run(list(map(str,cmd)),cwd=ROOT,env=dict(os.environ,RAYON_NUM_THREADS='1',EGG_LAYOUT_GRAPHS='0'),check=True,**kwargs)
    except subprocess.CalledProcessError as e:
        if e.stderr:print(e.stderr,file=sys.stderr)
        raise
def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--analyzer',type=Path,default=ROOT/'target/analyzer/debug/egg_layout')
    p.add_argument('--cpu-only',action='store_true');p.add_argument('--skip-build',action='store_true')
    a=p.parse_args();out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
    manifest=ROOT/'cell-assisted-egglog/Cargo.toml'
    binary=ROOT/'target/cell-assisted/release/cell-assisted-egglog'
    if not a.skip_build:run(['cargo','build','--offline','--release','--manifest-path',manifest,'--target-dir',ROOT/'target/cell-assisted'])
    if not a.analyzer.exists():
        run(['cargo','build','--offline','--bin','egg_layout','--target-dir',ROOT/'target/analyzer'])
    worker=out/'metal-residual'
    if not a.cpu_only:run(['swiftc','-O',ROOT/'cell-assisted-egglog/gpu.swift','-o',worker])
    getters=(ROOT/'cell-assisted-egglog/fixtures/eggcc-getters.egg').read_text()
    if_source=(ROOT/'experiments/cell_spine/eggcc_if_tiny/source.egg').read_text()
    for name in ['single-get','if-min','if-max']:
        if_source=if_source.replace(':ruleset if-tiny)',f':ruleset if-tiny :name "{name}")',1)
    ports=(ROOT/'experiments/cell_egraph/port-progression.egg').read_text()
    cases=[dict(name='eggcc-getters',source=getters,ruleset='',consumer='route-get',native='single-get',rounds=3,sorts=['Expr'],
                input='(Single (Leaf 100)) (Single (Leaf 200)) (Add-Gets (Leaf 1000) (Single (Leaf 100)) 5) (Add-Gets (Leaf 2000) (Single (Leaf 200)) 7) (Add-Gets (Leaf 3000) (Leaf 300) 9)',
                parts=[dict(atoms=[0],cell=True),dict(atoms=[1],cell=False)]),
           dict(name='eggcc-if',source=if_source,ruleset='if-tiny',consumer='if-min',native='single-get,if-max',rounds=3,
                sorts=['Expr','Assumption','BaseType','BinaryOp','Type','TypeList'],
                input='\n'.join(l.replace('train-','heldout-') for l in if_source.splitlines() if l.startswith('(If ')),
                parts=[dict(atoms=[i],cell=i in (5,9)) for i in [2,5,9,0,1,3,4,6,7,8,10,11,12]]),
           dict(name='two-cells',source=ports,ruleset='',consumer='overlap',native='left-to-pair,right-to-pair,pair-to-left,pair-to-right',rounds=4,sorts=['E'],
                input='(L (Atom 101) (Atom 102)) (R (Atom 102) (Atom 103)) (L (Atom 103) (Atom 104))',
                parts=[dict(atoms=[0],cell=True),dict(atoms=[1],cell=True)])]
    report=dict(scope='actual forked egglog RHS/union/rebuild; positive constructor/relation Cell fragments; fresh held-out inputs; fixed diagnostic macro rounds, not arbitrary schedule equivalence',
        fork_base='9ea85e47824fcef4e4336fe2f2e3160d37169f73',pristine_measured=False,
        binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),cases=[])
    for c in cases:
        folder=out/c['name'];folder.mkdir()
        source=folder/'training.egg';source.write_text(c['source']);inputs=folder/'input.egg';inputs.write_text(c['input'])
        analysis=folder/'analyze'
        with (folder/'analyze.log').open('w') as log:
            run([a.analyzer,'analyze','--source',source,'--output',analysis,'--save-history'],stdout=log,stderr=log)
        base=[binary,'--analysis',analysis,'--ruleset',c['ruleset'],'--consumer',c['consumer'],'--native',c['native'],
              '--parts',json.dumps(c['parts']),'--input',inputs,'--rounds',c['rounds']]
        result=dict(name=c['name'],source_sha256=hashlib.sha256(c['source'].encode()).hexdigest(),controls={})
        modes=['off','cpu']+([] if a.cpu_only else ['gpu'])
        for mode in modes:
            cmd=base+['--mode',mode,'--dump',folder/f'{mode}-graph.json']
            if mode=='gpu':cmd+=['--gpu-worker',worker]
            data=json.loads(run(cmd,capture_output=True,text=True).stdout)
            (folder/f'{mode}.json').write_text(json.dumps(data,indent=2)+'\n');result['controls'][mode]=data
            if mode!='off':
                result.setdefault('graphs',{})[mode]=verify(folder/'off-graph.json',folder/f'{mode}-graph.json',c['sorts'])
                assert all(not r['subsumed'] for r in json.loads((folder/f'{mode}-graph.json').read_text()))
                audit=json.loads(run(base+['--mode',mode,'--audit','true']+(['--gpu-worker',worker] if mode=='gpu' else []),capture_output=True,text=True).stdout)
                assert audit['stats']['audit_uncovered_per_round'][-1]==0,audit['stats']
                if c['name']!='two-cells':assert audit['stats']['audit_uncovered_bindings']==0,audit['stats']
                assert audit['stats']['audit_queries']==c['rounds']
                result.setdefault('binding_audits',{})[mode]=audit['stats']
                assert data['stats']['assisted_full_lhs_queries']==data['stats']['audit_queries']==0
        if not a.cpu_only:
            assert result['controls']['cpu']['stats']['candidate_pairs']==result['controls']['gpu']['stats']['candidate_pairs']
            assert result['controls']['cpu']['stats']['accepted_bindings']==result['controls']['gpu']['stats']['accepted_bindings']
        report['cases'].append(result)
        (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
        print(c['name'],result['graphs'],flush=True)
    print(out/'report.json')
if __name__=='__main__':main()
