"""Real eggcc context rules through Recipe IR and the exact GPU interpreter."""
import argparse,copy,hashlib,json,subprocess,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];sys.path.insert(0,str(ROOT));sys.path.insert(0,str(ROOT/'experiments/device_backend'))
from test_generic_cells import core_for
from run_generic_cells import run_library
from verify_native_source import verify
from eggfuse.ir.recipe import build,validate,instantiate,digest
from eggfuse.analyzer.recipes import adapt

FACTS='''(let ty (Base (IntT)))
(let oldctx (InFunc "old")) (let ctx (InFunc "new"))
(let x (Arg ty oldctx)) (let y (Const (Int 2) ty oldctx))
(let t (Bop (Add) (Bop (Mul) x y) (Get (Concat (Single x) (Single y)) 1)))
(let result (AddContext ctx t))
(AddContext ctx (AddContext ctx t))
'''
TERMS='(saturate (run terms) (saturate (run terms-helpers) (saturate (run terms-helpers-helpers))))'

def tests(source,build_dir,out):
 out.mkdir(parents=True,exist_ok=False);text=source.read_text();base=text[:text.index('(ruleset initialization)')]
 late='''(ruleset later)
(rule ((= a (Arg ty oldctx)) (= b (Const (Int 2) ty oldctx))) ((union a b)) :ruleset later)
'''
 cases={
  'saturated':base+FACTS+'(run-schedule (saturate (run context)))',
  'finite':base+FACTS+'(run context 1)',
  'late-union':base+late+FACTS+'(run-schedule (saturate (run context))) (run later 1) (run-schedule (saturate (run context)))',
  'context-to-terms':base+FACTS+'(run-schedule (saturate (run context)) '+TERMS+')',
 }
 cases['renamed']=cases['saturated'].replace('AddContextList','AttachScopeList').replace('AddContext','AttachScope').replace(':ruleset context',':ruleset attach-scope').replace('(ruleset context)','(ruleset attach-scope)').replace('(run context)','(run attach-scope)')
 results=dict(source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),scope='real eggcc rule bodies; small input; exact generic GPU execution, not a global shortcut or speedup',cases={})
 for name,text in cases.items():
  file=out/(name+'.egg');file.write_text(text);core=core_for(file,build_dir);ir=build(core)
  validate(ir,core);assert instantiate(ir,core['initial'])==core
  context_rules=[r for r in ir['rules'] if r['metadata'].get('source_set') in ['context','attach-scope']]
  assert len(context_rules)==17
  assert any('construct' in r['footprint']['operations'] and 'set' in r['footprint']['operations'] for r in context_rules)
  assert any('union' in r['footprint']['operations'] for r in context_rules)
  (out/(name+'.recipe.json')).write_text(json.dumps(ir,indent=2))
  initial=core.pop('initial');lib=adapt(dict(schema='analyzer-device-cells/v1',programs=[core],instances=[dict(unit=0,program=0,initial=initial)]))
  result=run_library(lib,build_dir,out/name,capacity=512,match_capacity=8192)
  assert result['complete']
  checked=verify(file,build_dir,out/name/'program-0/cell-0',out/(name+'-native'))
  results['cases'][name]=dict(check=checked,context_rules=len(context_rules),event_routes=len(ir['event_routes']),operations=ir['capabilities']['operations'],runs=len(result['programs'][0]['instances'][0]['history']))
  print(name,results['cases'][name],flush=True)
 assert results['cases']['finite']['runs']==1
 assert results['cases']['saturated']['runs']>1
 assert results['cases']['renamed']['check']['rows']==results['cases']['saturated']['check']['rows']
 (out/'results.json').write_text(json.dumps(results,indent=2));return results

if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--source',type=Path,required=True);p.add_argument('--build',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args();tests(a.source,a.build,a.output)
