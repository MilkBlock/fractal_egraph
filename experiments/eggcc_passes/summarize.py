"""Summarize separately measured production passes and frozen-library coverage."""
import argparse,hashlib,json,pathlib
p=argparse.ArgumentParser();p.add_argument('--input',type=pathlib.Path,default=pathlib.Path('out/eggcc-pass-audit'));p.add_argument('--library',type=pathlib.Path,default=pathlib.Path('out/ripsat-coverage-remeasure-merged/train/library.json'));p.add_argument('--output',type=pathlib.Path,default=pathlib.Path('experiments/eggcc_passes/results.json'));a=p.parse_args()
rows=[]
for name in ['fact','loopfact']:
 for i in range(1,4):
  folder=a.input/name;t=json.loads((folder/f'pass-{i}.json').read_text());c=json.loads((folder/f'coverage-stage{i}/report.json').read_text());s=c['structure'];m=c['matches'];e=c['effective_matches']
  assert c['same_final_state_as_native_control']
  assert c['signature_count_before']==c['signature_count_after']
  assert t['search_and_binding_seconds']>=0
  times={k.removesuffix('_seconds')+'_ms':round(v*1000,6) for k,v in t.items() if k.endswith('_seconds')}
  times['setup_and_other_ms']=times['execution_ms']-times['search_and_apply_ms']-times['insert_merge_ms']-times['rebuild_ms']
  def fraction(n,d):return {'covered':n,'total':d,'percent':round(100*n/d,3) if d else None}
  coverage={'expr_enodes':fraction(s['covered_expr_enodes'],s['expr_enodes']),'expr_eclasses':fraction(s['covered_expr_eclasses'],s['expr_eclasses']),'constructor_rows':fraction(s['covered_constructor_rows'],s['stored_constructor_rows']),'pre_step_full_lhs':fraction(m['all_reads_one_cell'],m['logical']),'pre_step_two_reads':fraction(m['multiple_internal_atoms'],m['logical']),'effective_signature_hits':fraction(e['known_effect_signature'],e['logical'])}
  rows.append({'workload':name,'pass':i,'timing':times,'production_nodes':t['nodes'],'production_eclasses':t['eclasses'],'production_matches':t['matches'],'source_sha256':hashlib.sha256((folder/f'pass-{i}.egg').read_bytes()).hexdigest(),'flat_source_sha256':hashlib.sha256((folder/f'pass-{i}-flat.egg').read_bytes()).hexdigest(),'coverage':coverage,'coverage_native_control_equal':True,'frozen_signatures':c['signature_count_after'],'patterns':c['library_patterns']})
result={'date':'2026-10-07','runs_per_workload':1,'schedule':'parallel, all three production passes; extraction feeds next pass','use_context':False,'timing_engine':'check_statewalk production egglog ebba7bb / experimental 08771f9, with temporary counters','extraction':'actual production Tiger pipeline, no ILP','coverage_engine':'separate egg_layout native diagnostic on exported actual per-pass sources','library_sha256':hashlib.sha256(a.library.read_bytes()).hexdigest(),'library_scope':'Existing frozen fact Pass-1 observation library: 163 pattern entries, not separately baked per-pass certified cells. fact is in-training, loopfact held-out. Later passes measure transfer.','limitations':['Search is search/binding wall time minus instrumented RHS intervals, not just hash probes.','RHS includes primitive work, constructor lookups and staging writes.','Insert/merge is the shared table-commit phase; no separate pure-insert timer is claimed.','Execution includes egraph construction, parsing, typechecking, rule installation and scheduling; source-program generation is outside this timer.','Coverage has its own native traced control; it does not establish cross-engine whole-state identity or certified transition coverage.','Counters introduce overhead. No averaging, speedup claim or cross-pass combined graph denominator.'],'rows':rows}
a.output.write_text(json.dumps(result,indent=2)+'\n')
for r in rows:
 print(r['workload'],r['pass'],{k:round(v,2) for k,v in r['timing'].items()}, {k:v['percent'] for k,v in r['coverage'].items()})
