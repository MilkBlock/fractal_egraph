"""Load every prepared native Cell in fresh processes; keep proof scopes separate."""
import argparse,json,hashlib
from pathlib import Path
from eggfuse.engine.cell_runtime import execute
from eggfuse.ir.cell_runtime import build
from eggfuse.engine.cpu import compare_native
ROOT=Path(__file__).resolve().parents[2]
def main():
    p=argparse.ArgumentParser();p.add_argument('--analysis',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--oracle',type=Path);a=p.parse_args()
    out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
    source=a.analysis/'cell-device-recipes.json';lib=json.loads(source.read_text())
    engine=ROOT/'engine/cell-runtime/target/release/eggfuse-cell-runtime';binary_sha=hashlib.sha256(engine.read_bytes()).hexdigest()
    refs={r['unit']:r['checkpoint'] for r in json.loads(a.oracle.read_text())['references']} if a.oracle else {}
    results=[]
    for inst in lib['instances']:
        core={**lib['programs'][inst['program']],'initial':inst['initial']};program=build(core);root=out/str(inst['unit']);root.mkdir()
        reports=[]
        for flag in ('0','1'):
            reports.append(execute(program,root/flag,engine,audit=True,environment={
                'CELL_CPU_MERGE_PROGRAMS':flag,'CELL_CPU_UNION_IMPACT':'0','CELL_CPU_UNION_GRAFT':'0','CELL_CPU_UNION_TRACE':'0',
                'CELL_CPU_MARKER_REFUTATION':'0','CELL_CPU_SIEVE':'0','CELL_CPU_CONGRUENCE':'0'}))
        for name in ('state.json','stages.json','binding-audit.json'):
            assert json.loads((root/'0/runtime'/name).read_text())==json.loads((root/'1/runtime'/name).read_text()),(inst['unit'],name)
        r=reports[1];v={'unit':inst['unit'],'runs':len(r['steps']),'merge_calls':r['stats']['merge_program_calls'],
            'consumer_bindings':r['multi_stage']['stats']['merge_bindings'],'exact_stages_and_bindings':True}
        if inst['unit'] in refs:
            try:compare_native(core,json.loads((root/'1/runtime/state.json').read_text()),refs[inst['unit']]);v['native_equal']=True
            except (AssertionError,ValueError) as e:v['native_equal']=False;v['native_difference']=str(e)
        results.append(v)
    assert binary_sha==hashlib.sha256(engine.read_bytes()).hexdigest(),'binary changed during verification'
    result={'scope':'fresh-process execution of native prepared Cell programs; not terminal reuse or timing',
        'source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'binary_sha256':binary_sha,'instances':len(results),'results':results,
        'native_compared':sum('native_equal' in r for r in results),'native_equal':sum(r.get('native_equal',False) for r in results),
        'exact_stage_runs':sum(r['runs'] for r in results),'merge_program_calls':sum(r['merge_calls'] for r in results),
        'consumer_bindings':sum(r['consumer_bindings'] for r in results),'terminal_cell_substitution':False}
    (out/'results.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps({k:v for k,v in result.items() if k!='results'},indent=2))
if __name__=='__main__':main()
