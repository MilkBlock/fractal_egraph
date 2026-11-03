"""One candidate run per small Pass 3, with saved, fingerprinted native controls."""
import argparse,hashlib,json,subprocess,sys,time
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];sys.path.insert(0,str(ROOT))
from eggfuse.engine.cpu import binary,compare_native
from eggfuse.compiler.targets.cpu import lower

def main():
 p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True);p.add_argument('--cpu-bin',type=Path);p.add_argument('--controls',type=Path,default=ROOT/'out/cpu-cell-release-bench');p.add_argument('--cases',nargs='+',default=['fact','loopfact','sum-bits']);a=p.parse_args()
 engine=a.cpu_bin.resolve() if a.cpu_bin else binary();a.output.mkdir(parents=True,exist_ok=False)
 suite=json.loads((ROOT/'experiments/profile_suite/suite.json').read_text());summary={}
 for name in a.cases:
  case=next(c for c in suite['cases'] if c['name']==name);src=a.output/(name+'.egg')
  data=b''.join((ROOT/'experiments/profile_suite'/part).read_bytes() for part in case['parts']);assert hashlib.sha256(data).hexdigest()==case['sha256'];src.write_bytes(data)
  saved=a.controls/(name+'-pass3');ref=json.loads((saved/'report.json').read_text())
  assert ref['source_sha256']==case['sha256'] and ref['verification']['exact_rows_values_equivalence_visibility']
  d=a.output/name;d.mkdir();subprocess.run([str(engine),'--source',str(src),'--prepare',str(d/'core.json')],check=True)
  core=json.loads((d/'core.json').read_text());assert core==json.loads((saved/'core.json').read_text())
  start=time.perf_counter();plan=lower(core);compile_seconds=time.perf_counter()-start;(d/'program.json').write_text(json.dumps(plan))
  with (d/'candidate.log').open('w') as log:
   subprocess.run([str(engine),'--source',str(src),'--mode','staged','--plan',str(d/'program.json'),'--output',str(d/'candidate')],stdout=log,stderr=subprocess.STDOUT,check=True)
  candidate=json.loads((d/'candidate/report.json').read_text())
  try:proof=compare_native(core,json.loads((d/'candidate/state.json').read_text()),json.loads((saved/'native/state.json').read_text()))
  except AssertionError as e:proof={'exact_rows_values_equivalence_visibility':False,'error':str(e)}
  summary[name]={'candidate_seconds':candidate['inference_seconds'],'historical_native_seconds':ref['native']['inference_seconds'],'historical_control':str(saved),'historical_binary_sha256':ref['binary_sha256'],'candidate_binary_sha256':hashlib.sha256(engine.read_bytes()).hexdigest(),'source_sha256':case['sha256'],'recipe_compile_seconds':compile_seconds,'setup_seconds':candidate['setup_seconds'],'verification':proof,'staged':candidate['staged']}
  (a.output/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
  print(name,'candidate',candidate['inference_seconds'],'historical native',ref['native']['inference_seconds'],'verified',proof['exact_rows_values_equivalence_visibility'],'reuse',candidate['staged']['reused_steps'],flush=True)
 if not all(x['verification']['exact_rows_values_equivalence_visibility'] for x in summary.values()):raise SystemExit(1)
if __name__=='__main__':main()
