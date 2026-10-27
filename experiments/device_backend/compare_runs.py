"""Audit a same-state device comparison; never label a resumed suffix as a full run."""
import argparse,json
from pathlib import Path

def compare(before,after,device_states=False,endpoints_only=False,semantic_device=False,saturation_macros=False):
    reports=[json.loads((p/'report.json').read_text()) for p in [before,after]]
    programs=[json.loads((p/'program.json').read_text()) for p in [before,after]]
    a,b=reports;pa,pb=programs
    if a['source_sha256']!=b['source_sha256']:raise ValueError('different sources')
    input_keys=['tables','strings','anchors','initial_rows','initial_sets','initial_parents','initial_fact_parents','initial_watermarks','meta','capacity','match_capacity']
    if endpoints_only:
        if not device_states:raise ValueError('endpoint comparison requires device states')
        cores=[json.loads((p/'core.json').read_text()) for p in [before,after]]
        if cores[0]!=cores[1]:raise ValueError('different original typed programs')
        input_keys=['tables','strings','anchors']
    for key in input_keys:
        if pa.get(key)!=pb.get(key):raise ValueError('different input/configuration: '+key)
    if semantic_device:
        if not device_states:raise ValueError('semantic device comparison requires device states')
        from run import compare as compare_graph
        from export_snapshot import gpu_rows
        sa,sb=[json.loads((p/'gpu-state.json').read_text()) for p in [before,after]]
        # Compare the actual device graphs without replay, native import or rebuild.
        # Allocation order is not part of graph equality. Typed primitive values,
        # visibility, hashcons keys and the entire anchored equality partition are.
        def root(i):
            seen=set()
            while sa['parents'][i]!=i:
                if i in seen:raise ValueError('invalid device forest')
                seen.add(i);i=sa['parents'][i]
            return i
        reference={'rows':gpu_rows(pa,sa),'anchors':[
            {'sort':s,'old':old,'now':f'gpu:{root(i+1)}'}
            for i,(s,old) in enumerate(pa['anchors'])]}
        final_check=compare_graph(pb,sb['rows'],sb['parents'],reference,sb['sets'])
        final_check.update(native_reference=False,identity_contract='typed graph bijection; physical allocation order may differ')
    elif device_states:
        snapshots=[json.loads((p/'gpu-state.json').read_text()) for p in [before,after]]
        def roots(forest):
            result=[]
            for i,parent in enumerate(forest):
                if parent<0 or parent>i:raise ValueError('invalid minimum-root forest')
                result.append(i if parent==i else result[parent])
            return result
        for key in snapshots[0].keys()|snapshots[1].keys():
            if key in ['parents','fact_parents']:
                if roots(snapshots[0][key])!=roots(snapshots[1][key]):raise ValueError('device equivalence differs: '+key)
            elif key=='rows':
                # Slots past a table's declared arity are workspace padding,
                # not facts. Compaction/resume may leave different old bytes.
                fields=lambda snap:[r[:4+len(pa['tables'][r[0]]['schema'])] for r in snap['rows']]
                if fields(snapshots[0])!=fields(snapshots[1]):raise ValueError('device state differs: declared row fields')
            elif snapshots[0].get(key)!=snapshots[1].get(key):raise ValueError('device state differs: '+key)
        final_check={'exact_device_state':snapshots[0]==snapshots[1],'exact_rows_values_equivalence_visibility':True,'rows':len(snapshots[1]['rows']),'native_reference':False}
    else:
        for r in reports:
            if not r['checked'] or not r['checks'] or not r['checks'][-1]['exact_rows_values_equivalence_visibility']:raise ValueError('missing exact final check')
            if r['checks'][-1]['step']!=len(r['steps']):raise ValueError('check is not final')
        final_check=b['checks'][-1]
    order=lambda r:[(s['set'],s['updated']) for s in r['steps']]
    if saturation_macros:
        if not semantic_device:raise ValueError('compacted saturation requires complete typed graph comparison')
        if not all(r['complete_schedule'] and not r['resumed_prefix_runs'] for r in reports):raise ValueError('macro comparison requires both complete schedules from initial state')
        if pa['schedule']!=pb['schedule'] or json.loads((before/'core.json').read_text())!=json.loads((after/'core.json').read_text()):raise ValueError('different source schedule/core')
        final_check['original_schedule_equal']=True
        final_check['internal_run_history_equal']=order(a)==order(b)
    elif order(a)!=order(b):raise ValueError('different executed schedule')
    if endpoints_only:
        return {'scope':'GPU endpoint comparison; no native-equivalence or speedup claim','before':str(before),'after':str(after),
                'original_typed_program_equal':True,'executed_runs':len(a['steps']),'final_check':final_check}
    if a['resumed_prefix_runs']!=b['resumed_prefix_runs']:raise ValueError('different resumed prefix')
    phases={}
    for key in ['device_control_wait','device_gpu','prepare','match','commit']:
        x,y=[r['timings_seconds'][key] for r in reports];phases[key]={'before_seconds':x,'after_seconds':y,'ratio':x/y if y else None}
    scope='full schedule' if a['complete_schedule'] and not a['resumed_prefix_runs'] else 'checked schedule suffix/prefix only, not full end-to-end acceleration'
    return {'scope':scope,'before':str(before),'after':str(after),'source_sha256':a['source_sha256'],'resumed_prefix_runs':a['resumed_prefix_runs'],'measured_runs':len(a['device_command_times']),'final_check':final_check,'timings':phases,'builds':[r['build'] for r in reports]}

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('before',type=Path);p.add_argument('after',type=Path);p.add_argument('--output',type=Path);p.add_argument('--device-states',action='store_true');p.add_argument('--semantic-device-states',action='store_true');p.add_argument('--endpoints-only',action='store_true');p.add_argument('--saturation-macros',action='store_true');a=p.parse_args();r=compare(a.before,a.after,a.device_states or a.semantic_device_states,a.endpoints_only,a.semantic_device_states,a.saturation_macros)
    if a.output:a.output.write_text(json.dumps(r,indent=2)+'\n')
    print(json.dumps(r,indent=2))
