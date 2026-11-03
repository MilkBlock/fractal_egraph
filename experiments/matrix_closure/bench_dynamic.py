"""One-shot, native-checked timing replay of dynamic matrix closure events.

Capture once with verify_dynamic_raytrace --capture-workload. No graph snapshots,
oracle predicates or final-state readback occur inside GPU timing regions.
This is a component benchmark: external event production and table handoff are
not implemented by the standalone executor and cannot be claimed as free.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import struct
import time
import numpy as np
from dynamic import DynamicContext,pack
from dynamic_client import Resident
from verify_dynamic import Oracle,full_check

HERE=Path(__file__).resolve().parent
def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()

def groups(operations,coalesce):
    """Never cross an external phase, patch or native quiet boundary."""
    pending=[]
    for op in operations:
        compatible=(coalesce and pending and pending[-1]['updated'] and
                    op['op']=='run' and op['epoch']==pending[-1]['epoch'] and
                    op['native_run']==pending[-1]['native_run']+1)
        if pending and not compatible:yield pending;pending=[]
        if op['op']=='patch':yield op
        else:pending.append(op)
    if pending:yield pending

def bank_sizes(words,banks):
    if words is None:return
    at=8+words[4]+words[5]*2+words[6]*3
    for _ in range(words[7]):
        k,old,n,updates=words[at:at+4];at+=4+updates*(k+3)
        assert banks.get(k,0)==old
        if n:banks[k]=n
        else:banks.pop(k,None)
    assert at==len(words)

def load(path,validate_runtime=True):
    w=json.loads((path/'workload.json').read_text())
    if w['schema']!='matrix-dynamic-workload/v1':raise ValueError('workload schema')
    if not w['full_graph_check']['exact_rows_values_equivalence_visibility']:raise ValueError('unverified workload')
    for name,sha in w['sources'].items():
        if validate_runtime and digest(HERE/name)!=sha:raise ValueError('changed verified runtime source: '+name)
    if digest(Path(w['source']))!=w['source_sha256']:raise ValueError('changed native input')
    if digest(path/'final-state.bin')!=w['final_state_sha256']:raise ValueError('damaged reference state')
    for op in w['operations']:
        if op['op']!='patch':continue
        data=(path/op['packet']).read_bytes()
        if hashlib.sha256(data).hexdigest()!=op['sha256']:raise ValueError('damaged event packet')
        words=list(struct.unpack('<'+'I'*(len(data)//4),data)) if data else None
        if words is not None:
            if words.pop(0)!=len(data)-4:raise ValueError('packet size')
        op['words']=words
        op['input_event']=json.loads((path/op['event']).read_text())
    return w

def compile_events(w):
    start=time.perf_counter();d=DynamicContext(w['core'],w['ruleset']);schema_seconds=time.perf_counter()-start
    events=[]
    for op in w['operations']:
        if op['op']!='patch':continue
        start=time.perf_counter();patch=d.apply(**op['input_event']);seconds=time.perf_counter()-start
        if patch['words']!=op['words'] or patch['stats']!=op['stats']:raise AssertionError('changed incremental wiring')
        events.append(dict(patch['stats'],seconds=seconds,packet_bytes=len(pack(patch['words'])) if patch['words'] else 0))
    return d,{'schema_seconds':schema_seconds,'initial_seconds':events[0]['seconds'],
              'later_seconds':sum(e['seconds'] for e in events[1:]),'events':events,
              'all_packets_identical':True}

def native_lane(w,frontend,compiler,path,reference):
    path.mkdir();start=time.perf_counter();native=Oracle(frontend,Path(w['source']),path)
    setup=time.perf_counter()-start
    try:
        result=native.ask({'op':'time_runs','sets':[s['set'] for s in w['native_trace']]})
        if len(result['steps'])!=len(w['native_trace']):raise AssertionError('native Run count')
        selected=[];costs={}
        for op,step in zip(w['native_trace'],result['steps']):
            if op['updated']!=step['updated']:raise AssertionError('native control flow changed')
            costs[op['set']]=costs.get(op['set'],0.)+step['seconds']
            if op['set']==w['ruleset']:selected.append(step)
        # No exports inside the timed Run loop; check the complete graph once.
        current=native.ask({'op':'current_program'});snapshot=native.ask({'op':'snapshot'})
        arrays=np.fromfile(reference/'final-state.bin',dtype='<u4').reshape(4,-1)
        complete=full_check(compiler,current,snapshot,arrays)
        report={'setup_seconds':setup,'all_native_runs_seconds':result['seconds'],
                'selected_seconds':sum(s['seconds'] for s in selected),'selected_runs':len(selected),
                'selected_logical_matches':sum(s['logical_matches'] for s in selected),
                'ruleset_seconds':costs,'full_graph_check':complete,'all_update_flags_identical':True,
                'scope':'native inference only, no per-Run IPC or graph export; not pristine upstream'}
        (path/'report.json').write_text(json.dumps(report,indent=2)+'\n');return report
    finally:native.close()

def gpu_lane(w,worker,path,mode,coalesce):
    start=time.perf_counter();gpu=Resident(worker,w['library'],path,mode);setup=time.perf_counter()-start
    records=[];banks={};checks=0;epochs=0;allocations=0
    try:
        for item in groups(w['operations'],coalesce):
            if isinstance(item,dict):
                if item['words'] is None:continue
                start=time.perf_counter();info=gpu.apply({'words':item['words']});wall=time.perf_counter()-start
                bank_sizes(item['words'],banks);epochs+=1;allocations+=info['buffers_resized']
                record={'phase':'patch','wall_seconds':wall,'gpu_seconds':info['gpu_seconds'],
                        'epoch':info['epoch'],'bytes':len(pack(item['words']))}
            else:
                start=time.perf_counter();info=gpu.step(len(item));wall=time.perf_counter()-start
                if (info['runs_executed']!=len(item) or info['runs_in_epoch']!=item[-1]['runs_in_epoch']
                    or info['updated']!=item[-1]['updated'] or info['any_updated']!=any(s['updated'] for s in item)):
                    raise AssertionError('GPU schedule boundary changed')
                checks+=len(item)
                record={'phase':'run','wall_seconds':wall,'gpu_seconds':info['gpu_seconds'],
                        'worker_control_seconds':info['control_seconds'],'runs':len(item),'batches':info['batches'],
                        'first_native_run':item[0]['native_run'],'last_native_run':item[-1]['native_run'],
                        'new_facts':info['new_facts'],'contexts':sum(banks.values()),
                        'submitted_buffers':info.get('submitted_buffers',1+info['batches']),
                        'forecast_passes':info.get('forecast_passes',0),'forecast_reused':info.get('forecast_reused',False),
                        'layout_record_visits':len(item)*sum(banks.values()) if mode!='local' else 0,
                        'layout_padded_flops':len(item)*sum(1024*(k+1)*((n+63)//64) for k,n in banks.items())}
            records.append(record)
        up,down=gpu.up,gpu.down
        final,arrays=gpu.snapshot() # audit excluded from every timing region
        if hashlib.sha256(arrays.tobytes()).hexdigest()!=w['final_state_sha256']:raise AssertionError('GPU final identity/truth/visibility/birth differs')
        assert not gpu.commands
        report={'mode':mode,'coalesced':coalesce,'setup_seconds':setup,'runs':checks,'epochs':epochs,
                'buffer_resize_operations':allocations,'exact_final_state':True,'boundary_checks_passed':True,
                'upload_bytes':up,'download_bytes_excluding_audit':down,'final':final,'operations':records}
        for phase in ['patch','run']:
            rs=[r for r in records if r['phase']==phase]
            report[phase]={'calls':len(rs),'wall_seconds':sum(r['wall_seconds'] for r in rs),
                           'gpu_seconds':sum(r['gpu_seconds'] for r in rs)}
        report['replay_seconds']=report['patch']['wall_seconds']+report['run']['wall_seconds']
        report['layout_record_visits']=sum(r.get('layout_record_visits',0) for r in records) if mode!='local' else None
        report['forecast_passes']=sum(r.get('forecast_passes',0) for r in records)
        report['forecast_reuse_calls']=sum(r.get('forecast_reused',False) for r in records)
        report['new_facts']=sum(r.get('new_facts',0) for r in records)
        report['nominal_matrix_flops']=sum(r.get('layout_padded_flops',0) for r in records) if mode=='matrix' else 0
        report['command_buffers']=epochs+sum(r.get('submitted_buffers',0) for r in records)
        (path/'report.json').write_text(json.dumps(report,indent=2)+'\n');return report
    except Exception as error:
        (path/'failure.json').write_text(json.dumps({'error':str(error),'operations':records},indent=2)+'\n');raise
    finally:gpu.close()

def bench(reference,frontend,worker,out,reuse_native=None,candidate_audit=None,modes=None):
    if os.environ.get('MTL_DEBUG_LAYER')=='1':raise ValueError('disable Metal validation for timing; capture already verifies it')
    w=load(reference,validate_runtime=candidate_audit is None)
    if digest(frontend)!=w['native_frontend_sha256']:raise ValueError('native binary must match capture')
    if candidate_audit is None:
        if digest(worker)!=w['worker_sha256']:raise ValueError('changed worker requires a candidate audit')
    else:
        audit=json.loads(candidate_audit.read_text())
        if (not audit['all_boundaries_exact'] or audit['workload_sha256']!=digest(reference/'workload.json')
            or audit['worker_sha256']!=digest(worker)):raise ValueError('candidate audit does not match')
        for name,sha in audit['sources'].items():
            if digest(HERE/name)!=sha:raise ValueError('changed audited source: '+name)
    out.mkdir(parents=True,exist_ok=False)
    compiler,compilation=compile_events(w)
    if reuse_native is not None:
        old=json.loads((reuse_native/'report.json').read_text())
        old_input=json.loads((Path(old['reference'])/'workload.json').read_text())
        if (old['frontend_sha256']!=digest(frontend) or old['source_sha256']!=w['source_sha256']
            or old_input['native_trace']!=w['native_trace']
            or not old['native']['full_graph_check']['exact_rows_values_equivalence_visibility']):
            raise ValueError('native history input, binary, schedule or verification differs')
        native=dict(old['native'],reused_from=str(reuse_native))
    else:native=native_lane(w,frontend,compiler,out/'native',reference)
    lanes={}
    # Each configuration once, sequential; never select the best repetition.
    selected=modes.split(',') if modes else ['matrix-step','direct-step','matrix-batched','direct-batched']
    for name in selected:
        mode,cadence=name.split('-');coalesce=cadence=='batched'
        if mode not in ['matrix','direct','local'] or cadence not in ['step','batched']:raise ValueError('unknown lane')
        name=mode+('-batched' if coalesce else '-step')
        lanes[name]=gpu_lane(w,worker,out/name,mode,coalesce)
        print(name,lanes[name]['replay_seconds'],flush=True)
    source_report=json.loads((reference/'report.json').read_text())
    report={'scope':'resident dynamic is-resolved component on actual Raytrace changes; not full GPU Pass 3',
            'reference':str(reference),'workload_sha256':digest(reference/'workload.json'),
            'frontend_sha256':digest(frontend),'worker_sha256':digest(worker),'source_sha256':w['source_sha256'],
            'sources':{name:digest(HERE/name) for name in ['bench_dynamic.py','verify_dynamic_raytrace.py','dynamic_client.py']},
            'candidate_audit':str(candidate_audit) if candidate_audit else None,
            'compilation':compilation,'native':native,'lanes':lanes,
            'historical_validation_adapter_seconds':source_report['diagnostic_event_adapter_seconds'],
            'historical_validation_rows_scanned':source_report['event_snapshot_rows_scanned'],
            'missing_production_costs':['committed structural event delivery','incremental table publication'],
            'join_depth_metrics':'not applicable to fixed grounded unary clauses; native candidate counters not measured',
            'no_end_to_end_speedup_claim':True}
    (out/'report.json').write_text(json.dumps(report,indent=2)+'\n');return report

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--reference',type=Path,required=True);p.add_argument('--frontend',type=Path,required=True)
    p.add_argument('--worker',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    p.add_argument('--reuse-native',type=Path,help='reuse a passed same-input/binary/schedule control; do not rerun unchanged CPU')
    p.add_argument('--candidate-audit',type=Path);p.add_argument('--modes',help='comma separated matrix/direct/local-step/batched lanes')
    a=p.parse_args();r=bench(a.reference,a.frontend,a.worker,a.output,a.reuse_native,a.candidate_audit,a.modes)
    print('native selected seconds:',r['native']['selected_seconds'],'wiring:',r['compilation']['initial_seconds']+r['compilation']['later_seconds'])
