"""Live Pass-3 adapter using the bench skill's graph and Tiger checkers.

Unchanged lane results are reused only under the complete input/tool identity.
This deliberately runs one sample per changed lane, never chooses a best time.
"""
import argparse,hashlib,json,os,subprocess,sys,time
from pathlib import Path
HERE=Path(__file__).resolve().parent
def sha(p):
    h=hashlib.sha256()
    with p.open('rb') as f:
        for b in iter(lambda:f.read(1024*1024),b''):h.update(b)
    return h.hexdigest()
def comparison(reports):
    def stats(mode):return reports[mode]['runtime']['saturation_diagnostics']['statistics']
    left,right=stats('exact'),stats('gpu');rules=[]
    for name,b in right['rules'].items():
        a=left['rules'][name]
        if a['delta_anchors']!=b['delta_anchors']:raise ValueError('different anchor population')
        for w in (a,b):
            if sum(w['deepest'])!=w['delta_anchors']:raise ValueError('invalid per-anchor depth histogram')
        n=a['delta_anchors']
        rules.append({'rule':name,'anchors':n,'cpu_fixed_reads':a['reads'],'gpu_ordered_reads':b['reads'],
            'fixed_depth_histogram':a['deepest'],'ordered_depth_histogram':b['deepest'],
            'complement_depth_histogram':b['complement_deepest'],
            'exact_cached_rejections':b.get('exact_cached_rejections',0),
            'confirmed_positive_bindings':b['positive_bindings'],'recovered_bindings':b['recovered_bindings'],
            'fixed_reach_probability':[sum(a['deepest'][d:])/n if n else None for d in range(1,5)],
            'ordered_reach_probability':[sum(b['deepest'][d:])/n if n else None for d in range(1,5)]})
    def total(k):return sum(r[k] for r in rules)
    return {'scope':'Exact CPU residual-probe depth, not native Free Join or the ordinary GPU Join; fixed/ordered use identical per-rule anchor populations.',
            'anchors':total('anchors'),'fixed_reads':total('cpu_fixed_reads'),'ordered_reads':total('gpu_ordered_reads'),
            'read_reduction':1-total('gpu_ordered_reads')/total('cpu_fixed_reads'),
            'confirmed_positive_bindings':total('confirmed_positive_bindings'),'recovered_bindings':total('recovered_bindings'),
            'fixed_rule_weights':'equal weight per rule with at least one anchor',
            'fixed_weight_reach_before':[sum(r['fixed_reach_probability'][d] for r in rules if r['anchors'])/sum(bool(r['anchors']) for r in rules) for d in range(4)],
            'fixed_weight_reach_after':[sum(r['ordered_reach_probability'][d] for r in rules if r['anchors'])/sum(bool(r['anchors']) for r in rules) for d in range(4)],
            'component_costs':{mode:{k:stats(mode)[k] for k in ('maintenance_seconds','query_seconds','gpu_seconds','device_roundtrip_seconds','gpu_batches','gpu_rows','uploaded_bytes','downloaded_bytes')} for mode in ('exact','gpu')},
            'condition_blocks':{mode:stats(mode).get('blocks') for mode in ('exact','gpu')},
            'coordinate_publication':reports['gpu']['runtime']['saturation_diagnostics'].get('coordinate_publication'),
            'marker_publication':reports['gpu']['runtime']['saturation_diagnostics'].get('marker_publication'),
            'routing_note':'Current-version exact condition blocks bypass prediction. Positive/negative counters describe this combined routing, not standalone GPU recall.',
            'rules':rules,'cell_proof_credit':0}
