"""Observe union-affected recipe instances; rank symbolic Bake requests.

This never labels a repeated coarse signature as a proved Cell transition.
"""
import argparse, collections, itertools, json, os, hashlib
from pathlib import Path
from eggfuse.engine.cell_runtime import load_pipeline_artifact,execute
from experiments.cell_runtime.compositions_bench import mode
from experiments.cell_runtime.refutation_bench import audit
ROOT=Path(__file__).resolve().parents[2]
def code(x):return json.dumps(x,sort_keys=True,separators=(',',':'))
def slots(x):
    if isinstance(x,dict):
        if 'slot' in x:yield x['slot']
        for v in x.values():yield from slots(v)
    elif isinstance(x,list):
        for v in x:yield from slots(v)
def signature(r,numeric=False,stateful=True):
    # Keep roles, aliases and stage; erase instance IDs and treat numerical
    # values as parameters in the primary grouping, not as execution semantics.
    # Only externally supplied Apply unions before this Run's first derived
    # union can seed a proposed Bake. Later repair conclusions are labels.
    # Preserve event order but erase run-specific event IDs. Multiple slots
    # touched by the SAME event remain grouped, never independent marginals.
    touch=[]
    for _,group in itertools.groupby(sorted(r['bake_input_touches'],key=lambda t:t['event']),key=lambda t:t['event']):
        touch.append([json.loads(s) for s in sorted({code({k:t[k] for k in ['slot','leaf','kind','peer_slots','peer_outputs']} | {'opponents':t['opponents'] if stateful else sorted(t['opponents'])}) for t in group})])
    return code([r['template'],r['active_ruleset'],r['before_input_partition'],r['before_output_partition'],touch,r['scalar_values'] if numeric else None,
        r.get('input_operator_before') if stateful else None,r.get('stage_path') if stateful else None])
def target(r):
    return code([r['after_input_partition'],r['after_output_partition'],r['new_input_output_aliases'],r['input_operator_refinements']])
def deformation(rows):
    classes=collections.Counter();flags=collections.Counter();reads=collections.Counter();work=collections.Counter();working=collections.Counter()
    for r in rows:
        added=any(d['added'] for d in r['port_deltas']);removed=any(d['removed'] for d in r['port_deltas'])
        alias=r['before_input_partition']!=r['after_input_partition'] or r['before_output_partition']!=r['after_output_partition'] or bool(r['new_input_output_aliases'])
        if added:flags['port_growth']+=1
        if removed:flags['visible_port_loss']+=1
        if alias:flags['slot_alias_change']+=1
        structural_read_change=any(d['read_added'] or d['read_removed'] for d in r['port_deltas'])
        function_change=any(d['changed_during_repair'] for d in r['read_deltas'])
        stale_before=any(not d['snapshot_current_before'] for d in r['read_deltas'])
        stale_after=any(not d['snapshot_current_after'] for d in r['read_deltas'])
        reads['constructor_read_changes']+=structural_read_change
        reads['function_port_changes']+=function_change
        reads['function_snapshot_stale_before']+=stale_before
        reads['function_snapshot_stale_after']+=stale_after
        reads['unchanged_current_input_projection']+=not (structural_read_change or function_change or alias or stale_after)
        state='stale_function_snapshot' if stale_after else 'changed_read_projection' if structural_read_change or function_change else 'slot_alias_change' if alias else 'unchanged_current_input_projection'
        work[state]+=r['owned_fact_repair_visits'];working[state]+=bool(r['owned_fact_repair_visits'])
        classes['new_constructor_rows' if added else 'lost_visible_rows' if removed else 'new_slot_equalities' if alias else 'observed_interface_unchanged']+=1
    return dict(classes=dict(classes),overlapping_flags=dict(flags),recipe_read_projection=dict(reads),associated_repair_visits=dict(work),instances_with_repair_work=dict(working),scope='canonical input constructors and actual query-table projections/current function ports; not complete Cell behavior or effect validity; events without union excluded; attributed repair is not saved work')
