"""Check a changed worker against every already native-verified Run boundary."""
import argparse
import hashlib
import json
from pathlib import Path
from bench_dynamic import load,compile_events,digest
from dynamic_client import Resident

HERE=Path(__file__).resolve().parent
def audit(reference,worker,out,mode):
    out.mkdir(parents=True,exist_ok=False);w=load(reference,validate_runtime=False)
    # The edited compiler must emit exactly the same packets for this workload.
    _,compilation=compile_events(w)
    truth=json.loads((reference/'report.json').read_text());checks={c['native_run']:c for c in truth['checks']}
    gpu=Resident(worker,w['library'],out/'worker',mode);records=[]
    try:
        for op in w['operations']:
            if op['op']=='patch':
                gpu.apply(op);continue
            got=gpu.step();_,arrays=gpu.snapshot();expected=checks[op['native_run']]
            if got['updated']!=op['updated'] or got['runs_in_epoch']!=op['runs_in_epoch']:raise AssertionError('Run metadata differs')
            if hashlib.sha256(arrays.tobytes()).hexdigest()!=expected['state_sha256']:raise AssertionError('state differs at Run '+str(op['native_run']))
            records.append({'native_run':op['native_run'],'forecast_passes':got.get('forecast_passes',0),
                            'forecast_reused':got.get('forecast_reused',False)})
        final,arrays=gpu.snapshot()
        if hashlib.sha256(arrays.tobytes()).hexdigest()!=w['final_state_sha256']:raise AssertionError('final state differs')
        report={'scope':'changed GPU replay, exact every-Run comparison to preserved native validation; not timing',
                'mode':mode,'worker_sha256':digest(worker),'workload_sha256':digest(reference/'workload.json'),
                'sources':{n:digest(HERE/n) for n in ['compiler.py','dynamic.py','dynamic.metal','local.metal','kernel.metal','dynamic_worker.swift','dynamic_client.py']},
                'all_boundaries_exact':True,'checked_runs':len(records),'full_graph_check_reused':w['full_graph_check'],
                'compiler_packets_identical':compilation['all_packets_identical'],'final':final,'records':records}
        (out/'report.json').write_text(json.dumps(report,indent=2)+'\n');return report
    except Exception as e:
        (out/'failure.json').write_text(json.dumps({'error':str(e),'checked_runs':len(records),'records':records},indent=2)+'\n');raise
    finally:gpu.close()

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--reference',type=Path,required=True);p.add_argument('--worker',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True);p.add_argument('--mode',choices=['matrix','direct','local'],default='local')
    a=p.parse_args();r=audit(a.reference,a.worker,a.output,a.mode);print('exact boundaries:',r['checked_runs'])
