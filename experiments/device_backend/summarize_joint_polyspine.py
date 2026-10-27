"""Read saved exact checks and report cross-ruleset reuse without rerunning."""
import argparse,json,hashlib
from pathlib import Path
from compare_runs import compare

def summarize(reference,runs,output):
    before=json.loads((reference/'report.json').read_text());lanes=[]
    for directory in runs:
        check=compare(reference,directory,device_states=True,semantic_device=True,saturation_macros=True)
        r=json.loads((directory/'report.json').read_text());calls=r['joint_polyspine']['calls'];accepted=[c for c in calls if c['accepted']]
        sums=lambda field:sum(c['statistics'][field] for c in accepted)
        model=r['joint_polyspine']['program']['program']['model']
        lane={'directory':str(directory),'source_sha256':r['source_sha256'],'program_sha256':r['program_sha256'],
              'worker_sha256':r['build']['worker_sha256'],'frontend_sha256':r['build']['frontend_sha256'],
              'report_sha256':hashlib.sha256((directory/'report.json').read_bytes()).hexdigest(),
              'clean_timing':r['clean_timing'],'inference_seconds':r['inference_seconds'],'setup_seconds':r['setup_seconds'],
              'feature_refresh_mode':r['joint_polyspine'].get('feature_refresh','full'),
              'feature_refresh':{key:sum(c.get('feature_refresh',{}).get(key,0) for c in accepted) for key in ['context_checks','base_rows_recomputed','base_rows_reused','seam_rows_recomputed','seam_rows_reused','encoding_row_lanes','ridge_row_lanes','exact_audit_rows']},
              'model_enabled':model is not None,'training_capture':r['joint_polyspine']['training'],
              'final_check':check['final_check'],'joint_calls':len(calls),'accepted':len(accepted),
              'fallback_reasons':{str(reason):sum(c['reason']==reason for c in calls if not c['accepted']) for reason in {c['reason'] for c in calls if not c['accepted']}},
              'interface_builds':len(accepted),'marker_binding_visits':sum(c['marker_bindings'] for c in accepted),
              'root_context_visits':sum(c['root_contexts'] for c in accepted),
              'service_histogram':{str(mask):sums(80+mask) for mask in range(8)},
              'private_phases_by_stage':{str(p):sum(sum(h[0]==p for h in c['phase_history']) for c in accepted) for p in range(3)},
              'fresh_private_constructor_rows':{'terms':sums(56),'helpers':sums(57)},'function_value_updates':sums(61),
              'surviving_new_rows':sums(88),'updated_existing_rows':sums(89),
              'cross_stage_successful_binding_read_uses':sums(59),'cross_stage_version_consumer_pairs':sums(60),
              'matrix_rhs_rows':sums(54),'exact_integer_fallback_rows':sums(55),
              'positive_prediction_counts':[sums(92+j) for j in range(3)],
              'global_publications':sum(c['global_publications'] for c in accepted),
              'peak_retained_workspace_bytes':max((c['scratch_bytes'] for c in calls),default=0),
              'device_to_device_copy_bytes':sum(c['device_copy_bytes'] for c in calls),
              'joint_gpu_seconds':sum(c['gpu_seconds'] for c in calls),'joint_control_seconds':sum(c['control_seconds'] for c in calls),
              'normal_intermediate_binding_download_bytes':r['intermediate_binding_download_bytes']}
        if 'stage_contexts' in r:
            stage=r['stage_contexts'];scopes=stage['scopes']
            lane['stage_contexts']={'scope_count':stage['scope_count'],'patches':stage['patches'],'audit':stage['audit'],
                'patch_words':sum(x['patch_words'] for x in scopes),'device_copy_bytes':sum(x['device_copy_bytes'] for x in scopes),
                'gpu_seconds':sum(x['gpu_seconds'] for x in scopes),'peak_resident_bytes':max((x['peak_resident_bytes'] for x in scopes),default=0),
                'contract_sha256':r['stage_cell_contract_sha256'],'loaded_artifact':r.get('loaded_stage_cell')}
        lanes.append(lane)
    result={'schema':'resident-joint-polyspine-results/v1','scope':'Actual GPU-owned complete saved Pass-3 schedule; not pristine egglog',
            'reference':{'context_phase_rows':sum(c['root_contexts']*c['private_phases'] for c in before.get('joint_polyspine',{}).get('calls',[]) if c['accepted']),'directory':str(reference),'inference_seconds':before['inference_seconds'],'setup_seconds':before['setup_seconds'],'gpu_seconds':before['timings_seconds']['device_gpu'],'source_sha256':before['source_sha256'],'worker_sha256':before['build']['worker_sha256']},
            'lanes':lanes,'definitions':{
                'service_histogram':'bit 0 terms binding, bit 1 helper binding, bit 2 changed comparison class; service is not net performance credit',
                'context_visits':'instances across joint invocations, not distinct RipSat library families',
                'fresh_rows':'real private hashcons allocations; some are retired by later congruence',
                'cross_stage_version_consumer_pairs':'distinct (row version, consumer stage) uses on successful exact bindings',
                'copy_bytes':'GPU-private transactional view copies; not CPU/GPU table transfer',
                'timing':'single candidate runs; saved unchanged reference retained with provenance'},
            'limits':['Native equality is independently checked on the real initial nested terms scope and regression cases.',
                      'The full backend retains its pre-existing complete-Pass native mismatch.',
                      'Ridge proposals never authorize effects or remove the exact fallback.',
                      'This version retains ordered GPU RHS materialization and a private row-store mirror.',
                      'Fresh hardware occupancy and Tiger extraction speed are not measured here.']}
    output.write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2));return result

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--reference',required=True,type=Path);p.add_argument('--run',action='append',required=True,type=Path);p.add_argument('--output',required=True,type=Path);a=p.parse_args();summarize(a.reference,a.run,a.output)