def matrix_comparison(before,after,lib):
    reports=[json.loads((d/'gpu/report.json').read_text()) for d in (before,after)]
    identities=[json.loads((d/'gpu/identity.json').read_text()) for d in (before,after)]
    for ident in identities:
        ident['configuration'].pop('STRUCTURAL_MATRIX_LAYOUT',None)
    if identities[0]!=identities[1]:raise ValueError('matrix comparison changed more than layout')
    if not all(r['exact_graph']['exact_rows_values_equivalence_visibility'] for r in reports):raise ValueError('unverified matrix graph')
    stats=[r['runtime']['saturation_diagnostics']['statistics'] for r in reports]
    if set(stats[0]['rules'])!=set(stats[1]['rules']):raise ValueError('different consumers')
    for name,a in stats[0]['rules'].items():
        b=stats[1]['rules'][name]
        for k in ('delta_anchors','positive_bindings','recovered_bindings','reads','deepest'):
            if a[k]!=b[k]:raise ValueError('layout changed decisions or work: '+name+' '+k)
    result={'scope':'Same exported workload, exact condition cache and completion; only requested matrix layout differs. Shape work is not Cell proof credit.','consumer_rules':len(stats[0]['rules']),
            'before':str(before.resolve()),'after':str(after.resolve()),'timing':{},'matrix':{},'rules':{n:{k:r[k] for k in ('delta_anchors','reads','positive_bindings','recovered_bindings','deepest')} for n,r in stats[1]['rules'].items()},'graph_equal':True}
    for label,r,t in zip(('single','shared'),reports,stats):
        m=t['matrix'];result['matrix'][label]=dict(m,extra_cells=m['computed_cells']-m['requested_cells'],
            useful_tile_fraction=m['requested_cells']/m['padded_cells'],coordinate_reuse=m['requests']/m['coordinate_rows'])
        result['timing'][label]={'inference_seconds':r['inference_seconds'],**{k:t[k] for k in ('maintenance_seconds','query_seconds','gpu_seconds','device_roundtrip_seconds','uploaded_bytes','downloaded_bytes','feature_requests','score_requests','constant_score_requests','ordering_roots','single_condition_roots')}}
    result['routing']= {k:sum(r[k] for r in stats[1]['rules'].values()) for k in ('delta_anchors','reads','positive_bindings','recovered_bindings')}
    result['ordinary_work_equal']={k:reports[0]['runtime']['runtime'][k]==reports[1]['runtime']['runtime'][k] for k in ('ruleset_runs','row_events','union_events','dispatches','uploaded_bytes','downloaded_bytes','cell_query_bindings')}
    catalog=[]
    for model in lib['models']:
        uses=[(p,step) for p in lib['plans'] for step in p['steps'] if step['model']==model['id']]
        projections=[]
        for p,step in uses:
            atom=p['shape']['atoms'][step['atom']];variables={};cols=[]
            for c in atom['columns']:
                if 'Slot' in c:
                    v=c['Slot'];variables.setdefault(v,len(variables));cols.append({'Slot':variables[v]})
                else:cols.append(c)
            projection={'table':atom['table'],'sorts':atom['sorts'],'columns':cols,'bound_slot':variables[step['slot']]}
            if projection not in projections:projections.append(projection)
        catalog.append({'id':model['id'],'sort':model['sort'],'predicate':model['channel'],
            'constant_prediction':model['bias'] if not any(w & 0x7fff for w in model['weights']) else None,
            'consumer_uses':len(uses),'projections':projections})
    result['column_catalog']=catalog;result['source_sha256']=lib['source_sha256'];result['training_scope']=lib['scope']
    result['interpretation']='Shared layout reduces padded cells; scores remain speculative, and native exact verification/completion stays on CPU. Shape utilization alone is not a speedup or Cell proof claim.'
    audit=after/'audit/report.json'
    if audit.exists():
        r=json.loads(audit.read_text());result['audit']={'native_query_checks':r['runtime']['runtime']['cell_query_rules'],'scope':r['runtime']['audit_scope'],'exact_graph':r['exact_graph']['exact_rows_values_equivalence_visibility']}
    return result

