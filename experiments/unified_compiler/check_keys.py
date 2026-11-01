"""Active SK + marker compiler artifacts on native, Cell CPU and resident Metal.

No raw-core benchmark lane. Default fixtures are tiny; this never runs Raytrace.
"""
import argparse,json,subprocess,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];sys.path.insert(0,str(ROOT))
from eggfuse.compiler.pipeline import compile_pipeline
from eggfuse.engine.cell_runtime import run_artifact as run_cpu,binary
from eggfuse.engine.metal import run_artifact as run_metal
from eggfuse.engine.cpu import compare_native
from experiments.device_backend.export_snapshot import gpu_rows
from experiments.device_backend.run import compare

def main():
 p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True);p.add_argument('--solver-python',type=Path,required=True);p.add_argument('--cases',nargs='+',default=['key-spine','key-values','key-refresh','key-depth']);a=p.parse_args()
 a.output.mkdir(parents=True,exist_ok=False);engine=binary();tools=ROOT/'target/debug/examples';summary={}
 for name in a.cases:
  if name not in ('key-spine','key-values','key-refresh','key-depth'):raise ValueError('only the tiny checked-in key fixtures are admitted')
  source=ROOT/'experiments/unified_compiler'/(name+'.egg');d=a.output/name;d.mkdir()
  pipeline=d/'compile'
  compile_pipeline(source,pipeline,d/'build',tools,a.solver_python.absolute(),stop_after='device',backend='cell-cpu')
  report=json.loads((pipeline/'pipeline.json').read_text());sk=Path(next(s['directory'] for s in report['passes'] if s['name']=='structural_keys'))/'result/report.json'
  choices=json.loads(sk.read_text())['selected_choices'];assert choices
  markers=json.loads((pipeline/'marker-program.json').read_text())['markers']
  if name=='key-spine':assert len(markers)>0
  artifact=pipeline/'eggfuse_related_files';core=json.loads((artifact/'core.json').read_text())
  cpu=run_cpu(artifact,d/'cpu',check=True,engine=engine)
  assert cpu['report']['stats']['key_operations']>0
  assert cpu['report']['stats']['merge_contract_calls']['ranked-selection']>0
  with (d/'native.log').open('w') as log:
   subprocess.run([str(tools/'eggfuse_frontend'),str(artifact/'source_sk_mk.egg'),'--native-full','--export-to',str(d/'native')],stdout=log,stderr=subprocess.STDOUT,check=True)
  reference=json.loads((d/'native/native-state.json').read_text());checks=set(core.get('assertion_tables',[]))
  state=json.loads((d/'cpu/runtime/state.json').read_text());state['rows']=[r for r in state['rows'] if r['table'] not in checks]
  cpu_proof=compare_native(core,state,reference)
  if name=='key-spine':
   original=json.loads((artifact/'program.json').read_text())
   for field in ('kind','reads','constructs','equal_inputs'):
    forged=json.loads(json.dumps(original));c=next(c for c in forged['merge_contracts']['tables'] if c['kind']=='ranked-selection')
    c[field]='lattice' if field=='kind' else [] if field=='reads' else ['forged-constructor'] if field=='constructs' else 'return_old'
    file=d/('forged-'+field+'.json');file.write_text(json.dumps(forged))
    rejected=subprocess.run([str(engine),'--program',str(file),'--inspect-program'],capture_output=True,text=True)
    assert rejected.returncode and 'merge contract' in rejected.stderr,(field,rejected.stderr)
   forged=json.loads(json.dumps(original));forged['merge_contracts']['tables']='malformed'
   file=d/'forged-schema.json';file.write_text(json.dumps(forged))
   rejected=subprocess.run([str(engine),'--program',str(file),'--inspect-program'],capture_output=True,text=True)
   assert rejected.returncode and 'merge contract' in rejected.stderr,rejected.stderr

  if name=='key-refresh':
   from eggfuse.engine.cell_runtime import execute
   program=json.loads((artifact/'program.json').read_text())
   execute(program,d/'paused',engine,checkpoint_after=3)
   execute(program,d/'resumed',engine,resume=d/'paused/runtime/checkpoint.json')
   restored=json.loads((d/'resumed/runtime/state.json').read_text());restored['rows']=[r for r in restored['rows'] if r['table'] not in checks]
   compare_native(core,restored,reference)
   assert json.loads((d/'resumed/runtime/state.json').read_text())==json.loads((d/'cpu/runtime/state.json').read_text())

  compile_pipeline(source,pipeline,d/'build',tools,a.solver_python.absolute(),stop_after='device',backend='gpu',multi_stage_cell=True,capacity=512,match_capacity=2048)
  # Both targets consume the same cached source SK/marker/Recipe passes.
  assert json.loads((artifact/'core.json').read_text())==core
  build=d/'build';build.mkdir(exist_ok=True);(build/'frontend').symlink_to(tools/'eggfuse_frontend')
  gpu=run_metal(artifact,build,d/'metal',check=True)
  leaf=d/'metal/program-0/cell-0';program=json.loads((leaf/'program.json').read_text());s=json.loads((leaf/'gpu-state.json').read_text())
  assert s['structural_keys'][1]>0 and s['structural_keys'][2]>0
  rows=[r for r in s['rows'] if program['tables'][r[0]]['name'] not in checks]
  gpu_proof=compare(program,rows,s['parents'],reference,s['sets'],s['structural_keys'])
  summary[name]={'selected_choices':choices,'markers':len(markers),'cpu':cpu_proof,'metal':gpu_proof,'metal_replay':gpu['verification'],
                 'cpu_operations':cpu['report']['stats'],'metal_key_header':s['structural_keys'][:8],
                 'scope':'correctness with active SK/marker passes; NOT a performance benchmark'}
  (a.output/'results.json').write_text(json.dumps(summary,indent=2)+'\n');print(name,cpu_proof,gpu_proof,flush=True)
if __name__=='__main__':main()
