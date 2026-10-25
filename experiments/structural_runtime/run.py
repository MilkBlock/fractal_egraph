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

def main():
    p=argparse.ArgumentParser()
    for n in ('runtime-repo','build','source','output'):p.add_argument('--'+n,type=Path,required=True)
    p.add_argument('--lanes',default='native,exact,audit,gpu')
    p.add_argument('--extract',action='store_true')
    p.add_argument('--query-worker',type=Path,help='existing full GPU matcher for all unclaimed queries')
    p.add_argument('--reuse-native',type=Path,help='explicit unchanged native lane from an earlier run; never use after kernel/harness/schedule changes')
    p.add_argument('--reuse-baseline',type=Path,help='reuse unchanged full-GPU baseline; source and query worker must match')
    p.add_argument('--compare-layout',type=Path,help='read existing same-model single-layout lane and report exact paired matrix work')
    p.add_argument('--matrix-layout',choices=['single','shared'],default='single')
    p.add_argument('--coordinates',choices=['events','legacy','markers'],default='events')
    p.add_argument('--no-shared-conditions',action='store_true')
    p.add_argument('--model',type=Path,help='explicit exported plan; default BUILD/model.json')
    a=p.parse_args();a.output=a.output.resolve();a.output.mkdir(parents=True,exist_ok=True)
    repo=a.runtime_repo.resolve();build=a.build.resolve();binary=repo/'target/analyzer/release/structural-runtime'
    model=a.model.resolve() if a.model else build/'model.json';lib=json.loads(model.read_text())
    if lib['source_sha256']!=sha(a.source):raise ValueError('wrong source for exported plan')
    env={k:v for k,v in os.environ.items() if not k.startswith(('CELL_','EGGLOG_','RAYON_'))}
    if a.query_worker:env['STRUCTURAL_QUERY_WORKER']=str(a.query_worker.resolve())
    env['STRUCTURAL_MATRIX_LAYOUT']=a.matrix_layout
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
        if not r['exact_graph']['exact_rows_values_equivalence_visibility']:raise ValueError('unverified baseline')
        target=a.output/'baseline'
        if not target.exists():target.symlink_to(old,target_is_directory=True)
    for mode in a.lanes.split(','):
        if mode not in ('native','baseline','exact','audit','gpu'):raise ValueError('unknown lane')
        dest=a.output/mode;dest.mkdir(exist_ok=True);control=a.output/'native/graph.control.json'
        if mode=='native' and a.reuse_native:print(f'explicit historical native: {dest.resolve()}',flush=True);continue
        if mode=='baseline' and a.reuse_baseline:print(f'explicit historical GPU baseline: {dest.resolve()}',flush=True);continue
        env['STRUCTURAL_COORDINATE_AUDIT']='1' if mode=='audit' else '0'
        files=[binary,a.source,model]+([build/'worker',build/'kernel.metal'] if mode in ('gpu','audit') else [])
        if a.query_worker and mode!='native':files.append(a.query_worker)
        if mode!='native':files.append(control)
        identity={str(f.resolve()):sha(f) for f in files}
        identity['configuration']={k:v for k,v in env.items() if k.startswith('STRUCTURAL_')}
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
    result={'scope':'Complete real saved Pass 3 replay, selected always-run queries only; unclaimed queries use '+('the existing full GPU matcher' if a.query_worker else 'native CPU')+'. Not pristine.',
            'modes':{m:{'inference_seconds':r['inference_seconds'],'setup_seconds':r['setup_seconds'],'exact_graph':r['exact_graph']} for m,r in reports.items()},
            'model_provenance':lib['scope'],'source_sha256':sha(a.source)}
    result['historical_native']=str(a.reuse_native.resolve()) if a.reuse_native else None
    result['historical_baseline']=str(a.reuse_baseline.resolve()) if a.reuse_baseline else None
    result['configuration']={'matrix_layout':a.matrix_layout,'coordinates':a.coordinates,'shared_conditions':not a.no_shared_conditions}
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
    if a.compare_layout:
        paired=matrix_comparison(a.compare_layout,a.output,lib)
        paired['extraction']=result.get('extraction')
        (a.output/'matrix-comparison.json').write_text(json.dumps(paired,indent=2)+'\n')
        result['matrix_comparison']=str(a.output/'matrix-comparison.json')
    (a.output/'summary.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(result,indent=2))
if __name__=='__main__':main()