def execution_comparison(before,after,storage=False):
    dirs=(before,after);reports=[json.loads((d/'gpu/report.json').read_text()) for d in dirs]
    identities=[json.loads((d/'gpu/identity.json').read_text()) for d in dirs]
    if storage:
        builds=[json.loads((d/'summary.json').read_text())['build'] for d in dirs]
        if builds[0]['features']!=[] or builds[1]['features']!=['shared-rows']:raise ValueError('not an owned/shared build pair')
        for key in ('sources','runtime_repo','runtime_commit','runtime_dirty'):
            if builds[0][key]!=builds[1][key]:raise ValueError('row storage changed source/dependency: '+key)
        if builds[0]['runtime_dirty']:raise ValueError('row storage comparison requires clean runtime dependency')
        for ident,build in zip(identities,builds):
            if ident.pop(str(Path(build['binary']).resolve()))!=build['binary_sha256']:raise ValueError('wrong row storage binary')
    else:
        for i in identities:i['configuration'].pop('STRUCTURAL_CONDITION_EXECUTION',None)
    if identities[0]!=identities[1]:raise ValueError('execution comparison changed other inputs/tools')
    if not all(r['exact_graph']['exact_rows_values_equivalence_visibility'] for r in reports):raise ValueError('unverified execution graph')
    ds=[r['runtime']['saturation_diagnostics'] for r in reports];ss=[d['statistics'] for d in ds]
    if set(ss[0]['rules'])!=set(ss[1]['rules']):raise ValueError('consumer set changed')
    rules=[]
    for name,b in ss[0]['rules'].items():
        a=ss[1]['rules'][name];n=b['delta_anchors']
        if n!=a['delta_anchors'] or b['positive_bindings']+b['recovered_bindings']!=a['positive_bindings']+a['recovered_bindings']:
            raise ValueError('candidate/binding population changed: '+name)
        for x in (a,b):
            if sum(x['deepest'])!=n:raise ValueError('incorrect candidate histogram')
        rules.append(dict(rule=name,candidates=n,bindings=a['positive_bindings']+a['recovered_bindings'],
            before_depth=b['deepest'],after_payload_depth=a['deepest'],group_rejected=a['exact_group_rejected'],
            before_consumer_reads=b['reads'],after_payload_references=a['reads'],
            before_reach=[sum(b['deepest'][d:])/n if n else None for d in range(1,5)],
            after_payload_reach=[sum(a['deepest'][d:])/n if n else None for d in range(1,5)]))
    result={'scope':'Same admitted-query workload and original full native schedule; only condition execution mode differs. Event-readiness, group work and candidate payload work are charged separately. Not Cell proof/sat-skip credit.',
        'before':str(before.resolve()),'after':str(after.resolve()),'rules':rules,'modes':{}}
    for r,d,t in zip(reports,ds,ss):
        label=d['row_storage'] if storage else d['condition_execution']
        result['modes'][label]={'inference_seconds':r['inference_seconds'],'provider_seconds':t['maintenance_seconds']+t['query_seconds'],
            **{k:t[k] for k in ('maintenance_seconds','query_seconds','gpu_seconds','device_roundtrip_seconds','groups','readiness','blocks','index_dispatches','anchor_dispatches')},
            'handles':d['condition_handles'],'new_gpu_predictions':t['gpu_rows']}
    if storage:result['scope']='Same-source compile-time owned/shared row storage pair; same execution mode, query plans, original full schedule and exact results. Not a Cell proof or graph-compression claim.'
    result['speedup']=reports[0]['inference_seconds']/reports[1]['inference_seconds']
    result['group_work_note']='Truth checks/direct handle reads and conjunction work are charged globally. After-depth counts payload references only; depth-zero group rejection is not free work or a native Free Join depth.'
    active=[r for r in rules if r['candidates']]
    result['fixed_rule_weight_reach']={key:[sum(r[key][d] for r in active)/len(active) for d in range(4)] for key in ('before_reach','after_payload_reach')}
    result['ordinary_work_equal']={k:reports[0]['runtime']['runtime'][k]==reports[1]['runtime']['runtime'][k] for k in ('ruleset_runs','row_events','union_events','dispatches','uploaded_bytes','downloaded_bytes','cell_query_bindings')}
    result['ordinary_traffic']={k:{'before':reports[0]['runtime']['runtime'][k],'after':reports[1]['runtime']['runtime'][k]} for k in ('uploaded_bytes','downloaded_bytes')}
    audit=json.loads((after/'audit/report.json').read_text())
    result['audit']={'checks':audit['runtime']['runtime']['cell_query_rules'],'exact_graph':audit['exact_graph']['exact_rows_values_equivalence_visibility']}
    return result

