"""Exact native and on/off checks for device-dispatched joint recipe jobs."""
import argparse,json,os
from pathlib import Path
from run import run
from compare_runs import compare
from eggfuse.compiler.recipes.executable import compile_library
from test_joint_polyspine import SCHEDULE

def tests(build,evidence,out):
 out.mkdir(parents=True,exist_ok=False)
 training=(evidence/'training.egg').read_text();base=training[:training.index('(let a (Const')]
 core=json.loads((evidence/'training-reference/core.json').read_text())
 artifact=compile_library(core,json.loads((evidence/'analysis-v2/analysis.json').read_text()),training,spines=True)
 recipe=out/'recipes.json';recipe.write_text(json.dumps(artifact))
 source=out/'wide.egg'
 source.write_text(base+''.join(f'(Bop (Add) (Const (Int {i}) (Base (IntT)) (InFunc "wide")) (Const (Int {i+1024}) (Base (IntT)) (InFunc "wide")))\n' for i in range(384))+SCHEDULE)
 results={}
 for mode in ['off','on']:
  os.environ['EGGFUSE_JOINT_FUSE_PREPARE']=os.environ['EGGFUSE_JOINT_LOCAL_SORT']=os.environ['EGGFUSE_JOINT_SHARED_ENCODER']='1' if mode=='on' else '0'
  r=run(source,build,out/mode,capacity=32768,match_capacity=65536,check_at_end=True,stage_contexts=True,stage_context_audit=True,cell_program=recipe)
  assert r['checks'][-1]['exact_rows_values_equivalence_visibility']
  phases=[p for c in r['joint_polyspine']['calls'] for p in c['phase_history'] if p[0]<2]
  assert max(p[4] for p in phases)>256
  assert any(p[4]==0 for p in phases)
  if mode=='on':assert r['joint_execution']['removed_host_fences']==len(phases)
  results[mode]=dict(check=r['checks'][-1],execution=r['joint_execution'],max_phase_matches=max(p[4] for p in phases))
 results['on_off']=compare(out/'off',out/'on',device_states=True,semantic_device=True,saturation_macros=True)['final_check']
 (out/'results.json').write_text(json.dumps(results,indent=2));return results
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--build',type=Path,required=True);p.add_argument('--evidence',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args();print(json.dumps(tests(a.build,a.evidence,a.output),indent=2))
