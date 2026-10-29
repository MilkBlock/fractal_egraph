"""Summarize a completed or failed unified pipeline without rerunning inference."""
import argparse,hashlib,json,sys
from pathlib import Path
from .multistage import execution_summary
ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'experiments/device_backend'))

def normal_schedule(s):
    if 'sequence' in s:
        xs=list(map(normal_schedule,s['sequence']))
        return xs[0] if len(xs)==1 else {'sequence':xs}
    if 'saturate' in s:return {'saturate':normal_schedule(s['saturate'])}
    if 'repeat' in s:return {'repeat':s['repeat'],'body':normal_schedule(s['body'])}
    return s

def typed_input(core):
    return {**core,'schedule':list(map(normal_schedule,core['schedule']))}

def summarize(folder,reference=None):
    load=lambda p:json.loads(p.read_text())
    receipts=load(folder/'pipeline.json');failed=receipts.get('failed')
    runs=[Path(r['directory']) for r in receipts['passes'] if r['name']=='verify']
    if failed and failed['pass_name']=='verify':runs.append(Path(failed['directory']))
    if not runs or not (runs[-1]/'gpu/report.json').exists():raise ValueError('no completed GPU inference in pipeline')
    run=runs[-1]/'gpu';r=load(run/'report.json');p=load(run/'program.json');state=load(run/'gpu-state.json')
    result=dict(schema='eggfuse-pipeline-run-summary/v1',pipeline=str(folder.resolve()),run=str(run.resolve()),
        scope='actual GPU-owned full source schedule; compiler/setup/export excluded from inference time',
        complete_schedule=r['complete_schedule'],inference_seconds=r['inference_seconds'],gpu_setup_seconds=r['setup_seconds'],
        clean_timing=r['clean_timing'],timings=r['timings_seconds'],rows=len(state['rows']),
        source_sha256=r['source_sha256'],program_sha256=r['program_sha256'],
        worker_sha256=r['build']['worker_sha256'],frontend_sha256=r['build']['frontend_sha256'],
        marker_program=p['marker_program'],multi_stage_cell=execution_summary(r),
        passes=[{k:v for k,v in x.items() if k in ['name','cached','original_seconds']} for x in receipts['passes']],
        verification_failure=failed)
    refs={x['name']:Path(x['directory'])/'result/native-state.json' for x in receipts['passes'] if x['name'] in ['sk_native','marked_native']}
    from .verify import compare_source
    from run import compare
    if len(refs)==2:
        marker=load(folder/'marker-program.json');sk=load(refs['sk_native']);mk=load(refs['marked_native'])
        result['source_check']=compare_source(sk,mk,marker)
        try:result['native_check']=compare(p,state['rows'],state['parents'],mk,state['sets'])
        except (ValueError,AssertionError) as e:result['native_check']=dict(exact_rows_values_equivalence_visibility=False,error=str(e),native_rows=len(mk['rows']))
    if reference:
        if typed_input(load(reference/'core.json'))!=typed_input(load(run/'core.json')):
            raise ValueError('historical reference typed input/rules/schedule differ')
        old=load(reference/'report.json');op=load(reference/'program.json');os=load(reference/'gpu-state.json')
        if not old['complete_schedule'] or old['resumed_prefix_runs']:raise ValueError('reference is not a complete initial-state run')
        from export_snapshot import gpu_rows
        def root(i):
            seen=set()
            while os['parents'][i]!=i:
                if i in seen:raise ValueError('reference parent cycle')
                seen.add(i);i=os['parents'][i]
            return i
        native=dict(rows=gpu_rows(op,os),anchors=[dict(sort=s,old=oldid,now=f'gpu:{root(i+1)}') for i,(s,oldid) in enumerate(op['anchors'])])
        check=compare(p,state['rows'],state['parents'],native,state['sets'])
        result['historical_device_reference']=dict(directory=str(reference.resolve()),inference_seconds=old['inference_seconds'],
            source_sha256=old['source_sha256'],worker_sha256=old['build']['worker_sha256'],
            final_check=check,typed_input_equal_modulo_singleton_sequences=True,
            native_reference=False,paired_speedup=False,
            note='unchanged saved GPU endpoint; historical time, not a fresh same-build ablation')
    (folder/'run-summary.json').write_text(json.dumps(result,indent=2)+'\n')
    return result

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('pipeline',type=Path);p.add_argument('--reference',type=Path);a=p.parse_args()
    r=summarize(a.pipeline,a.reference);print(json.dumps(r,indent=2))
if __name__=='__main__':main()
