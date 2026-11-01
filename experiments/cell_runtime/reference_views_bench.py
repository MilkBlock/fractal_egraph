"""Reuse verified upstream passes; re-emit only the CPU device artifact.

The comparison uses identical marked source, control, keys and merge contracts.
No raw source_core benchmark path; no second Bake or solver invocation.
"""
import argparse,collections,json,shutil,os,subprocess
from contextlib import contextmanager
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


@contextmanager
def storage_mode(fields,absorb,enabled=True):
    settings={'CELL_CPU_REFERENCE_VIEWS':str(int(enabled)),
              'CELL_CPU_MARKER_FIELDS':fields,'CELL_CPU_ABSORB':str(int(absorb))}
    before={k:os.environ.get(k) for k in settings}
    try:
        os.environ.update(settings)
        yield
    finally:
        for k,v in before.items():
            if v is None:os.environ.pop(k,None)
            else:os.environ[k]=v


def marker_field_ablation(path,engine,out,provenance):
    """Only field ownership and takeover vary; source markers stay enabled."""
    lanes=[('control','copy',False,False),
           ('reference-no-takeover','reference',False,True),
           ('copy-no-takeover','copy',False,True),
           ('reference-takeover','reference',True,True),
           ('copy-takeover','copy',True,True)]
    audits={}
    for name,fields,absorb,enabled in lanes[1:]:
        with storage_mode(fields,absorb,enabled):
            r=subprocess.run([str(engine),'--program',str(path),'--verify-reference-views'],capture_output=True,text=True)
        (out/(name+'.audit.log')).write_text(r.stdout+r.stderr)
        if r.returncode:raise RuntimeError(r.stderr)
        audits[name]=json.loads(r.stdout)
        print(name,'audit passed',audits[name]['runs'],'Runs',flush=True)
    measured={};expected=None
    for name,fields,absorb,enabled in lanes:
        with storage_mode(fields,absorb,enabled):
            measurement,report=timed(path,engine,out/name)
        state=json.loads((out/name/'state.json').read_text())
        state['rows'].sort(key=lambda r:json.dumps(r,sort_keys=True))
        if expected is None:expected=state
        elif state!=expected:raise ValueError(name+' final graph differs')
        phases=collections.Counter()
        for step in report['steps']:phases.update(step['phases'])
        measured[name]={'measurement':measurement,'phase_seconds':dict(phases),
                        'report':{k:v for k,v in report.items() if k!='steps'}}
        print(name,measurement['inference_seconds'],'seconds',flush=True)
    result={'provenance':provenance,'verification':audits,'lanes':measured,
            'scope':'same binary and marked source; standalone Cell CPU; not pristine egglog; repair includes takeover and event retargeting'}
    (out/'results.json').write_text(json.dumps(result,indent=2)+'\n')


def main():
    p=argparse.ArgumentParser();source=p.add_mutually_exclusive_group(required=True);source.add_argument('--pipeline',type=Path);source.add_argument('--compiled',type=Path,help='reuse an already verified reference-view pipeline after engine-only changes');p.add_argument('--output',type=Path,required=True);p.add_argument('--engine',type=Path);p.add_argument('--marker-field-ablation',action='store_true',help='audit and time copied/referenced marker fields with takeover off/on plus a common source-marker control');a=p.parse_args()
    out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
    path,program,provenance=load_pipeline_artifact(a.compiled.resolve()) if a.compiled else reemit(a.pipeline.resolve(),out/'compile')
    if not program.get('reference_views',{}).get('marker_tables'):raise ValueError('compiled input has no reference marker contract; reemit the device pass')
    engine=a.engine or binary()
    if a.marker_field_ablation:return marker_field_ablation(path,engine,out,provenance)
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
