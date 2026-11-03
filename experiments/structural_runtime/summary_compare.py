"""Read-only, paired semantic-marker feature report; never reruns a bench."""
import argparse
import json
from pathlib import Path


def quality(c):
    tp,fp,tn,fn=(c[k] for k in ('tp','fp','tn','fn_'))
    return dict(c,requests=tp+fp+tn+fn,precision=tp/(tp+fp) if tp+fp else None,
        recall=tp/(tp+fn) if tp+fn else None,false_positive_rate=fp/(tn+fp) if tn+fp else None)


def compare(before,after):
    b=json.loads((before/'gpu/report.json').read_text());a=json.loads((after/'gpu/report.json').read_text())
    audit=json.loads((after/'audit/report.json').read_text())
    for r in (b,a,audit):
        if not r['exact_graph']['exact_rows_values_equivalence_visibility']:raise ValueError('unverified graph')
    bi=json.loads((before/'gpu/identity.json').read_text());ai=json.loads((after/'gpu/identity.json').read_text())
    for p,h in bi.items():
        if p=='configuration':
            if h!=ai[p]:raise ValueError('runtime configuration differs')
        elif p in ai and h!=ai[p]:raise ValueError('common input/tool changed: '+p)
    bs=b['runtime']['saturation_diagnostics']['statistics'];ass=a['runtime']['saturation_diagnostics']['statistics']
    rules=[]
    for n,old in bs['rules'].items():
        new=ass['rules'][n]
        if old['delta_anchors']!=new['delta_anchors']:raise ValueError('different candidate population')
        old_count=old['positive_bindings']+old['recovered_bindings'];new_count=new['positive_bindings']+new['recovered_bindings']
        if old_count!=new_count:raise ValueError('different complete binding output')
        count=old['delta_anchors']
        rules.append(dict(rule=n,anchors=count,bindings=new_count,
            before={k:old[k] for k in ('reads','deepest','entered','rejected','positive_bindings','recovered_bindings')},
            after={k:new[k] for k in ('reads','deepest','entered','rejected','positive_bindings','recovered_bindings')},
            reach_before=[sum(old['deepest'][d:])/count if count else None for d in range(1,5)],
            reach_after=[sum(new['deepest'][d:])/count if count else None for d in range(1,5)]))
    def totals(s):return {k:sum(r[k] for r in s['rules'].values()) for k in ('delta_anchors','positive_bindings','recovered_bindings','false_positive_roots','reads','exact_cached_rejections')}
    stats=audit['runtime']['saturation_diagnostics']['statistics']
    qa={k:quality(v) for k,v in stats['prediction_audit'].items()}
    for group in ('all','with-summary','without-summary'):
        x,y=qa['candidate/'+group],qa['reference/'+group]
        if (x['requests'],x['tp']+x['fn_'])!=(y['requests'],y['tp']+y['fn_']):raise ValueError('not paired prediction populations')
    def timing(r,s):return dict(inference_seconds=r['inference_seconds'],maintenance_seconds=s['maintenance_seconds'],query_seconds=s['query_seconds'],provider_seconds=s['maintenance_seconds']+s['query_seconds'],gpu_rows=s['gpu_rows'],gpu_seconds=s['gpu_seconds'],roundtrip_seconds=s['device_roundtrip_seconds'])
    result=dict(scope='Actual complete saved raytrace Pass 3. Predictor provider covers seven unchanged always-run rules; original RHS and CPU exact complement retained.',
        before=str(before),after=str(after),timing=dict(before=timing(b,bs),after=timing(a,ass)),
        routing=dict(before=totals(bs),after=totals(ass)),paired_condition_quality=qa,
        audit_note='Only diagnostic Run: both models score identical unknown model/key blocks at the same pre-Run native state. Exact cached answers excluded. Audit timing is not benchmark timing.',
        training_note='Same-program final-snapshot training with fixed held-out-key split; live quality includes training keys. Not unseen-program generalization.',
        source_marker_publication=a['runtime']['saturation_diagnostics']['summary_publication'],rules=rules,
        exact_condition_discharges=0,cell_closure_proof_credit=0)
    valid=[r for r in rules if r['anchors']]
    result['fixed_rule_weight_reach']={k:[sum(r['reach_'+k][d] for r in valid)/len(valid) for d in range(4)] for k in ('before','after')}
    fields=('ruleset_runs','row_events','union_events','dispatches','uploaded_bytes','downloaded_bytes','cell_query_bindings')
    result['ordinary_runtime_work']={k:{'before':b['runtime']['runtime'][k],'after':a['runtime']['runtime'][k]} for k in fields}
    result['unchanged_phase_timings']={k:{'before':b['runtime']['runtime'][k],'after':a['runtime']['runtime'][k]} for k in ('native_rhs_seconds','native_rebuild_seconds','roundtrip_seconds','device_seconds')}
    result['timing_interpretation']='Single serial samples, not averages. The large total-time difference is not attributable to semantic markers: ordinary runtime work/traffic are unchanged and exact condition reads did not decrease. Unmodified RHS/device phases also changed. No net acceleration credit claimed.'
    result['extraction']=json.loads((after/'summary.json').read_text()).get('extraction')
    return result

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--before',type=Path,required=True);p.add_argument('--after',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();r=compare(a.before,a.after);a.output.write_text(json.dumps(r,indent=2)+'\n')
    print(json.dumps({k:r[k] for k in ('timing','routing','paired_condition_quality','source_marker_publication')},indent=2))
