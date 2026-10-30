"""Compiler contract checks; tiny end-to-end runs, not a speed benchmark."""
import argparse,copy,json,subprocess,sys
from pathlib import Path
from unittest.mock import patch
ROOT=Path(__file__).resolve().parents[2];sys.path.insert(0,str(ROOT))
from eggfuse.compiler.pipeline import compile_pipeline
from eggfuse.compiler.marker_ir import validate,digest

def main():
    p=argparse.ArgumentParser();p.add_argument('--build',type=Path,required=True);p.add_argument('--solver-python',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    a.output.mkdir(parents=True,exist_ok=False);tools=ROOT/'target/debug/examples';summary={}
    def compile(name,backend='gpu',profile=None,output=None):
        return compile_pipeline(ROOT/f'experiments/unified_compiler/{name}.egg',output or a.output/name,a.build,tools,a.solver_python.absolute(),profile,capacity=512,match_capacity=2048,backend=backend)
    first=compile('spine');folder=first.output
    report=json.loads((folder/'verification.json').read_text());ir=json.loads((folder/'marker-program.json').read_text());core=json.loads((folder/'eggfuse_related_files/core.json').read_text())
    assert len(ir['markers'])==2 and len(ir['markers'][0]['guarantee'])==3
    assert report['device_marker_contract']['publication_sites']>=2 and report['device_marker_contract']['consumer_reads']>=2
    assert len(ir['continuations'])==2
    summary['spine']=report
    verified_dir=Path(next(r for r in first.receipts if r['name']=='verify')['directory'])
    gpu_report=json.loads((verified_dir/'gpu/report.json').read_text())
    assert report['device_marker_contract']['receipt_shortcuts']>=2
    assert all(s['matched']==0 for s in gpu_report['steps'] if s['set']=='cell-spine-complete')
    summary['published_marker_avoids_recompletion']=True
    again=compile('spine');assert all(r['cached'] for r in again.receipts);summary['unchanged_passes_reused']=len(again.receipts)
    import run
    runtime=Path(next(r for r in again.receipts if r['name']=='verify')['directory'])/'runtime'
    # Execute the emitted binary-layout/shader document. Recompilation here is
    # a bug, even if it happens to regenerate an equivalent program.
    with patch.object(run,'compile_program',side_effect=AssertionError('runtime recompiled')):
        prefix=run.run(folder/'source_sk_mk.egg',runtime,a.output/'first-commit',check_at_end=True,max_runs=1,marker_program=folder/'marker-program.json',compiled_program=folder/'eggfuse_related_files/program.json')
    state=json.loads((a.output/'first-commit/gpu-state.json').read_text());program=json.loads((a.output/'first-commit/program.json').read_text())
    assert prefix['steps'][0]['set']=='build' and len(prefix['steps'])==1
    assert sum(program['tables'][r[0]]['name']==ir['markers'][0]['name'] for r in state['rows'])==1
    summary['published_before_completion_run']=True;summary['runtime_did_not_recompile']=True
    from eggfuse.engine.metal import run_artifact
    import tempfile,shutil
    with tempfile.TemporaryDirectory(prefix='eggfuse-artifact-') as tmp:
        artifact=Path(tmp)/'artifact';shutil.copytree(folder/'eggfuse_related_files',artifact)
        with patch.object(run,'compile_program',side_effect=AssertionError('artifact runner recompiled')):
            loaded=run_artifact(artifact,runtime,a.output/'portable-artifact',check=True)
        assert loaded['checks'][-1]['exact_rows_values_equivalence_visibility']
    capability=json.loads((folder/'eggfuse_related_files/capabilities.json').read_text())
    assert all(r['expressed'] and r['verified'] and r['generic_lowered'] and r['executed'] is None for r in capability['rules'])
    summary['self_contained_artifact_without_relowering']=True

    negatives=[]
    for kind in ['guarantee','field','schedule','guard']:
        bad=copy.deepcopy(ir);other=copy.deepcopy(core)
        if kind=='guarantee':bad['markers'][0]['guarantee'].pop()
        elif kind=='field':bad['markers'][0]['fields'][0]['sort']='i64'
        elif kind=='guard':other['rulesets'][0]['rules'][0]['seminaive']=not other['rulesets'][0]['rules'][0]['seminaive']
        else:
            def remove(s):
                if 'sequence' in s:s['sequence']=[remove(x) for x in s['sequence'] if x.get('run')!='cell-spine-complete' and x.get('saturate',{}).get('run')!='cell-spine-complete']
                elif 'body' in s:s['body']=remove(s['body'])
                elif 'saturate' in s:s['saturate']=remove(s['saturate'])
                return s
            other['schedule']=[remove(s) for s in other['schedule']];bad['typed_program']['schedule']=copy.deepcopy(other['schedule'])
        bad.pop('sha256');bad['sha256']=digest(bad)
        try:validate(bad,other)
        except (ValueError,KeyError):negatives.append(kind)
        else:raise AssertionError('accepted stale/invalid '+kind)
    summary['rejected_contract_changes']=negatives
    # Explicit synthetic prices exercise the cost/invalidation path. They are
    # not runtime measurements and do not justify a performance claim.
    marker_dir=Path(next(r for r in again.receipts if r['name']=='markers')['directory'])/'result'
    offered=json.loads((marker_dir/'markers.json').read_text())['offered_fragments']
    prices=dict(schema='marker-cost-profile/v1',source_sha256=ir.get('unused',''))
    import hashlib
    prices.update(source_sha256=hashlib.sha256((folder/'source_sk.egg').read_bytes()).hexdigest(),provenance='synthetic unit-test prices; not measured',
        markers={g['key']:dict(maintenance_ns=1e9,saved_search_ns_by_rule={n:1 for n in g['consumers']}) for g in offered})
    profile=a.output/'prices.json';profile.write_text(json.dumps(prices));priced=compile('spine',profile=profile)
    assert all(r['cached'] for r in priced.receipts if r['name'] in ('structural_keys','analyze','sk_native'))
    assert not json.loads((folder/'marker-program.json').read_text())['markers']
    summary['cost_change_reused_sk_and_bake']=True
    for name in ['late-union','one-run','partial-publisher','alias-fiber']:
        pm=compile(name);summary[name]=json.loads((pm.output/'verification.json').read_text())
        if name in ('late-union','partial-publisher'):
            path=Path(next(r for r in pm.receipts if r['name']=='verify')['directory'])/'gpu/report.json'
            steps=json.loads(path.read_text())['steps']
            assert any(s['set']=='cell-spine-complete' and s['matched']>0 for s in steps)
    local=compile('spine',backend='local',output=a.output/'local');summary['local']=json.loads((local.output/'verification.json').read_text())
    key=compile_pipeline(ROOT/'experiments/unified_compiler/choice.egg',a.output/'choice',a.build,tools,a.solver_python.absolute(),stop_after='sk')
    result=subprocess.run([str(tools/'eggfuse_frontend'),str(key.output/'source_sk.egg'),'--native-check'],capture_output=True,text=True,check=True)
    summary['source_declared_key_policy']=json.loads(result.stdout)
    from eggfuse.compiler.pipeline import native_core
    opaque_dir=a.output/'opaque-key-ir';opaque_dir.mkdir()
    opaque=native_core(tools/'eggfuse_frontend',key.output/'source_sk.egg',opaque_dir)
    from eggfuse.compiler.targets.metal.compiler import compile_program
    try:compile_program(opaque)
    except ValueError as e:
        assert 'physical lowering' in str(e);summary['unsupported_gpu_key_type_rejected']=str(e)
    else:raise AssertionError('opaque structural keys were silently lowered')
    (a.output/'checks.json').write_text(json.dumps(summary,indent=2)+'\n');print(json.dumps({k:v for k,v in summary.items() if not isinstance(v,dict)},indent=2))

if __name__=='__main__':main()
