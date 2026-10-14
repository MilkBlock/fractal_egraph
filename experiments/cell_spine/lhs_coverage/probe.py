"""Bake once per fixture; reuse fixed libraries on genuinely unseen fact programs.
No held-out input is analyzed, ripened, added to the catalog, or used by the solver.
"""
import argparse
import json
import subprocess
import sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[3]
sys.path.insert(0,str(ROOT))
from tools.cell_spine import compiler as c, markers


def run(args,log):
    r=subprocess.run(list(map(str,args)),cwd=ROOT,capture_output=True,text=True)
    log.write_text(r.stdout+r.stderr)
    if r.returncode:raise RuntimeError(log.read_text()[-4000:])


def verify(bake,folder,label,facts):
    manifest=json.loads((bake/'work/validation.json').read_text())
    data=json.loads((bake/'work/input.json').read_text())
    environment='\n'.join(c.commands(data,{'declaration','rule'}))
    # Test facts are constructor-only and parsed by egglog, never a regex fork.
    p=folder/(label+'.egg');p.write_text(environment+'\n'+facts)
    parsed=c.ast(p,folder/(label+'-ast.json'))
    raw=[a['action'] for a in parsed['actions']]
    forked='\n'.join(c.actions(raw,True,data['signatures']))+'\n'+markers.RUNTIME_SCHEDULE
    ruleset=data['rule_rulesets'][0] if data['rule_rulesets'] else ''
    manifest.update(dump_states=True,cases=[dict(name=label,reference=environment+'\n'+facts+f'\n(run-schedule (saturate (run {ruleset})))',compressed=forked,restore='(run-schedule (saturate Export!))')])
    path=folder/(label+'-verify.json');path.write_text(json.dumps(manifest));result=folder/(label+'-result.json')
    r=subprocess.run([ROOT/'target/release/examples/spine_validate',path,result],cwd=ROOT,capture_output=True,text=True)
    if not result.exists():raise RuntimeError(r.stdout+r.stderr)
    result=json.loads(result.read_text());record=result['cases'][0] if r.returncode==0 else result
    if r.returncode and 'full closure mismatch' not in r.stderr:raise RuntimeError(r.stdout+r.stderr)
    expected=record.get('original_constructor_rows',len(record.get('expected',{}).get('rows',[])))
    actual=record.get('restored_constructor_rows',len(record.get('actual',{}).get('rows',[])))
    return dict(case=label,complete_state_equal=r.returncode==0,original_rows=expected,restored_rows=actual,
                active_summaries=record['active_summary_tables'],comparison=record['comparison'],
                runtime_matches=record.get('compressed_logical_matches_before_export'),original_rules_executed=record.get('original_rules_executed'))


def main():
    p=argparse.ArgumentParser();p.add_argument('--output',required=True,type=Path);a=p.parse_args();out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
    fixtures=Path(__file__).parent
    tests={
        'train': [('unseen-q','(Q (V "new"))'),('two-q','(Q (V "a")) (Q (V "b"))'),('old-p','(P (V "other"))'),('no-entry','(V "alone")')],
        'permuted':[('unseen-reversed-q','(Q (V "second") (V "first"))'),('repeated-input','(Q (V "same") (V "same"))'),('old-p','(P (V "a") (V "b"))')],
        'correlated':[('same-slot','(Q (V "a")) (R (V "a"))'),('different-slots','(Q (V "a")) (R (V "b"))'),('union-enabled','(Q (V "a")) (R (V "b")) (union (V "a") (V "b"))')],
        'math':[('expanded','(Add (Mul (Var "a") (Var "b")) (Mul (Var "a") (Var "c")))'),('commuted','(Mul (Add (Var "b") (Var "c")) (Var "a"))'),('factored','(Mul (Var "a") (Add (Var "b") (Var "c")))'),('different-coefficients','(Add (Mul (Var "a") (Var "b")) (Mul (Var "d") (Var "c")))')],
        'multi':[('unseen-q-r','(Q (V "a")) (R (V "b"))'),('two-joins','(Q (V "a")) (R (V "b")) (R (V "c"))'),('missing-r','(Q (V "alone"))'),('old-p','(P (V "a") (V "b"))')],
    }
    result={}
    for name,cases in tests.items():
        source=fixtures.parent/'math_tiny/source.egg' if name=='math' else fixtures/(name+'.egg');folder=out/name;folder.mkdir()
        for mode in ('off','entries'):
            bake=folder/mode;args=[sys.executable,'-m','tools.cell_spine.compiler','--source',source,'--output',bake,'--skip-build','--lhs-coverage',mode]
            if mode=='entries':args+=['--analysis-dir',folder/'off/analysis']
            run(args,folder/(mode+'.log'))
            r=json.loads((bake/'report.json').read_text())
            checked=[verify(bake,folder,mode+'-'+label,facts) for label,facts in cases]
            if mode=='entries' and not all(x['complete_state_equal'] for x in checked):raise AssertionError(checked)
            result.setdefault(name,{})[mode]=dict(cells=len({x['family'] for x in r['paths']}),historical_paths=len(r['paths']),library_bytes=r['library_bytes'],coverage=r['lhs_coverage'],checks=checked)
        if name!='math' and all(x['complete_state_equal'] for x in result[name]['off']['checks']):raise AssertionError('fixture did not expose a Bake entry gap')
    summary=dict(scope='actual production Bake and generated .egg runtime; same training catalog per ablation, no held-out rebake, complete constructor/equality comparisons; no performance claim',fixtures=result)
    (out/'results.json').write_text(json.dumps(summary,indent=2)+'\n')
    print(json.dumps({k:{m:sum(x['complete_state_equal'] for x in v['checks']) for m,v in modes.items()} for k,modes in result.items()}))


if __name__=='__main__':main()