def recurrence(rows,numeric=False,stateful=True):
    prior={};known_frames=collections.defaultdict(set);result=collections.Counter()
    for _,batch in itertools.groupby(sorted(rows,key=lambda x:(x['epoch'],x['frame'])),key=lambda r:r['epoch']):
        batch=list(batch)
        # All labels from this publication boundary stay unavailable until
        # every row is evaluated. No within-Run target leakage.
        for r in batch:
            key=signature(r,numeric,stateful);value=target(r);seen=prior.get(key,set())
            if not seen:result['unseen_before_epoch']+=1
            elif len(seen)==1 and value in seen:
                result['same_observed_target']+=1
                if r['frame'] not in known_frames[key]:result['different_frame_same_target']+=1
            elif len(seen)>1:result['ambiguous_prior_targets']+=1
            else:result['new_target_for_known_key']+=1
        for r in batch:
            key=signature(r,numeric,stateful);prior.setdefault(key,set()).add(target(r));known_frames[key].add(r['frame'])
    return dict(result,signatures=len(prior),multiple_target_signatures=sum(len(v)>1 for v in prior.values()),
        observation_count=len(rows),scope='previous-epoch-only coarse-signature recurrence; no within-Run labels; NOT certified transition hits or predicted full egraphs')
def summarize(report,program):
    survey=report['union_impact'];rows=survey['rows'];rules=program['recipe']['rules']
    if survey.get('schema')!=4:raise ValueError('rerun survey: causes, exact port deltas and current function-read projections required')
    events=report['union_lineage']['events'];first_derived={}
    for i,e in enumerate(events):
        if not isinstance(e['cause'],dict) or 'Apply' not in e['cause']:
            first_derived.setdefault(e['epoch'],i)
    steps={s['epoch']:s for s in report['steps']}
    for r in rows:
        r['stage_path']=steps.get(r['epoch'],{}).get('path',[])
        r['iteration_state']=steps.get(r['epoch'],{}).get('iterations',[])
        r['direct_touches']=[t for t in r['touches'] if isinstance(events[t['event']]['cause'],dict) and 'Apply' in events[t['event']]['cause']]
        r['bake_input_touches']=[t for t in r['direct_touches'] if t['event']<first_derived.get(r['epoch'],len(events))]
    by_kind={}
    for kind in sorted({r['kind'] for r in rows}):
        rr=[r for r in rows if r['kind']==kind]
        by_kind[kind]={'frame_run_impacts':len(rr),'distinct_frames':len({r['frame'] for r in rr}),
            'input_touched':sum(bool(r['touches']) for r in rr),'born_this_run':sum(r['born_this_run'] for r in rr),
            'owned_repair_visits':sum(r['owned_fact_repair_visits'] for r in rr)}
    existing=[r for r in rows if r['kind']=='original_rule' and not r['born_this_run'] and r['completed']]
    ports=[r for r in existing if r['touches']]
    causal=[r for r in ports if r['bake_input_touches']]
    per_template=[]
    for tid in sorted({r['template'] for r in ports}):
        rr=[r for r in ports if r['template']==tid];used=set(slots(rules[tid]['effects']));leaf=[r for r in rr if any(t['leaf'] for t in r['touches'])]
        cr=[r for r in rr if r['bake_input_touches']]
        counts=collections.Counter((t['slot'],t['kind'],tuple(sorted(t['opponents']))) for r in cr for t in r['bake_input_touches'])
        per_template.append({'template':tid,'name':rules[tid]['metadata']['name'],'producer_ruleset':rules[tid]['metadata']['source_set'],
            'frame_run_impacts':len(rr),'distinct_frames':len({r['frame'] for r in rr}),
            'leaf_input_impacts':len(leaf),'rhs_input_impacts':sum(any(t['slot'] in used for t in r['touches']) for r in rr),
            'repair_visits':sum(r['owned_fact_repair_visits'] for r in rr),
            'initial_apply_input_impacts':len(cr),
            'initial_apply_leaf_impacts':sum(any(t['leaf'] for t in r['bake_input_touches']) for r in cr),
            'initial_apply_associated_repairs':sum(r['owned_fact_repair_visits'] for r in cr),
            'input_alias_changes':sum(r['before_input_partition']!=r['after_input_partition'] for r in rr),
            'internal_output_alias_changes':sum(r['before_output_partition']!=r['after_output_partition'] for r in rr),
            'new_input_output_aliases':sum(bool(r['new_input_output_aliases']) for r in rr),
            'input_operator_changes':sum(bool(r['input_operator_refinements']) for r in rr),
            'multi_step_frames':sum(r['continuation_depth']>1 for r in rr),
            'active_rulesets':dict(collections.Counter(r['active_ruleset'] for r in rr)),
            'top_changes':[{'slot':s,'kind':k,'opponents':list(ops),'count':n} for (s,k,ops),n in counts.most_common()],
            'causal_recurrence':recurrence(cr),'causal_recurrence_with_exact_scalars':recurrence(cr,True),
            'coarse_transition_key_recurrence':recurrence(cr,stateful=False)})
    per_template.sort(key=lambda x:(-x['initial_apply_associated_repairs'],-x['initial_apply_input_impacts'],x['template']))
    total=len(ports);freq=sorted(collections.Counter(r['template'] for r in ports).values(),reverse=True)
    cfreq=sorted(collections.Counter(r['template'] for r in causal).values(),reverse=True)
    patterns=collections.Counter((r['template'],t['slot'],t['kind'],tuple(sorted(t['opponents']))) for r in causal for t in r['bake_input_touches'])
    event_counts=collections.Counter()
    for e in events:event_counts[next(iter(e['cause'])) if isinstance(e['cause'],dict) else e['cause']]+=1
    return {'scope':survey['scope'],'timing_eligible':False,'effective_unions':survey['effective_unions'],
        'scanned_frames':survey['scanned_frames'],'unready_inputs':survey['unready_inputs'],
        'by_kind':by_kind,'existing_completed_original_frames':len(existing),'existing_original_input_impacts':len(ports),
        'existing_original_leaf_impacts':sum(any(t['leaf'] for t in r['touches']) for r in ports),
        'existing_original_distinct_input_frames':len({r['frame'] for r in ports}),
        'template_count':len(freq),'top_template_coverage':{str(k):sum(freq[:k])/total if total else None for k in [1,3,5,10]},
        'union_event_causes':dict(event_counts),
        'initial_apply_input_impacts':len(causal),'direct_apply_input_impacts':sum(bool(r['direct_touches']) for r in ports),
        'derived_only_input_impacts':sum(not r['direct_touches'] for r in ports),
        'initial_apply_leaf_impacts':sum(any(t['leaf'] for t in r['bake_input_touches']) for r in causal),
        'initial_apply_distinct_frames':len({r['frame'] for r in causal}),
        'initial_apply_associated_repairs':sum(r['owned_fact_repair_visits'] for r in causal),
        'initial_apply_template_count':len(cfreq),
        'initial_apply_top_template_coverage':{str(k):sum(cfreq[:k])/len(causal) if causal else None for k in [1,3,5,10]},
        'initial_apply_kinds_frame_runs':dict(collections.Counter(k for r in causal for k in {t['kind'] for t in r['bake_input_touches']})),
        'initial_apply_leaf_kinds_frame_runs':dict(collections.Counter(k for r in causal for k in {t['kind'] for t in r['bake_input_touches'] if t['leaf']})),
        'initial_apply_kind_touches':dict(collections.Counter(t['kind'] for r in causal for t in r['bake_input_touches'])),
        'initial_apply_pattern_count':len(patterns),
        'deformation_existing_original':deformation(existing),
        'deformation_initial_apply':deformation(causal),
        'initial_apply_top_pattern_coverage':{str(k):sum(n for _,n in patterns.most_common(k))/sum(patterns.values()) if patterns else None for k in [1,3,5,10,20]},
        'causal_recurrence':recurrence(causal),'causal_recurrence_with_exact_scalars':recurrence(causal,True),'coarse_transition_key_recurrence':recurrence(causal,stateful=False),'templates':per_template,
        'certified_cell_upgrades':0,'note':'recipe frame != RipSat Cell; observed target is only alias/operator-count projection; repair counts are association, not measured savings'}

