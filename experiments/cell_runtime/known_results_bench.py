"""Metrics-first existing-result experiment, always on the merge-program baseline."""
import argparse,json,os
from pathlib import Path
from eggfuse.engine.cell_runtime import execute,load_pipeline_artifact
from experiments.cell_runtime.reference_views_bench import reemit
from experiments.cell_runtime.compositions_bench import mode,metrics
from experiments.cell_runtime.suite import timed
from experiments.cell_runtime.refutation_bench import audit
ROOT=Path(__file__).resolve().parents[2]
def main():
    p=argparse.ArgumentParser();g=p.add_mutually_exclusive_group(required=True);g.add_argument('--pipeline',type=Path);g.add_argument('--compiled',type=Path)
    p.add_argument('--output',type=Path,required=True);p.add_argument('--observe',action='store_true');p.add_argument('--audit',action='store_true');p.add_argument('--method',choices=['on','value','static'],default='on');a=p.parse_args()
    out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
    path,program,provenance=load_pipeline_artifact(a.compiled.resolve()) if a.compiled else reemit(a.pipeline.resolve(),out/'compile')
    if 'known_results' not in program:raise ValueError('re-emit known result plans')
    settings={'CELL_CPU_MERGE_PROGRAMS':'1','CELL_CPU_KNOWN_RESULTS':'off','CELL_CPU_UNION_GRAFT':'0','CELL_CPU_UNION_TRACE':'0','CELL_CPU_UNION_IMPACT':'0','CELL_CPU_MARKER_REFUTATION':'0','CELL_CPU_SIEVE':'0','CELL_CPU_CONGRUENCE':'0'}
    old={k:os.environ.get(k) for k in settings};engine=ROOT/'engine/cell-runtime/target/release/eggfuse-cell-runtime'
    result={'scope':'standalone Cell CPU, fixed default merge baseline, identical program and stage control','program':str(path),'provenance':provenance,'plans':len(program['known_results']['plans'])}
    try:
        os.environ.update(settings)
        with mode(False):
            if a.observe:
                os.environ['CELL_CPU_KNOWN_RESULTS']='observe';r=execute(program,out/'observe',engine)
                result.update(timing_eligible=False,known_results=r['known_results'],stats=r['stats'])
                print(json.dumps(r['known_results'],indent=2),flush=True)
            else:
                result['lanes']={};states=[];schedules=[]
                for name in ('off','on'):
                    os.environ['CELL_CPU_KNOWN_RESULTS']=a.method if name=='on' else 'off';m,r=timed(path,engine,out/name)
                    assert r['features']['merge_programs']
                    result['lanes'][name]={'measurement':m,**metrics(r),'known_results':r['known_results']}
                    states.append(json.loads((out/name/'state.json').read_text()))
                    schedules.append([{k:s[k] for k in ('ruleset','path','iterations','epoch','logical_matches','changed')} for s in r['steps']])
                    print(name,m['inference_seconds'],r['known_results'],flush=True)
                assert states[0]==states[1] and schedules[0]==schedules[1]
                result['exact_endpoint_and_schedule']=True
                result['ratio']=result['lanes']['on']['measurement']['inference_seconds']/result['lanes']['off']['measurement']['inference_seconds']
            (out/'results.json').write_text(json.dumps(result,indent=2)+'\n')
            if a.audit:
                # Streaming auditor uses 0/1; these aliases mean off/on.
                result['verification']=audit(path,program,out/'audit',ROOT/'engine/cell-runtime/target/release/examples/project_stages','CELL_CPU_KNOWN_RESULTS',enabled=a.method)
                (out/'results.json').write_text(json.dumps(result,indent=2)+'\n');print(result['verification'],flush=True)
    finally:
        for k,v in old.items():
            if v is None:os.environ.pop(k,None)
            else:os.environ[k]=v
if __name__=='__main__':main()
