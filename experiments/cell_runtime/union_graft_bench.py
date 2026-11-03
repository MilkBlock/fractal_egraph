"""Reuse SK/Bake/markers; re-emit CPU entries, measure off/lineage/graft once."""
import argparse, json, os
from pathlib import Path
from experiments.cell_runtime.reference_views_bench import reemit
from experiments.cell_runtime.refutation_bench import audit
from experiments.cell_runtime.compositions_bench import mode, metrics
from experiments.cell_runtime.suite import timed
from eggfuse.engine.cell_runtime import load_pipeline_artifact
ROOT=Path(__file__).resolve().parents[2]
def main():
    p=argparse.ArgumentParser();g=p.add_mutually_exclusive_group(required=True);g.add_argument('--pipeline',type=Path);g.add_argument('--compiled',type=Path)
    p.add_argument('--output',type=Path,required=True);p.add_argument('--audit',action='store_true');a=p.parse_args()
    a.output.mkdir(parents=True,exist_ok=False)
    path,program,provenance=load_pipeline_artifact(a.compiled.resolve()) if a.compiled else reemit(a.pipeline.resolve(),a.output/'compile')
    result={'scope':'single-thread standalone Cell CPU; effective union lineage and exact two-atom entry; not pristine egglog',
        'provenance':provenance,'program':str(path),'compiled_entries':len(program['multi_stage']['union_entries']),'lanes':{}}
    envkeys=['CELL_CPU_UNION_GRAFT','CELL_CPU_UNION_TRACE','CELL_CPU_MARKER_REFUTATION','CELL_CPU_SIEVE'];old={k:os.environ.get(k) for k in envkeys}
    try:
        with mode(False):
            os.environ.update(CELL_CPU_MARKER_REFUTATION='0',CELL_CPU_SIEVE='0');states=[];schedules=[]
            engine=ROOT/'engine/cell-runtime/target/release/eggfuse-cell-runtime'
            for name,trace,graft in [('off','0','0'),('lineage','1','0'),('graft','0','1')]:
                os.environ.update(CELL_CPU_UNION_TRACE=trace,CELL_CPU_UNION_GRAFT=graft)
                m,r=timed(path,engine,a.output/name);states.append(json.loads((a.output/name/'state.json').read_text()))
                schedules.append([{k:s[k] for k in ('ruleset','iterations','path','epoch','logical_matches','changed')} for s in r['steps']])
                result['lanes'][name]={'measurement':m,**metrics(r),'graft':r['union_graft'],'union_lineage':r['union_lineage'],
                    'point_program_stats':r['multi_stage']['stats']}
                print(name,m['inference_seconds'],r['union_graft'],flush=True)
                (a.output/'results.json').write_text(json.dumps(result,indent=2)+'\n')
            assert states[0]==states[1]==states[2] and schedules[0]==schedules[1]==schedules[2]
            result['exact_endpoint_and_schedule']=True
            if a.audit:
                os.environ.update(CELL_CPU_UNION_TRACE='0',CELL_CPU_UNION_GRAFT='0')
                result['verification']=audit(path,program,a.output/'audit',ROOT/'engine/cell-runtime/target/release/examples/project_stages','CELL_CPU_UNION_GRAFT')
            (a.output/'results.json').write_text(json.dumps(result,indent=2)+'\n')
            print(result.get('verification'),flush=True)
    finally:
        for k,v in old.items():
            if v is None:os.environ.pop(k,None)
            else:os.environ[k]=v
if __name__=='__main__':main()
