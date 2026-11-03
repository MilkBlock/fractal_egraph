"""Pre-search context experiment on an authenticated source_sk_mk pipeline.

Reuse existing timers and full-stage auditor; never rebuild/retime native controls.
"""
import argparse,collections,json,os
from pathlib import Path
from eggfuse.engine.cell_runtime import execute,load_pipeline_artifact
from experiments.cell_runtime.compositions_bench import mode,metrics
from experiments.cell_runtime.suite import timed,fingerprint
from experiments.cell_runtime.refutation_bench import audit

ROOT=Path(__file__).resolve().parents[2]
SETTINGS={'CELL_CPU_MERGE_PROGRAMS':'1','CELL_CPU_KNOWN_RESULTS':'off',
    'CELL_CPU_UNION_GRAFT':'0','CELL_CPU_UNION_TRACE':'0','CELL_CPU_UNION_IMPACT':'0',
    'CELL_CPU_MARKER_REFUTATION':'0','CELL_CPU_SIEVE':'0','CELL_CPU_CONGRUENCE':'0',
    'CELL_CPU_PRESEARCH_SURVEY':'0','CELL_CPU_SEARCH_CONTEXTS':'0'}

def main():
    p=argparse.ArgumentParser();p.add_argument('--compiled',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True);g=p.add_mutually_exclusive_group()
    g.add_argument('--survey',action='store_true');g.add_argument('--shadow',action='store_true')
    p.add_argument('--reuse-control',type=Path,help='Verified same-binary off lane from this experiment')
    p.add_argument('--method',choices=['all','marker'],default='all')
    p.add_argument('--survey-key',choices=['full','projected'],default='projected')
    p.add_argument('--audit',action='store_true');a=p.parse_args()
    out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
    path,program,provenance=load_pipeline_artifact(a.compiled.resolve())
    engine=ROOT/'engine/cell-runtime/target/release/eggfuse-cell-runtime'
    result={'scope':'single-thread standalone Cell CPU; same source_sk_mk and default-on merge programs; not native egglog',
        'program':str(path),'program_sha256':fingerprint(path),'binary_sha256':fingerprint(engine),'provenance':provenance}
    old={k:os.environ.get(k) for k in SETTINGS}
    try:
        os.environ.update(SETTINGS)
        with mode(False):
            if a.survey or a.shadow:
                os.environ['CELL_CPU_PRESEARCH_SURVEY']=('full' if a.survey_key=='full' else '1') if a.survey else '0'
                os.environ['CELL_CPU_SEARCH_CONTEXTS']='observe' if a.shadow else '0'
                r=execute(program,out/'diagnostic',engine)
                totals={}
                for rule in r['presearch_survey']['rules']:
                    for k,w in rule['work'].items():totals.setdefault(k,collections.Counter()).update(w)
                result.update(timing_eligible=False,survey_totals=totals,**{k:r[k] for k in ('presearch_survey','search_contexts','stats')})
                print(json.dumps({'totals':totals,'reuse':r['search_contexts']},indent=2),flush=True)
            else:
                result['lanes']={};states=[];controls=[]
                for name,flag in [('off','0'),('on','marker' if a.method=='marker' else '1')]:
                    os.environ['CELL_CPU_SEARCH_CONTEXTS']=flag
                    data=out/name
                    if name=='off' and a.reuse_control:
                        receipt=json.loads((a.reuse_control.resolve()/'results.json').read_text())
                        assert receipt['exact_endpoint_and_schedule']
                        assert receipt['verification']['exact_graphs_bindings_visibility_and_schedule']
                        data=a.reuse_control.resolve()/'off'
                        m=json.loads((data/'measurement.json').read_text());r=json.loads((data/'report.json').read_text())
                        assert m['program_sha256']==fingerprint(path) and m['binary_sha256']==fingerprint(engine)
                        assert m['configuration']=={k:v for k,v in os.environ.items() if k.startswith('CELL_CPU_')}
                        assert m['audit'] is False
                        import platform
                        assert m['machine']==platform.node()
                        result['control_reused_from']=str(data)
                    else:m,r=timed(path,engine,data)
                    assert r['status']=='complete'
                    assert r['features']['merge_programs']
                    result['lanes'][name]={'measurement':m,**metrics(r),'decoded_rows':r['stats']['decoded_rows'],
                        'merge_candidate_rows':r['multi_stage']['stats']['merge_candidate_rows'],'search_contexts':r['search_contexts']}
                    states.append(json.loads((data/'state.json').read_text()))
                    controls.append([{k:s[k] for k in ('ruleset','iterations','path','epoch','logical_matches','changed')} for s in r['steps']])
                    print(name,m['inference_seconds'],r['search_contexts'],flush=True)
                assert states[0]==states[1] and controls[0]==controls[1]
                result['exact_endpoint_and_schedule']=True
                result['ratio']=result['lanes']['on']['measurement']['inference_seconds']/result['lanes']['off']['measurement']['inference_seconds']
            (out/'results.json').write_text(json.dumps(result,indent=2)+'\n')
            if a.audit:
                feature='CELL_CPU_PRESEARCH_SURVEY' if a.survey else 'CELL_CPU_SEARCH_CONTEXTS'
                result['verification']=audit(path,program,out/'audit',ROOT/'engine/cell-runtime/target/release/examples/project_stages',feature,enabled=('full' if a.survey_key=='full' else '1') if a.survey else ('observe' if a.shadow else ('marker' if a.method=='marker' else '1')))
                (out/'results.json').write_text(json.dumps(result,indent=2)+'\n');print(result['verification'],flush=True)
    finally:
        for k,v in old.items():
            if v is None:os.environ.pop(k,None)
            else:os.environ[k]=v
if __name__=='__main__':main()
