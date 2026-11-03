"""Full typed-graph and work comparison against the retained Raytrace control.

No native inference/rebuild repairs either graph. Physical allocation IDs may
change; the existing anchored typed graph comparator checks their bijection.
"""
import argparse,json,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
sys.path[:0]=[str(ROOT),str(ROOT/'experiments/device_backend')]
from run import compare
from export_snapshot import gpu_rows

def read(p):return json.loads(Path(p).read_text())

def sequence_normal_form(x):
    # Only associativity and singleton elimination of Sequence. In particular,
    # never flatten Saturate or change any Repeat count/Run visibility boundary.
    if isinstance(x,list):
        result=[]
        for child in x:
            value=sequence_normal_form(child)
            if isinstance(value,dict) and set(value)=={'sequence'}:result.extend(value['sequence'])
            else:result.append(value)
        return result
    if isinstance(x,dict):
        value={k:sequence_normal_form(v) for k,v in x.items()}
        return value['sequence'][0] if set(value)=={'sequence'} and len(value['sequence'])==1 else value
    return x

def report(suite,baseline):
    suite=read(suite);assert not suite['failures'],suite['failures']
    case=next(c for c in suite['cases'] if c['name']=='raytrace');assert suite['configuration']=='resident-core';assert case['marker_tables']==0
    new=Path(case['clean']['attempt']);old=baseline
    pa,pb=read(old/'program.json'),read(new/'program.json')
    ca=read(old.parent/'cell-0/core.json');cb=read(Path(case['cache']['compile']['directory'])/'artifact/core.json')
    assert sequence_normal_form(ca)==sequence_normal_form(cb),'different typed program beyond redundant Sequence wrappers'
    for key in ['tables','strings','anchors','initial_rows','initial_sets','action_code','set_rule_ids','capacity','match_capacity','id_capacity']:
        assert pa[key]==pb[key],key
    assert pa['generic_cell']['control']==pb['generic_cell']['control'],'compiled control differs'
    assert sequence_normal_form(pa['resident_schedules']['programs'])==sequence_normal_form(pb['resident_schedules']['programs'])
    a,b=read(old/'result.json'),read(new/'result.json');sa,sb=a['instances'][0],b['instances'][0]
    assert len(a['instances'])==len(b['instances'])==1
    assert sa['completed'] and sb['completed'] and not sa['meta'][4] and not sb['meta'][4]
    differences=[key for key in ['rows','parents','fact_parents','sets','watermarks','meta','history'] if sa[key]!=sb[key]]
    def root(x):
        while sa['parents'][x]!=x:x=sa['parents'][x]
        return x
    print('checking entire typed graph, raw differences:',differences,flush=True)
    reference=dict(rows=gpu_rows(pa,sa),anchors=[dict(sort=s,old=value,now=f'gpu:{root(i+1)}') for i,(s,value) in enumerate(pa['anchors'])])
    exact=compare(pb,sb['rows'],sb['parents'],reference,sb['sets'])
    print('typed graph equal:',exact,flush=True)
    private_fields=['private_schedule_regions','private_schedule_runs','private_schedule_rows','private_schedule_forwarded_reads','private_schedule_binding_outputs']
    assert all(a[k]==b[k] for k in private_fields),'private work differs'
    old_profile=read(ROOT/'out/profile-restored-full-encoder-v2/summary.json')
    new_profile=read(Path(case['cache']['encoders']['directory'])/'profile/summary.json')
    groups={k:dict(old_diagnostic_seconds=old_profile['groups'].get(k,{}).get('seconds',0),new_diagnostic_seconds=new_profile['groups'].get(k,{}).get('seconds',0)) for k in old_profile['groups'].keys()|new_profile['groups'].keys()}
    phases={k:dict(old_diagnostic_seconds=old_profile['phases'].get(k,{}).get('seconds',0),new_diagnostic_seconds=new_profile['phases'].get(k,{}).get('seconds',0)) for k in old_profile['phases'].keys()|new_profile['phases'].keys() if k.startswith('resident_packet_') or k in ['batched_cell_publish','batched_cell_match','batched_cell_pack']}
    old_work=read(ROOT/'out/resident-restoration-pass3/results.json')['candidate']['work']
    new_work=case['clean']['execution']
    hot=[]
    for block in new_profile['hot_commands']:
        if block['run_before']!=block['run_after_observed']:continue
        done=block['run_before'][0];instruction=pb['generic_cell']['control']['instructions'][block['pc_before']]
        if instruction[0]!=0:continue
        hot.append(dict(outer_command=block['outer_before'],run_number=done+1,ruleset=pb['sets'][instruction[1]],
            final_run_binding_count=sb['history'][done][2],rhs_encoder_seconds=block['encoder_seconds'].get('batched_cell_publish',0),
            packet_encoder_calls=sum(n for k,n in block['encoder_calls'].items() if k.startswith('resident_packet_')),
            scope='one command wholly inside this Run; time includes publisher maintenance, not only these RHS actions'))
    return dict(schema='eggfuse-rhs-packets-raytrace/v1',suite_generated_unix=suite['generated_unix'],configuration='resident-core',source_markers=0,
      measurement='actual complete Raytrace Pass 3 GPU runtime; cached historical control, single new clean measurement; compile and extraction excluded',
      same_typed_source_modulo_sequence_wrappers=True,same_compiled_control=True,graph_verification=exact,raw_differences=differences,
      history_equal=sa['history']==sb['history'],watermarks_equal=sa['watermarks']==sb['watermarks'],private_work_equal=True,
      inference_seconds=dict(old=a['inference_seconds'],new=b['inference_seconds'],new_over_old=b['inference_seconds']/a['inference_seconds']),
      gpu_seconds=dict(old=a['gpu_seconds'],new=b['gpu_seconds']),storage_bytes=dict(old=a['storage_bytes'],new=b['storage_bytes']),
      outer_commands=dict(old=a['dispatches'],new=b['dispatches']),packet_readiness_polls=b['packet_readiness_polls'],packet_dispatch_quanta=b['packet_dispatch_quanta'],
      final_rows=len(sb['rows']),logical_runs=len(sb['history']),private_regions=b['private_schedule_regions'],private_runs=b['private_schedule_runs'],
      work=new_work,old_work=old_work,net_additional_parallel_fresh_rows=new_work['packet_fresh_rows']+new_work['wave_fresh_rows']-old_work['parallel_constructor_rows']-old_work['wave_fresh_rows'],hot_single_run_blocks=hot,diagnostic_groups=groups,diagnostic_phases=phases,
      profile_verification=new_profile['verification'],profile_seconds=dict(old=old_profile['profiled_inference_seconds'],new=new_profile['profiled_inference_seconds']),
      worker_sha256=b['worker_sha256'],program_sha256=b['program_sha256'],source_sha256=case['source_sha256'],
      control=str(old/'result.json'),candidate=str(new/'result.json'),profile=str(Path(case['cache']['encoders']['directory'])/'profile/summary.json'),dashboard=case['dashboard'])

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--suite',type=Path,default=Path('out/profile-suite/latest.json'));p.add_argument('--baseline',type=Path,default=Path('out/resident-restoration-pass3/pass3-on/program-0/attempt-0'));p.add_argument('--output',type=Path,default=Path('experiments/rhs_packets/raytrace.json'));a=p.parse_args()
    r=report(a.suite,a.baseline);a.output.write_text(json.dumps(r,indent=2)+'\n');print('timing:',r['inference_seconds'],flush=True)