def bake_requests(result,report,program):
    """Export every observed causal cohort; no top-K/budget or runtime claims."""
    requests=[];rules=program['recipe']['rules'];events=report['union_lineage']['events']
    for t in result['templates']:
        if not t['initial_apply_input_impacts']:continue
        rows=[r for r in report['union_impact']['rows'] if r['template']==t['template'] and r['kind']=='original_rule' and r['completed'] and not r['born_this_run'] and r['bake_input_touches']]
        cohorts=collections.defaultdict(list)
        for r in rows:cohorts[signature(r)].append(r)
        groups=[]
        for key,rr in sorted(cohorts.items()):
            targets=collections.Counter(target(r) for r in rr)
            groups.append({'source_projection_and_ordered_delta':json.loads(key),
                'observation_refs':[{'epoch':r['epoch'],'frame':r['frame'],'direct_events':sorted({t['event'] for t in r['bake_input_touches']})} for r in rr],
                'trigger_rules':sorted({events[touch['event']]['cause']['Apply']['rule'] for r in rr for touch in r['bake_input_touches']}),
                'observed_target_projections':[{'target':json.loads(k),'count':v} for k,v in sorted(targets.items())],
                'requires_more_state':len(targets)>1})
        requests.append({'source_rule':t['template'],'source_name':t['name'],'source_slots':rules[t['template']]['slots'],'source_query':rules[t['template']]['query'],
            'source_effects':rules[t['template']]['effects'],'observed_stages':sorted(t['active_rulesets']),
            'observed_changes':t['top_changes'],'support_frame_runs':t['initial_apply_input_impacts'],
            'associated_repairs':t['initial_apply_associated_repairs'],'cohorts':groups,
            'requires':['map runtime frame/query slots to certified RipSat boundary slots',
                'full correlated boundary wiring and existing closure identity',
                'symbolic scalar/alias guards and complete external constructor evidence',
                'original schedule visibility and late/subsumed input invalidation',
                'replay complete effects, function merge and derived congruence',
                'certify target closure before registering executable transitions'],
            'ready_for_runtime':False})
    return {'schema':1,'scope':'symbolic Bake work requests, not execution certificates; no selection cutoff',
        'program_sha256':program['sha256'],'original_control':program['recipe']['control'],'requests':requests}

