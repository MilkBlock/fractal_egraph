"""Replay the original native schedule; assist only one declared rule family.

External phases are a native event source. Predicate truth is never copied back
into GPU inference after bootstrap. Full snapshots are a diagnostic adapter,
not a claim of production commit-journal integration or end-to-end speedup.
"""
import argparse
import hashlib
import json
from pathlib import Path
import time
from dynamic import DynamicContext,pack
from dynamic_client import Resident
from dynamic_events import SnapshotEvents
from verify_dynamic import Oracle,full_check

def verify(source,frontend,worker,out,ruleset,capture_workload=False):
    out.mkdir(parents=True,exist_ok=False);native=Oracle(frontend,source,out)
    compiler=DynamicContext(native.core,ruleset);gpu=None;events=None;dirty=True
    steps=0;checks=[];epochs=[];adapter_seconds=0.;compile_seconds=0.;snapshot_rows=0
    operations=[];native_trace=[]
    if capture_workload:(out/'events').mkdir()
    try:
        # Every other phase may change the structural carrier. It must not
        # privately execute this analysis family or introduce unobserved seeds.
        for rs in native.core['rulesets']:
            if rs['name']==ruleset:continue
            if any(a.get('call',{}).get('table') in compiler.outputs for r in rs['rules'] for a in r['head']):
                raise ValueError('another scheduled ruleset writes the selected predicates')
        def update_context():
            nonlocal gpu,events,dirty,adapter_seconds,compile_seconds,snapshot_rows
            start=time.perf_counter();snapshot=native.ask({'op':'snapshot'});snapshot_rows+=len(snapshot['rows'])
            if gpu is None:
                events=SnapshotEvents(compiler.templates,snapshot['rows']);event=None
            else:event=events.diff(snapshot)
            adapter_seconds+=time.perf_counter()-start;start=time.perf_counter()
            if event is None:
                event={'upserts':[(i,r) for i,r in enumerate(snapshot['rows']) if r['table'] in compiler.templates.by_table],
                       'seeds':[(r['table'],r['values'][0],r['subsumed']) for r in snapshot['rows'] if r['table'] in compiler.outputs]}
            patch=compiler.apply(**event)
            compile_seconds+=time.perf_counter()-start
            if gpu is None:gpu=Resident(worker,compiler.library(),out/'worker')
            ack=gpu.apply(patch);epochs.append({'before_native_run':steps+1,'compiler':patch['stats'],'gpu':ack})
            if capture_workload:
                stem=f'events/{len(epochs):03}'
                (out/f'{stem}.json').write_text(json.dumps(event,separators=(',',':')))
                data=pack(patch['words']) if patch['words'] is not None else b''
                (out/f'{stem}.bin').write_bytes(data)
                operations.append({'op':'patch','event':f'{stem}.json','packet':f'{stem}.bin',
                                   'sha256':hashlib.sha256(data).hexdigest(),'stats':patch['stats']})
            native.ask({'op':'track_anchors'});dirty=False
        def advance(s):
            nonlocal steps,dirty
            if 'run' in s:
                name=s['run']
                if name==ruleset:
                    if dirty:update_context()
                    actual=gpu.step();expected=native.ask({'op':'run','set':name})
                    if actual['updated']!=expected['updated']:raise AssertionError(f'Run update differs at {steps+1}')
                    info,arrays=gpu.snapshot();decoded=compiler.decode(*arrays[:3])
                    truth=native.ask({'op':'unary_predicates','tables':compiler.outputs})
                    if decoded!=truth:raise AssertionError(f'predicate closure differs at {steps+1}')
                    checks.append({'native_run':steps+1,'epoch':compiler.epoch,'facts':sum(map(len,decoded.values())),
                                   'live_contexts':info['live_contexts'],'context_bytes':info['context_reserved_bytes'],
                                   'state_sha256':hashlib.sha256(arrays.tobytes()).hexdigest(),'exact':True})
                    if capture_workload:
                        operations.append({'op':'run','native_run':steps+1,'epoch':compiler.epoch,
                                           'updated':expected['updated'],'runs_in_epoch':actual['runs_in_epoch']})
                else:
                    expected=native.ask({'op':'run','set':name});dirty |= expected['updated']
                steps+=1
                if capture_workload:native_trace.append({'set':name,'updated':expected['updated']})
                if steps%256==0:
                    (out/'progress.json').write_text(json.dumps({'native_runs':steps,'checked_closure_runs':len(checks),'context_epoch':compiler.epoch}))
                return expected['updated']
            if 'sequence' in s:
                changed=False
                for child in s['sequence']:changed=advance(child) or changed
                return changed
            if 'repeat' in s:
                changed=False
                for _ in range(s['repeat']):
                    current=advance(s['body']);changed |= current
                    if not current:break
                return changed
            if 'saturate' in s:
                changed=False
                while advance(s['saturate']):changed=True
                return changed
            raise ValueError('schedule node')
        for schedule in native.core['schedule']:advance(schedule)
        if dirty:update_context()  # aliases are visible, but do not run extra rules
        info,arrays=gpu.snapshot();snapshot=native.ask({'op':'snapshot'});current=native.ask({'op':'current_program'})
        complete=full_check(compiler,current,snapshot,arrays)
        if capture_workload:
            arrays.astype('<u4').tofile(out/'final-state.bin')
            artifact={'schema':'matrix-dynamic-workload/v1','source':str(source.resolve()),
                'source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'ruleset':ruleset,
                'library':compiler.library(),'core':dict(native.core,initial=[]),
                'operations':operations,'native_trace':native_trace,'full_graph_check':complete,
                'final_state_sha256':hashlib.sha256(arrays.tobytes()).hexdigest(),
                'native_frontend_sha256':hashlib.sha256(frontend.read_bytes()).hexdigest(),
                'worker_sha256':hashlib.sha256(worker.read_bytes()).hexdigest(),
                'sources':{name:hashlib.sha256((Path(__file__).parent/name).read_bytes()).hexdigest()
                           for name in ['compiler.py','dynamic.py','dynamic.metal','kernel.metal','dynamic_worker.swift']}}
            (out/'workload.json').write_text(json.dumps(artifact,separators=(',',':')))
        report={'scope':'complete original native raytrace schedule; GPU replaces only selected closure computation; diagnostic external-event adapter',
          'source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'ruleset':ruleset,
          'native_runs':steps,'checked_closure_runs':len(checks),'epochs':epochs,'checks':checks,
          'full_graph_check':complete,'post_bootstrap_predicate_seeds':0,
          'event_snapshot_rows_scanned':snapshot_rows,'diagnostic_event_adapter_seconds':adapter_seconds,
          'incremental_wiring_compiler_seconds':compile_seconds,'final':info,
          'upload_bytes':gpu.up,'download_bytes_including_audit':gpu.down,
          'limits':['External phases execute in native egglog.','Snapshot differences supply events for validation; the production journal is not connected.',
                    'Matrix truth is resident, but changed-row binding compilation remains CPU control work.','This is not a full GPU end-to-end benchmark.']}
        (out/'report.json').write_text(json.dumps(report,indent=2)+'\n');return report
    except Exception as error:
        (out/'failure.json').write_text(json.dumps({'error':str(error),'native_runs':steps,'checks':checks,'epochs':epochs},indent=2)+'\n');raise
    finally:
        if gpu:gpu.close()
        native.close()

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--source',type=Path,required=True);p.add_argument('--frontend',type=Path,required=True)
    p.add_argument('--worker',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--ruleset',default='is-resolved')
    p.add_argument('--capture-workload',action='store_true',help='save compact events for fast timing replays, not full graph snapshots')
    a=p.parse_args();r=verify(a.source,a.frontend,a.worker,a.output,a.ruleset,a.capture_workload);print('verified',r['native_runs'],'native Runs;',r['checked_closure_runs'],'GPU closure Runs;',r['full_graph_check'])
