"""One paired run of the actual analyze CLI; compare every completed final state."""
import argparse,json,os,pathlib,subprocess,time,hashlib
p=argparse.ArgumentParser();p.add_argument('--output',type=pathlib.Path,required=True);p.add_argument('--source',type=pathlib.Path,default=pathlib.Path('experiments/ripsat_graft/integrated.egg'));a=p.parse_args()
a.output.mkdir(parents=True,exist_ok=False)
binary=pathlib.Path('target/release/egg_layout').resolve();reports={};results={}
for mode in ['rebuild','snapshot']:
    out=(a.output/mode).resolve();start=time.monotonic()
    env={**os.environ,'EGG_LAYOUT_GRAPHS':'0','EGG_LAYOUT_ANALYSIS_THREADS':'2','RUST_MIN_STACK':'16777216','EGG_LAYOUT_RIPEN_PARENT_REUSE':'0' if mode=='rebuild' else '1'}
    with (a.output/(mode+'.log')).open('w') as log:
        subprocess.run([str(binary),'analyze','--source',str(a.source.resolve()),'--output',str(out)],env=env,stdout=log,stderr=log,check=True)
    wall=time.monotonic()-start
    r=json.loads((out/'analysis.json').read_text())['saturated_rule_composition'];reports[mode]=r
    jobs=r['jobs'];results[mode]={'wall_seconds':wall,'counts':r['counts'],'parent_snapshot_reuse':r['parent_snapshot_reuse'],
        'initialization_seconds':sum(j.get('initialization',{}).get('seconds',0) for j in jobs),
        'initialization_paths':[{'job':j['candidate_id'],'path':j.get('initialization')} for j in jobs if j.get('initialization')]}
a_jobs=reports['rebuild']['jobs'];b_jobs=reports['snapshot']['jobs'];assert len(a_jobs)==len(b_jobs)
verified=0
for x,y in zip(a_jobs,b_jobs):
    assert (x['candidate_id'],x['state'])==(y['candidate_id'],y['state'])
    if x['state']=='Saturated':
        paths=[str(pathlib.Path(j['folder'])/'saturated-rule-composition.json') for j in [x,y]]
        comparison=json.loads(subprocess.check_output([str(binary),'saturated-rule-composition-compare',*paths],text=True))
        assert comparison['status']=='equivalent',comparison
        verified+=1
summary={'scope':'Actual default analyze CLI, positive arithmetic-shaped integration fixture; not eggcc or math-microbenchmark speedup',
         'source_sha256':hashlib.sha256(a.source.read_bytes()).hexdigest(),'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),
         'equivalent_completed_states':verified,'modes':results}
(a.output/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps({mode:{k:v for k,v in r.items() if k!='initialization_paths'} for mode,r in results.items()},indent=2))