def baked_library(compiled):
    pipeline=json.loads((compiled/'pipeline.json').read_text())
    path=Path(next(p['directory'] for p in pipeline['passes'] if p['name']=='analyze'))/'result/cell-recipes.json'
    data=json.loads(path.read_text())
    return {'path':str(path),'sha256':hashlib.sha256(path.read_bytes()).hexdigest(),
        'cell_count':data['cell_count'],'registered_cell_count':data['registered_cell_count'],
        'registered_with_program':data['registered_with_program'],'counts':data['counts'],
        'reasons':dict(collections.Counter(c.get('reason','') for c in data['cells'])),
        'live_substitution_authorized':sum(c.get('live_substitution_authorized',False) for c in data['cells'])}
def main():
    p=argparse.ArgumentParser();p.add_argument('--compiled',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--report-only',type=Path);p.add_argument('--audit',action='store_true');a=p.parse_args()
    a.output.mkdir(parents=True,exist_ok=False);path,program,provenance=load_pipeline_artifact(a.compiled.resolve())
    settings={'CELL_CPU_MERGE_PROGRAMS':'1','CELL_CPU_KNOWN_RESULTS':'off','CELL_CPU_UNION_IMPACT':'1','CELL_CPU_UNION_GRAFT':'0','CELL_CPU_UNION_TRACE':'0','CELL_CPU_MARKER_REFUTATION':'0','CELL_CPU_SIEVE':'0','CELL_CPU_CONGRUENCE':'0'}
    with mode(False):
        if a.report_only:
            saved=a.report_only.parent.parent/'cell-runtime-program.json'
            if json.loads(saved.read_text())!=program:raise ValueError('survey report belongs to a different program')
            report=json.loads(a.report_only.read_text())
        else:report=execute(program,a.output/'diagnostic',ROOT/'engine/cell-runtime/target/release/eggfuse-cell-runtime',environment=settings)
        result=summarize(report,program);result['program']=str(path);result['provenance']=provenance
        result['baked_library']=baked_library(a.compiled.resolve())
        report_path=(a.report_only or a.output/'diagnostic/runtime/report.json').resolve()
        result['diagnostic_report']=str(report_path)
        result['diagnostic_report_sha256']=hashlib.sha256(report_path.read_bytes()).hexdigest()
        result['effective_settings']={**settings,**{k:os.environ[k] for k in ['CELL_CPU_REFERENCE_VIEWS','CELL_CPU_MARKER_FIELDS','CELL_CPU_ABSORB','CELL_CPU_MULTISTAGE','CELL_CPU_COMPOSITIONS']}}
        result['total_repair_visits']=report['stats']['repaired_dependencies']
        result['attributed_repair_visits']=sum(v['owned_repair_visits'] for v in result['by_kind'].values())
        result['unattributed_repair_visits']=result['total_repair_visits']-result['attributed_repair_visits']
        if result['unattributed_repair_visits']<0:raise ValueError('repair work attributed more than once')
        # A reused report retains its own audit, never the current binary's
        # performance identity. Exact report digest binds the saved audit.
        if a.report_only:
            previous=report_path.parents[2]/'summary.json'
            if previous.exists():
                prior=json.loads(previous.read_text())
                if prior.get('diagnostic_report_sha256')==result['diagnostic_report_sha256'] and prior.get('verification'):
                    result['verification']=prior['verification'];result['verification_reused_from']=str(previous)
        if a.audit:
            old={k:os.environ.get(k) for k in settings}
            try:
                os.environ.update(settings)
                result['verification']=audit(path,program,a.output/'audit',ROOT/'engine/cell-runtime/target/release/examples/project_stages','CELL_CPU_UNION_IMPACT')
            finally:
                for k,v in old.items():
                    if v is None:os.environ.pop(k,None)
                    else:os.environ[k]=v
        (a.output/'summary.json').write_text(json.dumps(result,indent=2)+'\n')
        requests=bake_requests(result,report,program)
        (a.output/'bake-requests.json').write_text(json.dumps(requests,indent=2)+'\n')
        catalog={k:v for k,v in requests.items() if k not in ['original_control','requests']}
        catalog['requests']=[{**{k:v for k,v in t.items() if k not in ['source_query','source_slots','source_effects','cohorts','observed_changes','requires']},
            'cohort_count':len(t['cohorts']),
            'multiple_target_cohorts':sum(c['requires_more_state'] for c in t['cohorts']),
            'trigger_rules':sorted({i for c in t['cohorts'] for i in c['trigger_rules']})} for t in requests['requests']]
        (a.output/'bake-request-catalog.json').write_text(json.dumps(catalog,indent=2)+'\n')
        print(json.dumps({k:v for k,v in result.items() if k not in ['templates','provenance']},indent=2),flush=True)
        for t in result['templates'][:12]:print(t['template'],t['producer_ruleset'],t['frame_run_impacts'],t['repair_visits'],t['name'][:100],flush=True)
if __name__=='__main__':main()