def resident_comparison(before,after,scoped=False,exact_markers=False):
    dirs=[before.resolve(),after.resolve()]
    reports=[json.loads((d/'gpu/report.json').read_text()) for d in dirs]
    identities=[json.loads((d/'gpu/identity.json').read_text()) for d in dirs]
    layouts=[i['configuration'].pop('STRUCTURAL_MATRIX_LAYOUT',None) for i in identities]
    scopes=[i['configuration'].pop('STRUCTURAL_RESIDENT_SCOPE','sort') for i in identities]
    historical=False
    if exact_markers:
        builds=[json.loads((d/'summary.json').read_text())['build'] for d in dirs]
        for key in ('runtime_repo','runtime_commit','runtime_dirty','features','worker_sha256','kernel_sha256'):
            if builds[0][key]!=builds[1][key]:raise ValueError('exact marker comparison changed kernel/worker: '+key)
        if builds[0]['runtime_dirty']:raise ValueError('dirty runtime dependency')
        sources=[{Path(k).name:v for k,v in b['sources'].items()} for b in builds]
        for name in set(sources[0])|set(sources[1]):
            if name not in ('hook.rs','exact_markers.rs') and sources[0].get(name)!=sources[1].get(name):raise ValueError('unexpected adapter change: '+name)
        flags=[i['configuration'].pop('STRUCTURAL_EXACT_MARKERS','0') for i in identities]
        if flags!=['0','1'] or scopes[0]!=scopes[1]:raise ValueError('not an exact marker off/on pair')
        historical=builds[0]['binary_sha256']!=builds[1]['binary_sha256']
        for ident,b in zip(identities,builds):
            binary=Path(b['binary']);assert ident.pop(str(binary))==b['binary_sha256']
            for name in ['worker','kernel.metal']:ident['shared-'+name]=ident.pop(str(binary.parent/name))
    expected=['resident','resident'] if scoped or exact_markers else ['shared','resident']
    if layouts!=expected or identities[0]!=identities[1] or (scoped and scopes!=['sort','consumers']):raise ValueError('resident pair changed more than the requested backend/scope')
    for r in reports:
        d=r['runtime']['saturation_diagnostics']
        if not r['exact_graph']['exact_rows_values_equivalence_visibility'] or d.get('predictor_loaded') is not True or d.get('predictor_executed') is not True:
            raise ValueError('resident pair requires verified graphs and actual ridge GEMM on both sides')
    def bindings(r):return {(q['ruleset'],q['rule']):q['stats']['emitted_bindings'] for q in r['runtime']['rules']}
    if bindings(reports[0])!=bindings(reports[1]):raise ValueError('complete original binding counts differ')
    work=[]
    before_rules=reports[0]['runtime']['saturation_diagnostics']['statistics']['rules']
    after_rules=reports[1]['runtime']['saturation_diagnostics']['statistics']['rules']
    if set(before_rules)!=set(after_rules):raise ValueError('different ridge consumers')
    for name,left in before_rules.items():
        right=after_rules[name];n=left['delta_anchors']
        if right['delta_anchors']!=n:raise ValueError('different ridge anchor populations')
        for row in (left,right):
            if sum(row['deepest'])!=n:raise ValueError('invalid exact-check depth histogram')
        work.append({'rule':name,'anchors':n,'reads_before':left['reads'],'reads_after':right['reads'],
            'marker_candidates':right.get('marker_candidates',0),'marker_rejections':right.get('marker_rejections',0),
            'marker_bindings':right.get('marker_bindings',0),'marker_payload_references':right.get('marker_payload_references',0),
            'depth_before':left['deepest'],'depth_after':right['deepest'],
            'reach_before':[sum(left['deepest'][d:])/n if n else None for d in range(1,5)],
            'reach_after':[sum(right['deepest'][d:])/n if n else None for d in range(1,5)]})
    rows=[]
    for path,r in zip(dirs,reports):
        d=r['runtime']['saturation_diagnostics'];t=d['statistics']
        rows.append({'directory':str(path),'backend':d['predictor_backend'],'joint_services_enabled':r.get('joint_services_enabled',False),
            'predictor_executed':d.get('predictor_executed'),
            'resident_scope':d.get('resident_scope'),'consumer_scopes':d.get('consumer_scopes'),
            'exact_markers':d.get('exact_markers'),'condition_blocks':t['blocks'],
            'inference_seconds':r['inference_seconds'],'setup_seconds':r['setup_seconds'],
            'matrix':t['matrix'],'resident':t['resident'],
            **{k:t[k] for k in ('gpu_batches','gpu_rows','gpu_seconds','device_roundtrip_seconds','maintenance_seconds','query_seconds','uploaded_bytes','downloaded_bytes')},
            'ridge_bindings':sum(x['positive_bindings']+x['recovered_bindings'] for x in t['rules'].values()),
            'combined_services':d.get('combined_services'),
            'original_graph':r['exact_graph']})
    return {'scope':'Same full Pass 3, model, native kernel, exact completion and joint-service configuration. Only '+('event-published shared exact condition markers' if exact_markers else ('all same-sort heads versus event-published consumer heads' if scoped else 'packet coordinates versus resident factored ridge responses'))+' changes. Not new Cell proof credit.',
            'historical_control':historical,
            'marker_totals':({'candidates':sum(w['marker_candidates'] for w in work),
                'rejections':sum(w['marker_rejections'] for w in work),'bindings':sum(w['marker_bindings'] for w in work),
                'payload_references':sum(w['marker_payload_references'] for w in work),
                'residual_accesses_before':sum(w['reads_before'] for w in work),'residual_accesses_after':sum(w['reads_after'] for w in work),
                'new_answer_reads_before':rows[0]['condition_blocks']['reads'],'new_answer_reads_after':rows[1]['condition_blocks']['reads']} if exact_markers else None),
            'marker_depth_note':('Depth zero means marker routing without the old per-root residual loop, not zero work. Include leaf truth reads, conjunction publications, payload loads, tuple fanout and event maintenance from exact_markers; this is not native Free Join or ordinary GPU search.' if exact_markers else None),
            'variants':rows,'speedup':rows[0]['inference_seconds']/rows[1]['inference_seconds'],
            'same_complete_binding_counts':True,'complete_bindings':sum(bindings(reports[0]).values()),
            'exact_cpu_check_work':work,'depth_scope':'CPU factorized residual checks, not ordinary GPU/Free Join depth; equal original anchor population',
            'fixed_rule_weight_reach':{k:[sum(w[k][d] for w in work if w['anchors'])/sum(bool(w['anchors']) for w in work) for d in range(4)] for k in ('reach_before','reach_after')},
            'hardware_note':'Apple Metal simdgroup matrix arithmetic; no NVIDIA Tensor Core utilization measurement.',
            'prefetch_note':'Computed but unrequested heads are speculative overhead until actually reused. Current/discarded unused heads are reported explicitly.'}

