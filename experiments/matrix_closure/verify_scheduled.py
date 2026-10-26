"""Native A/B/C differential validation of forecast visibility and invalidation."""
import argparse
import json
from pathlib import Path
from dynamic import DynamicContext
from dynamic_client import Resident
from dynamic_events import SnapshotEvents
from verify_dynamic import Oracle,full_check

HERE=Path(__file__).resolve().parent
def verify(frontend,worker,out):
    out.mkdir(parents=True,exist_ok=False);native=Oracle(frontend,HERE/'scheduled_fixture.egg',out)
    schedule=['A','B','C'];d=DynamicContext(native.core,schedule);events=SnapshotEvents(d.templates,native.core['initial'])
    gpu=Resident(worker,d.library(),out/'worker','local');clock=0;checks=[]
    def check(label):
        info,arrays=gpu.snapshot();view=d.decode(*arrays[:3]);expected=native.ask({'op':'unary_predicates','tables':d.outputs})
        if view!=expected:raise AssertionError((label,view,expected))
        checks.append({'label':label,'clock':clock,'view':view,'forecast_ready':info['forecast_ready'],'phase_offset':info['phase_offset']})
        return arrays
    def advance():
        nonlocal clock
        name=schedule[clock%len(schedule)];got=gpu.advance();ref=native.ask({'op':'run','set':name});clock+=1
        if got['updated']!=ref['updated']:raise AssertionError('updated '+name)
        check(name);return got
    def edit(source,seeds=()):
        native.ask({'op':'edit_actions','source':source});s=native.ask({'op':'snapshot'})
        ack=gpu.apply(d.apply(**events.diff(s),seeds=seeds));native.ask({'op':'track_anchors'})
        if ack['forecast_ready'] or ack['forecast_reserved_bytes']!=0:raise AssertionError('expired forecast retained')
        check(source)
    try:
        gpu.apply(d.bootstrap(native.core['initial']));native.ask({'op':'track_anchors'});check('initial')
        first=advance()
        assert first['forecast_max_logical_time']>1,'future computation should exist privately'
        assert len(checks[-1]['view']['P'])==1 and not checks[-1]['view']['Q'] and not checks[-1]['view']['R']
        # Retire the Link and overwrite the function before their predicted B
        # effects become visible. No Q(b) or Q(Blocked 7) may leak out.
        edit('(subsume (Link a)) (set (Pick a) (Blocked 8))')
        advance()
        assert len(checks[-1]['view']['Q'])==1
        edit('(union b (Blocked 8))')
        snapshot=native.ask({'op':'snapshot'})
        value=next(r['values'][-1] for r in snapshot['rows'] if r['table']=='Blocked' and r['values'][0].get('value')==8)
        edit('(subsume (Q b))',seeds=[('Q',value,True)])
        advance();assert not checks[-1]['view']['R']
        edit('(let fresh (Seed 9)) (Link (Link (Link fresh)))')
        for _ in range(6):advance()
        assert clock%3==0
        before=clock;got=gpu.saturate();any_updated=False
        while True:
            changed=False
            for name in schedule:
                update=native.ask({'op':'run','set':name})['updated'];changed |= update;clock+=1
            any_updated |= changed
            if not changed:break
        assert got['runs_executed']==clock-before and got['any_updated']==any_updated
        arrays=check('joint saturation')
        # Explicit phases do not stop at the first quiet ruleset.
        quiet=gpu.advance(2)
        for _ in range(2):
            ref=native.ask({'op':'run','set':schedule[clock%3]});assert not ref['updated'];clock+=1
        assert quiet['runs_executed']==2 and not quiet['any_updated']
        arrays=check('two explicit quiet phases')
        rejected,_=gpu.send([3,gpu.epoch],allow_error=True)
        assert not rejected['ok'],'joint saturate must reject a mid-cycle entry'
        check('mid-cycle saturation rejected')
        full=full_check(d,native.ask({'op':'current_program'}),native.ask({'op':'snapshot'}),arrays)
        report={'scope':'native three-ruleset positive closure, GPU private logical-time forecasts; not arbitrary eggcc effects',
                'checks':checks,'all_boundaries_exact':True,'native_runs':clock,'full_graph_check':full,
                'future_computed_before_first_B':first['forecast_max_logical_time']>1,
                'function_overwrite_subsume_and_alias_invalidation':True,
                'explicit_quiet_phases_and_midcycle_rejection':True,
                'joint_saturation':got}
        (out/'report.json').write_text(json.dumps(report,indent=2)+'\n');return report
    except Exception as e:
        (out/'failure.json').write_text(json.dumps({'error':str(e),'checks':checks},indent=2)+'\n');raise
    finally:gpu.close();native.close()

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--frontend',type=Path,required=True);p.add_argument('--worker',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True);a=p.parse_args();r=verify(a.frontend,a.worker,a.output)
    print(r['native_runs'],'native Runs:',r['full_graph_check'])
