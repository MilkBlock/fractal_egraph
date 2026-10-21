"""Read native time and remaining GPU join work without rerunning either."""
import json
from pathlib import Path


def read(path):
    path=Path(path);path=path/'report.json' if path.is_dir() else path
    return json.loads(path.read_text())


def summarize(gpu_path,native_path=None):
    r=read(gpu_path);g=r.get('gpu',r.get('runtime',{}))
    if not g.get('cells',{}).get('profile'):raise ValueError('GPU hotspot ranking requires a diagnostic counter run')
    fields=g['join_depth_fields'];live=fields.index('live_rows');gpu=[]
    for q in g['rules']:
        s=q['stats'];depth=s.get('depth_work') or []
        if not depth:continue
        gpu.append(dict(rule=q['rule'],ruleset=q['ruleset'],candidate_rows=sum(d[live] for d in depth),
            eligible_seeds=s['eligible_seeds'],emitted_bindings=s['emitted_bindings'],
            cell_queries=s.get('cell_queries'),depth=[dict(depth=i,**dict(zip(fields,d))) for i,d in enumerate(depth)]))
    total=sum(q['candidate_rows'] for q in gpu);gpu.sort(key=lambda q:q['candidate_rows'],reverse=True)
    for q in gpu:q['share_of_measured_rows']=q['candidate_rows']/total if total else None
    native=[]
    if native_path:
        report=read(native_path).get('run_report',{})
        for name,t in report.get('search_and_apply_time_per_rule',{}).items():
            seconds=t['secs']+t['nanos']/1e9 if isinstance(t,dict) else float(t)
            native.append(dict(rule=name,search_apply_seconds=seconds,logical_matches=report['num_matches_per_rule'].get(name)))
        native.sort(key=lambda q:q['search_apply_seconds'],reverse=True)
    return dict(schema='pass3-hotspots/v1',gpu=gpu,native=native,measured_gpu_rows=total,
        gpu_source=str(gpu_path),native_source=str(native_path) if native_path else None,
        scope='saved measurements; GPU ranking covers available depth counters only; native search+apply time is a different execution protocol; not Cell credit')


def run(a,resolve):
    r=summarize(resolve(a.input),resolve(a.native) if a.native else None)
    for q in r['gpu'][:10]:print(q['candidate_rows'],q['ruleset'],q['rule'].splitlines()[0])
    if a.output:
        p=resolve(a.output);p.parent.mkdir(parents=True,exist_ok=True)
        with p.open('x') as f:json.dump(r,f,indent=2);f.write('\n')
    if a.boundary_output:
        if not a.root:raise ValueError('--boundary-output requires an explicit --root constructor')
        if not r['gpu']:raise ValueError('no measured GPU queries')
        q=r['gpu'][0];p=resolve(a.boundary_output);p.parent.mkdir(parents=True,exist_ok=True)
        with p.open('x') as f:json.dump(dict(schema='cell-boundary-programs/v1',rules=[dict(ruleset=q['ruleset'],rule=q['rule'],root=a.root)]),f,indent=2);f.write('\n')
    return 0


