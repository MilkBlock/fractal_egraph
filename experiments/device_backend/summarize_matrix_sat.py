"""Save a bounded, reproducible matrix-Saturate comparison, without rerunning.

The checked endpoints are actual device tables. Native checks, when supplied,
are separately scoped; a device-to-device pass is not native equivalence.
"""
import argparse,hashlib,json
from pathlib import Path
from compare_runs import compare

def summarize(before,after,output,tests=None,native=None):
    comparison=compare(before,after,device_states=True,semantic_device=True)
    reports=[json.loads((p/'report.json').read_text()) for p in [before,after]]
    fields=['set','rows','matched','updated','epoch','unions']
    histories=[[tuple(s[k] for k in fields) for s in r['steps']] for r in reports]
    if histories[0]!=histories[1]:raise ValueError('logical Run history differs')
    calls=reports[1]['matrix_saturate']['calls'];selected={c['set'] for c in calls}
    def identity(folder,r):
        b=r['build']
        return {'directory':str(folder),'source_sha256':r['source_sha256'],
                'report_sha256':hashlib.sha256((folder/'report.json').read_bytes()).hexdigest(),
                'program_sha256':r['program_sha256'],'frontend_sha256':b['frontend_sha256'],
                'worker_sha256':b['worker_sha256'],'git_head':b['git_head'],
                'gpu_sources':{Path(k).name:v for k,v in b['sources'].items() if Path(k).parent.name=='device_backend'},
                'inference_seconds':r['inference_seconds'],'clean_timing':r['clean_timing'],
                'setup_seconds':r['setup_seconds'],'device_seconds':r['timings_seconds']['device_gpu'],
                'initial_device_allocation_bytes':r['gpu_allocated_bytes_at_start']}
    def phase(r):
        ds=[x for x in r['device_command_times'] if x['set'] in selected]
        return dict(commands=len(ds),**{k:sum(x.get(k,0) or 0 for x in ds) for k in
                    ['gpu_seconds','prepare_seconds','match_seconds','commit_seconds']})
    result={'scope':'Actual GPU-owned backend, full saved schedule; single measurements, no pristine speedup claim',
            'runs':[identity(p,r) for p,r in zip([before,after],reports)],
            'same_worker':reports[0]['build']['worker_sha256']==reports[1]['build']['worker_sha256'],
            'final_check':comparison['final_check'],'logical_history_equal':True,'logical_runs':len(histories[1]),
            'admitted':[{k:p[k] for k in ['name','predicates','rules_sha256']}|{'rules':len(p['recipes'])}
                        for p in reports[1]['matrix_saturate']['programs']],
            'selected_phases':[phase(r) for r in reports],
            'matrix_totals':{'calls':len(calls),'logical_runs':sum(c['logical_runs'] for c in calls),
                             'published_rows':sum(c['published_rows'] for c in calls),
                             'scratch_peak_bytes':max((c['scratch_bytes'] for c in calls),default=0),
                             'control_seconds':sum(c.get('control_seconds',0) for c in calls)},
            'calls':[{k:v for k,v in c.items() if k!='history'} for c in calls],
            'limitations':['Only certified positive unary predicate closures over a fixed structural carrier.',
                           'No automatic Analyze-library Cell specialization import in this path.',
                           'No new hardware occupancy measurement for this matrix macro.',
                           'Physical row and identity allocation order may differ.',
                           'The full device backend has a pre-existing native final-graph mismatch.']}
    if tests:
        t=json.loads((tests/'results.json').read_text());result['regression']={'path':str(tests),'passed':len(t),'cases':list(t)}
    if native:
        n=json.loads((native/'comparison.json').read_text())
        result['native_scope']={k:n[k] for k in ['scope','source_sha256','native_final_checks','logical_history_equal']}
        result['native_scope']['calls']=[{k:v for k,v in c.items() if k!='history'} for c in n['matrix_calls']]
    output.write_text(json.dumps(result,indent=2)+'\n');print(json.dumps({k:v for k,v in result.items() if k not in ['runs','calls','native_scope','regression']},indent=2))

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('before',type=Path);p.add_argument('after',type=Path);p.add_argument('--output',required=True,type=Path);p.add_argument('--tests',type=Path);p.add_argument('--native',type=Path);a=p.parse_args();summarize(a.before,a.after,a.output,a.tests,a.native)
