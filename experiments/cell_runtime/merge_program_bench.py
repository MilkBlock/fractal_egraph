"""Same typed pipeline and source schedule; one clean timing per mode."""
import argparse,json,os
from pathlib import Path
from experiments.cell_runtime.reference_views_bench import reemit
from experiments.cell_runtime.compositions_bench import mode,metrics
from experiments.cell_runtime.refutation_bench import audit
from experiments.cell_runtime.suite import timed
from eggfuse.engine.cell_runtime import load_pipeline_artifact

ROOT=Path(__file__).resolve().parents[2]

def main():
    p=argparse.ArgumentParser();g=p.add_mutually_exclusive_group(required=True);g.add_argument('--pipeline',type=Path);g.add_argument('--compiled',type=Path);p.add_argument('--output',type=Path,required=True);p.add_argument('--audit',action='store_true');a=p.parse_args()
    out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
    path,program,provenance=load_pipeline_artifact(a.compiled.resolve()) if a.compiled else reemit(a.pipeline.resolve(),out/'compile')
    settings={'CELL_CPU_MERGE_PROGRAMS':'0','CELL_CPU_UNION_GRAFT':'0','CELL_CPU_UNION_TRACE':'0',
        'CELL_CPU_UNION_IMPACT':'0','CELL_CPU_MARKER_REFUTATION':'0','CELL_CPU_SIEVE':'0','CELL_CPU_CONGRUENCE':'0'}
    old={k:os.environ.get(k) for k in settings};result={'scope':'same-binary standalone Cell CPU, compiled ordered merges + scheduled consumers, not native egglog',
        'program':str(path),'provenance':provenance,'lanes':{},'compiled_programs':len(program['merge_programs']['programs']),
        'compiled_entries':len(program['merge_programs']['entries']),'terminal_cell_substitution':False}
    try:
        os.environ.update(settings)
        with mode(False):
            states=[];schedules=[]
            for name,flag in [('off','0'),('on','1')]:
                os.environ['CELL_CPU_MERGE_PROGRAMS']=flag
                m,r=timed(path,ROOT/'engine/cell-runtime/target/release/eggfuse-cell-runtime',out/name)
                result['lanes'][name]={'measurement':m,**metrics(r),'merge_program_calls':r['stats']['merge_program_calls'],
                    'merge_program_operations':r['stats']['merge_program_operations'],'merge_program_constructs':r['stats']['merge_program_constructs'],
                    'continuation_stats':r['multi_stage']['stats']}
                states.append(json.loads((out/name/'state.json').read_text()))
                schedules.append([{k:s[k] for k in ('ruleset','path','iterations','epoch','logical_matches','changed')} for s in r['steps']])
                print(name,m['inference_seconds'],r['stats']['merge_program_calls'],r['multi_stage']['stats']['merge_bindings'],flush=True)
            assert states[0]==states[1] and schedules[0]==schedules[1]
            result['exact_endpoint_and_schedule']=True
            result['ratio']=result['lanes']['on']['measurement']['inference_seconds']/result['lanes']['off']['measurement']['inference_seconds']
            (out/'results.json').write_text(json.dumps(result,indent=2)+'\n')
            if a.audit:
                result['verification']=audit(path,program,out/'audit',ROOT/'engine/cell-runtime/target/release/examples/project_stages','CELL_CPU_MERGE_PROGRAMS')
                (out/'results.json').write_text(json.dumps(result,indent=2)+'\n');print(result['verification'],flush=True)
    finally:
        for k,v in old.items():
            if v is None:os.environ.pop(k,None)
            else:os.environ[k]=v

if __name__=='__main__':main()