def compare(before,after,profile_before=None,profile_after=None):
    def packet(path):
        p=Path(path);r=read(p);w=read(p/'workflow.json');g=r['gpu']
        if not (r.get('same_commit_verification') or {}).get('exact_rows_values_equivalence_visibility'):
            raise ValueError('boundary comparison needs exact same-commit graphs')
        return r,g,w
    a,ga,wa=packet(before);b,gb,wb=packet(after)
    for key in ['snapshot_sha256','binary_sha256','worker_sha256','policy']:
        if wa.get(key)!=wb.get(key):raise ValueError('boundary control differs: '+key)
    ia,ib=wa['inference_identity'],wb['inference_identity']
    if ia.get('boundary_programs') is not None or ib.get('boundary_programs') is None:
        raise ValueError('expected boundary programs off/on')
    for key in ['analysis','joint','all_rules']:
        if ia.get(key)!=ib.get(key):raise ValueError('common inference contract differs')
    for key in ['refutation','refutation_signatures','refutation_domains','staged_constraints','compact_binding_workspace','binding_connections','binding_ranges']:
        if ga['cells'].get(key)!=gb['cells'].get(key):raise ValueError('common GPU configuration differs')
    if ga.get('cell_query_reuse')!=gb.get('cell_query_reuse'):raise ValueError('common Cell query service differs')
    if ga.get('gpu_optimizations')!=gb.get('gpu_optimizations') or ga.get('query_sharing')!=gb.get('query_sharing'):
        raise ValueError('common GPU executor optimization differs')
    da,db=ga['saturation_diagnostics'],gb['saturation_diagnostics']
    if (da.get('joint') or {}).get('proofs_enabled')!=(db.get('joint') or {}).get('proofs_enabled'):
        raise ValueError('common saturation proofs differ')
    if (da.get('port_reuse') or {}).get('enabled')!=(db.get('port_reuse') or {}).get('enabled'):
        raise ValueError('common port reuse differs')
    ja,jb=da.get('joint') or {},db.get('joint') or {}
    for field in ['contract','closed_cost_producer_universe']:
        if ja.get(field)!=jb.get(field):raise ValueError('common joint contract differs')
    for field in ['macro_instances','uniform_cost_boundaries','dataflow_boundaries']:
        if ja.get('stats',{}).get(field)!=jb.get('stats',{}).get(field):raise ValueError('joint proof consumption changed')
    for field in ['saturations','union_proposals']:
        if (ja.get('comparison_closure') or {}).get(field)!=(jb.get('comparison_closure') or {}).get(field):raise ValueError('comparison proof changed')
    def common_programs(d):return sorted((p['ruleset'],p['rule'],p['kind'],p['seed'],json.dumps(p['accesses'],sort_keys=True))
        for p in d['programs'] if p['kind']!='boundary_query')
    if common_programs(da)!=common_programs(db):raise ValueError('existing Cell execution programs differ')
    if a['schedule']!=b['schedule'] or a['assisted_rulesets']!=b['assisted_rulesets']:raise ValueError('schedule differs')
    if any(g['cells']['profile'] or g['query_timing_profile'] or g['audit'] for g in [ga,gb]):raise ValueError('timing runs must be uninstrumented')
    qa={(q['ruleset'],q['rule']):q for q in ga['rules']};qb={(q['ruleset'],q['rule']):q for q in gb['rules']}
    if qa.keys()!=qb.keys():raise ValueError('rule universe differs')
    for k,q in qa.items():
        # emitted_bindings already includes Cell-provided bindings; the latter
        # is a subset, not an additional output stream.
        n=lambda x:x['stats']['emitted_bindings']
        if n(q)!=n(qb[k]):raise ValueError('changed complete binding count')
    seconds=lambda g:g['runtime']['run_seconds']
    result=dict(schema='boundary-reuse-comparison/v1',before=str(before),after=str(after),
        before_seconds=seconds(ga),after_seconds=seconds(gb),saved_seconds=seconds(ga)-seconds(gb),
        saved_fraction=1-seconds(gb)/seconds(ga),
        cpu_programs=[p for p in gb['saturation_diagnostics']['programs'] if p['kind']=='boundary_query'],
        scope='net source-derived boundary-context reuse with unchanged joint proofs and GPU backend; not automatic Analyze library or new sat-proof credit')
    if bool(profile_before)!=bool(profile_after):raise ValueError('both diagnostic runs required')
    if profile_before:
        _,pa,pwa=packet(profile_before);_,pb,pwb=packet(profile_after)
        for timed,profile,pw in [(wa,pa,pwa),(wb,pb,pwb)]:
            for key in ['snapshot_sha256','binary_sha256','worker_sha256','policy']:
                if timed.get(key)!=pw.get(key):raise ValueError('diagnostic binary/input differs')
            if timed['inference_identity'].get('boundary_programs')!=pw['inference_identity'].get('boundary_programs'):raise ValueError('diagnostic boundary plan differs')
            if not profile['cells']['profile']:raise ValueError('diagnostic counters missing')
        left={(q['ruleset'],q['rule']):q for q in pa['rules']};right={(q['ruleset'],q['rule']):q for q in pb['rules']}
        fields=pa['join_depth_fields'];live=fields.index('live_rows');changes=[]
        for k,q in left.items():
            r=right[k];x=q['stats'];y=r['stats'];dx=x['depth_work'];dy=y['depth_work']
            bx=sum(d[live] for d in dx);by=sum(d[live] for d in dy)
            if bx!=by or x['cell_queries']!=y['cell_queries']:
                def survival(ds,n):
                    return [sum(z[fields.index('deepest_seeds')] for z in ds[i:])/n for i in range(len(ds))] if n else None
                changes.append(dict(ruleset=k[0],rule=k[1],gpu_rows_before=bx,gpu_rows_after=by,
                    seeds_before=x['eligible_seeds'],seeds_after=y['eligible_seeds'],
                    cell_queries_before=x['cell_queries'],cell_queries_after=y['cell_queries'],
                    depths_before=[dict(zip(fields,d)) for d in dx],depths_after=[dict(zip(fields,d)) for d in dy],
                    conditional_survival_before=survival(dx,x['eligible_seeds']),conditional_survival_after=survival(dy,y['eligible_seeds']),
                    probability_scope='no conditional depth probability when query has moved entirely off GPU'))
        result['query_changes']=sorted(changes,key=lambda x:x['gpu_rows_before']-x['gpu_rows_after'],reverse=True)
        result['gpu_total_rows']=[pa['cell_work']['join_live_row_visits'],pb['cell_work']['join_live_row_visits']]
        common=[k for k in left if left[k]['stats']['eligible_seeds'] and right[k]['stats']['eligible_seeds'] and
                left[k]['stats']['depth_work'] and right[k]['stats']['depth_work']]
        weight=sum(left[k]['stats']['eligible_seeds'] for k in common)
        depth=max((len(left[k]['stats']['depth_work']) for k in common),default=0)
        def curve(queries):
            return [sum(left[k]['stats']['eligible_seeds']*sum(row[fields.index('deepest_seeds')] for row in queries[k]['stats']['depth_work'][d:])/
                        queries[k]['stats']['eligible_seeds'] for k in common)/weight for d in range(depth)] if weight else None
        result['common_rule_fixed_weights']=dict(rules=len(common),weight='before eligible seeds; queries moved off GPU excluded',
                                                  before=curve(left),after=curve(right),fixed_candidate_identity=False)
    return result
