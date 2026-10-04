"""Generate, measure, and validate producer delta markers without demanding
identical internal tables. Acceptance is scoped to this measured workload."""
import argparse,hashlib,json,pathlib,shutil,statistics,subprocess
p=argparse.ArgumentParser()
p.add_argument('--source',type=pathlib.Path,required=True)
p.add_argument('--history',type=pathlib.Path,required=True)
p.add_argument('--inner',choices=['Base','TupleT'],required=True)
p.add_argument('--output',type=pathlib.Path,required=True)
p.add_argument('--repeats',type=int,default=3)
p.add_argument('--term-checker',type=pathlib.Path,default=pathlib.Path('../check_statewalk/target/release/eggcc-term-validate'))
a=p.parse_args();out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
binroot=pathlib.Path('target/release/examples').resolve();generated=out/'generated';validation=out/'legality';measurement=out/'measurement'
def run(args,name):
    with (out/(name+'.log')).open('w') as log:subprocess.run([str(x) for x in args],stdout=log,stderr=log,check=True)
run([binroot/'delta_marker',a.source.resolve(),a.history.resolve(),a.inner,generated],'generate')
files=[generated/(s+'.egg') for s in ['baseline','producer-only','candidate']]
run([binroot/'eggcc_marker','measure',measurement,a.repeats,*files],'measure')
run([binroot/'marker_validate',files[0],files[2],generated/'manifest.json',validation],'legality')
run([a.term_checker.resolve(),validation/'input.term',validation/'baseline.term',validation/'candidate.term',validation/'eggcc-check.json'],'eggcc')
m=json.loads((measurement/'measurement.json').read_text());v=json.loads((validation/'report.json').read_text());e=json.loads((validation/'eggcc-check.json').read_text());manifest=json.loads((generated/'manifest.json').read_text())
accepted=not v['marker_violations'] and v['schedule_walk_matches_plain_execution'] and all(x['in_original_root_eclass'] for x in v['function_membership']) and all(not x['failures'] for x in v['to_hoist'].values()) and e['passed']
metrics=[]
for i in range(3):
    q=m['profiles'][i];samples=[s for s in m['samples'] if s['variant']==i]
    metrics.append(dict(variant=['baseline','producer-only','candidate'][i],scan_rows=q['scan_rows'],refine_rows=q['refine_rows'],key_lookups=q['key_lookups'],marker_rows=m['marker_rows'][i],execution_ms_median=1000*statistics.median(s['execution_seconds'] for s in samples),setup_ms_median=1000*statistics.median(s['setup_seconds'] for s in samples)))
paired=[]
for r in range(a.repeats):
    sample={s['variant']:s for s in m['samples'] if s['round']==r};paired.append(100*(1-sample[2]['execution_seconds']/sample[0]['execution_seconds']))
result=dict(scope='Rule-level producer-marker experiment on actual native runtime. Marker legality and extracted-program witnesses, not identical internal tables or a universal optimization proof.',
    source_sha256=hashlib.sha256(a.source.read_bytes()).hexdigest(),history_sha256=hashlib.sha256(a.history.read_bytes()).hexdigest(),
    generator_sha256=hashlib.sha256((binroot/'delta_marker').read_bytes()).hexdigest(),repeats=a.repeats,inner=a.inner,
    accepted_for_measured_instance=accepted,legality=v,eggcc=e,manifest=manifest,metrics=metrics,
    original_table_equality_diagnostic=m['profile_same_original_snapshot'],
    row_work_reduction_percent=100*(1-(metrics[2]['scan_rows']+metrics[2]['refine_rows'])/(metrics[0]['scan_rows']+metrics[0]['refine_rows'])),
    paired_execution_improvement_percent=paired,paired_execution_improvement_percent_median=statistics.median(paired),
    production_dag_extractor_validated=False)
if accepted:
    shutil.copyfile(files[2],out/'instance-validated.egg');result['program']='instance-validated.egg'
(out/'summary.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps({k:result[k] for k in ['accepted_for_measured_instance','metrics','row_work_reduction_percent','paired_execution_improvement_percent_median']},indent=2))

if not accepted:
    raise SystemExit('Measured-instance legality checks failed; no validated program emitted')