def main():
    p=argparse.ArgumentParser()
    for n in ('runtime-repo','build','source','output'):p.add_argument('--'+n,type=Path,required=True)
    p.add_argument('--lanes',default='native,exact,audit,gpu')
    p.add_argument('--extract',action='store_true')
    p.add_argument('--exact-markers',action='store_true',help='share event-published exact residual conjunctions and whole witness payloads')
    p.add_argument('--joint-sat',action='store_true',help='keep the existing joint closure services active alongside ridge')
    p.add_argument('--boundary-programs',type=Path,help='existing loop/shared-spine provider; requires --joint-sat')
    p.add_argument('--query-worker',type=Path,help='existing full GPU matcher for all unclaimed queries')
    p.add_argument('--reuse-native',type=Path,help='explicit unchanged native lane from an earlier run; never use after kernel/harness/schedule changes')
    p.add_argument('--reuse-baseline',type=Path,help='reuse unchanged full-GPU baseline; source and query worker must match')
    p.add_argument('--compare-resident',type=Path,help='compare to same-binary/model/shared-predictor run; both must actually execute ridge')
    p.add_argument('--compare-consumer-scope',type=Path,help='compare to same-binary resident all-sort-heads control')
    p.add_argument('--compare-exact-markers',type=Path,help='reuse a checked marker-off control with the same kernel, model, workers and all other adapter sources')
    p.add_argument('--compare-storage',type=Path,help='compare current shared-row build to a verified same-source owned-row run')
    p.add_argument('--compare-execution',type=Path,help='compare verified condition execution modes without retiming')
    p.add_argument('--compare-layout',type=Path,help='read existing same-model single-layout lane and report exact paired matrix work')
    p.add_argument('--condition-execution',choices=['roots','groups','ready'],default='roots')
    p.add_argument('--matrix-layout',choices=['single','shared','resident'],default='shared')
    p.add_argument('--resident-scope',choices=['consumers','sort'],default='sort',help='consumer scoping is experimental; full-pass net benefit is not established')
    p.add_argument('--coordinates',choices=['events','legacy','markers'],default='events')
    p.add_argument('--no-shared-conditions',action='store_true')
    p.add_argument('--model',type=Path,help='explicit exported plan; default BUILD/model.json')
    p.add_argument('--allow-idle-predictor',action='store_true',help='explicit exact/idle control; otherwise gpu/audit lanes must actually invoke ridge GEMM')
    a=p.parse_args()
    if a.matrix_layout=='resident' and a.condition_execution!='roots':p.error('resident ridge requires roots; groups/ready bypass the predictor')
    if a.condition_execution!='roots' and not a.allow_idle_predictor:p.error('groups/ready are exact controls; pass --allow-idle-predictor explicitly')
    if a.boundary_programs and not a.joint_sat:p.error('boundary programs require --joint-sat')
    a.output=a.output.resolve();a.output.mkdir(parents=True,exist_ok=True)
    repo=a.runtime_repo.resolve();build=a.build.resolve();binary=Path(json.loads((build/'build.json').read_text())['binary'])
    model=a.model.resolve() if a.model else build/'model.json';lib=json.loads(model.read_text())
    if lib['source_sha256']!=sha(a.source):raise ValueError('wrong source for exported plan')
    env={k:v for k,v in os.environ.items() if not k.startswith(('CELL_','EGGLOG_','RAYON_','STRUCTURAL_'))}
    if a.query_worker:env['STRUCTURAL_QUERY_WORKER']=str(a.query_worker.resolve())
    env['STRUCTURAL_EXACT_MARKERS']='1' if a.exact_markers else '0'
    env['STRUCTURAL_JOINT_SAT']='1' if a.joint_sat else '0'
    if a.boundary_programs:env['CELL_BOUNDARY_PROGRAMS']=str(a.boundary_programs.resolve())
    env['STRUCTURAL_REQUIRE_PREDICTOR']='0' if a.allow_idle_predictor else '1'
    env['STRUCTURAL_CONDITION_EXECUTION']=a.condition_execution
    env['STRUCTURAL_MATRIX_LAYOUT']=a.matrix_layout
    env['STRUCTURAL_RESIDENT_SCOPE']=a.resident_scope
    env['STRUCTURAL_COORDINATES']=a.coordinates
    env['STRUCTURAL_SHARED_CONDITIONS']='0' if a.no_shared_conditions else '1'
    if a.reuse_native:
        old=a.reuse_native.resolve()/'native';identity=json.loads((old/'identity.json').read_text())
        if identity.get(str(a.source.resolve()))!=sha(a.source):raise ValueError('historical native input differs')
        r=json.loads((old/'report.json').read_text())
        if r['mode']!='native' or not (old/'graph.control.json').exists():raise ValueError('not a successful native control')
        target=a.output/'native'
        if not target.exists():target.symlink_to(old,target_is_directory=True)
    if a.reuse_baseline:
        old=a.reuse_baseline.resolve()/'baseline';identity=json.loads((old/'identity.json').read_text())
        if identity.get(str(a.source.resolve()))!=sha(a.source):raise ValueError('historical GPU input differs')
        if not a.query_worker or identity.get(str(a.query_worker.resolve()))!=sha(a.query_worker):raise ValueError('historical GPU worker differs')
        r=json.loads((old/'report.json').read_text())
        if r.get('joint_services_enabled',False)!=a.joint_sat:raise ValueError('historical baseline uses different joint services')
        if a.boundary_programs and identity.get(str(a.boundary_programs.resolve()))!=sha(a.boundary_programs):raise ValueError('historical boundary program differs')
        if not r['exact_graph']['exact_rows_values_equivalence_visibility']:raise ValueError('unverified baseline')
        target=a.output/'baseline'
        if not target.exists():target.symlink_to(old,target_is_directory=True)
    for mode in a.lanes.split(','):
        if mode not in ('native','baseline','exact','audit','gpu'):raise ValueError('unknown lane')
        dest=a.output/mode;dest.mkdir(exist_ok=True);control=a.output/'native/graph.control.json'
        if mode=='native' and a.reuse_native:print(f'explicit historical native: {dest.resolve()}',flush=True);continue
        if mode=='baseline' and a.reuse_baseline:print(f'explicit historical GPU baseline: {dest.resolve()}',flush=True);continue
        env['STRUCTURAL_COORDINATE_AUDIT']='1' if mode=='audit' else '0'
        files=[binary,a.source,model]+([build/'worker',build/'kernel.metal'] if mode in ('gpu','audit') and a.condition_execution=='roots' else [])
        if a.query_worker and mode!='native':files.append(a.query_worker)
        if mode!='native':files.append(control)
        if a.joint_sat and a.boundary_programs and mode!='native':files.append(a.boundary_programs.resolve())
        identity={str(f.resolve()):sha(f) for f in files}
        identity['configuration']={k:v for k,v in env.items() if k.startswith(('STRUCTURAL_','CELL_BOUNDARY_PROGRAMS'))}
        prior=dest/'identity.json';report=dest/'report.json'
        if prior.exists() and report.exists() and json.loads(prior.read_text())==identity:
            print(f'reusing {mode}: {report}',flush=True);continue
        if report.exists():raise ValueError(f'{dest} belongs to another binary/input; use a new output')
        command=[str(binary),str(a.source.resolve()),str(model),str(build/'worker'),str(build/'kernel.metal'),str(dest),mode,str(control) if mode!='native' else '-']
        with (dest/'stderr.log').open('w') as log:
            print('running',mode,flush=True);started=time.perf_counter();r=subprocess.run(command,env=env,stdout=log,stderr=log)
        (dest/'execution.json').write_text(json.dumps({'command':command,'returncode':r.returncode,'wall_seconds_including_setup_exports_check':time.perf_counter()-started},indent=2)+'\n')
        r.check_returncode();prior.write_text(json.dumps(identity,indent=2)+'\n')
    reports={m:json.loads((a.output/m/'report.json').read_text()) for m in ('native','baseline','exact','audit','gpu') if (a.output/m/'report.json').exists()}
    result={'scope':'Complete real saved Pass 3 replay, '+('joint/loop services and ' if a.joint_sat else '')+'selected always-run ridge queries; unclaimed queries use '+('the existing full GPU matcher' if a.query_worker else 'native CPU')+'. Not pristine.',
            'modes':{m:{'inference_seconds':r['inference_seconds'],'setup_seconds':r['setup_seconds'],'exact_graph':r['exact_graph']} for m,r in reports.items()},
            'model_provenance':lib['scope'],'source_sha256':sha(a.source)}
    result['predictor']={m:{k:r['runtime'].get('saturation_diagnostics',{}).get(k) for k in ('predictor_loaded','predictor_backend','predictor_executed')} for m,r in reports.items() if m in ('gpu','audit','exact')}
    result['predictor_required']=not a.allow_idle_predictor
    result['joint_services_enabled']=a.joint_sat
    result['boundary_programs']=str(a.boundary_programs.resolve()) if a.boundary_programs else None
    result['build']=json.loads((build/'build.json').read_text())
    result['historical_native']=str(a.reuse_native.resolve()) if a.reuse_native else None
    result['historical_baseline']=str(a.reuse_baseline.resolve()) if a.reuse_baseline else None
    result['configuration']={'condition_execution':a.condition_execution,'matrix_layout':a.matrix_layout,'resident_scope':a.resident_scope,'coordinates':a.coordinates,'shared_conditions':not a.no_shared_conditions,'exact_markers':a.exact_markers}
    for m in ('baseline','exact','gpu'):
        if m in reports and 'native' in reports and reports[m]['exact_graph']['exact_rows_values_equivalence_visibility']:
            result[m+'_speedup_vs_native']=reports['native']['inference_seconds']/reports[m]['inference_seconds']
    if 'baseline' in reports and 'gpu' in reports:
        result['predictor_speedup_vs_gpu_baseline']=reports['baseline']['inference_seconds']/reports['gpu']['inference_seconds']
    if 'exact' in reports and 'gpu' in reports:result['completion']=comparison(reports)
    if a.extract:
        # Existing production Tiger workflow; its 'native' label here is the
        # same-commit original-query lane, NOT a pristine/fused-native control.
        ex=a.output/'extraction-check';(ex/'graphs').mkdir(parents=True,exist_ok=True)
        for dst,src in [('native','native'),('cpu','exact' if 'exact' in reports else 'native'),('gpu','gpu')]:
            target=ex/'graphs'/f'{dst}.json'
            if not target.exists():target.symlink_to(a.output/src/'graph.json')
        if not (ex/'extraction/quality.json').exists():
            subprocess.run([sys.executable,str(repo/'experiments/raytrace_pass3_gpu/extract.py'),str(ex)],check=True,env=env)
        result['extraction_cpu_control']='exact' if 'exact' in reports else 'native'
        result['extraction']=json.loads((ex/'extraction/quality.json').read_text())
    (a.output/'summary.json').write_text(json.dumps(result,indent=2)+'\n')
    if a.compare_resident:
        paired=resident_comparison(a.compare_resident,a.output);paired['extraction']=result.get('extraction')
        (a.output/'resident-comparison.json').write_text(json.dumps(paired,indent=2)+'\n')
        result['resident_comparison']=str(a.output/'resident-comparison.json')
    if a.compare_consumer_scope:
        paired=resident_comparison(a.compare_consumer_scope,a.output,scoped=True);paired['extraction']=result.get('extraction')
        (a.output/'consumer-comparison.json').write_text(json.dumps(paired,indent=2)+'\n')
        result['consumer_comparison']=str(a.output/'consumer-comparison.json')
    if a.compare_exact_markers:
        paired=resident_comparison(a.compare_exact_markers,a.output,exact_markers=True);paired['extraction']=result.get('extraction')
        (a.output/'exact-marker-comparison.json').write_text(json.dumps(paired,indent=2)+'\n')
        result['exact_marker_comparison']=str(a.output/'exact-marker-comparison.json')
    if a.compare_storage:
        paired=execution_comparison(a.compare_storage,a.output,storage=True);paired['extraction']=result.get('extraction')
        (a.output/'storage-comparison.json').write_text(json.dumps(paired,indent=2)+'\n')
        result['storage_comparison']=str(a.output/'storage-comparison.json')
    if a.compare_execution:
        paired=execution_comparison(a.compare_execution,a.output);paired['extraction']=result.get('extraction')
        (a.output/'execution-comparison.json').write_text(json.dumps(paired,indent=2)+'\n')
        result['execution_comparison']=str(a.output/'execution-comparison.json')
    if a.compare_layout:
        paired=matrix_comparison(a.compare_layout,a.output,lib)
        paired['extraction']=result.get('extraction')
        (a.output/'matrix-comparison.json').write_text(json.dumps(paired,indent=2)+'\n')
        result['matrix_comparison']=str(a.output/'matrix-comparison.json')
    (a.output/'summary.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(result,indent=2))
if __name__=='__main__':main()
