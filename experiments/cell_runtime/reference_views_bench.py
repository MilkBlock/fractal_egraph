"""Reuse verified upstream passes; re-emit only the CPU device artifact.

The comparison uses identical marked source, control, keys and merge contracts.
No raw source_core benchmark path; no second Bake or solver invocation.
"""
import argparse,json,shutil,os,subprocess
from pathlib import Path
from eggfuse.engine.cell_runtime import load_pipeline_artifact,binary
from eggfuse.compiler.passes import verified_receipts,PassManager
from eggfuse.compiler.targets.cell_cpu import compile_target
from experiments.cell_runtime.suite import timed


def reemit(source,dest):
    _,_,provenance=load_pipeline_artifact(source)
    original=json.loads((source/'pipeline.json').read_text());names=[s['name'] for s in original['passes'] if s['name'] not in ('device','verify')]
    records=verified_receipts(source,names);pm=PassManager(dest)
    for step in original['passes']:
        if step['name'] not in records:continue
        old=records[step['name']][0];new=dest/'passes'/step['name']/step['key'];new.parent.mkdir(parents=True,exist_ok=True);shutil.copytree(old,new)
        pm.receipts.append({**step,'directory':str(new.resolve())})
    for name in ('source.egg','source_sk.egg','source_sk_mk.egg','marker-program.json','recipe-program.json'):
        pm.publish(source/name,name)
    pm.save()
    core=next(Path(s['directory'])/'core.json' for s in pm.receipts if s['name']=='marker_ir')
    compile_target(pm,core,dest/'recipe-program.json',dest/'marker-program.json',dest/'source_sk_mk.egg','device',False)
    path,program,new=load_pipeline_artifact(dest)
    assert new['source_sk_mk_sha256']==provenance['source_sk_mk_sha256']
    return path,program,new


def main():
    p=argparse.ArgumentParser();source=p.add_mutually_exclusive_group(required=True);source.add_argument('--pipeline',type=Path);source.add_argument('--compiled',type=Path,help='reuse an already verified reference-view pipeline after engine-only changes');p.add_argument('--output',type=Path,required=True);p.add_argument('--engine',type=Path);a=p.parse_args()
    out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
    path,program,provenance=load_pipeline_artifact(a.compiled.resolve()) if a.compiled else reemit(a.pipeline.resolve(),out/'compile')
    if not program.get('reference_views',{}).get('marker_tables'):raise ValueError('compiled input has no reference marker contract; reemit the device pass')
    engine=a.engine or binary()
    audit=subprocess.run([str(engine),'--program',str(path),'--verify-reference-views'],capture_output=True,text=True)
    (out/'audit.log').write_text(audit.stdout+audit.stderr)
    if audit.returncode:raise RuntimeError(audit.stderr)
    verification=json.loads(audit.stdout)
    if not verification['reference_views']['enabled']:raise ValueError('reference views were not enabled in the audit')
    runs=[];before=os.environ.get('CELL_CPU_REFERENCE_VIEWS')
    try:
        for flag in ('0','1'):
            os.environ['CELL_CPU_REFERENCE_VIEWS']=flag;runs.append(timed(path,engine,out/('mode-'+flag)))
    finally:
        if before is None:os.environ.pop('CELL_CPU_REFERENCE_VIEWS',None)
        else:os.environ['CELL_CPU_REFERENCE_VIEWS']=before
    result=dict(provenance=provenance,verification=verification,measurements=[r[0] for r in runs],reports=[{k:v for k,v in r[1].items() if k!='steps'} for r in runs],speedup=runs[0][0]['inference_seconds']/runs[1][0]['inference_seconds'])
    (out/'results.json').write_text(json.dumps(result,indent=2));print(json.dumps({k:v for k,v in result.items() if k not in ('provenance','reports')},indent=2))
if __name__=='__main__':main()
