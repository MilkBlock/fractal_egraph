"""One clean same-binary local-saturation pair; streaming semantic-boundary audit."""
import argparse,json,os,subprocess
from pathlib import Path
from eggfuse.engine.cell_runtime import load_pipeline_artifact,prepare
from eggfuse.engine.cpu import compare_native
from eggfuse.ir.cell_runtime import build
from experiments.cell_runtime.presearch_bench import SETTINGS,ROOT
from experiments.cell_runtime.compositions_bench import mode,metrics
from experiments.cell_runtime.suite import timed,fingerprint

def audit(path,program,out):
    out.mkdir();binary=ROOT/'engine/cell-runtime/target/release/examples/project_semantics'
    logs=[(out/(str(i)+'.log')).open('w') for i in (0,1)];procs=[];count=0;summaries=[]
    try:
        for i,log in enumerate(logs):
            procs.append(subprocess.Popen([str(binary),str(path)],stdout=subprocess.PIPE,stderr=log,text=True,
                env={**os.environ,'CELL_CPU_LOCAL_SAT':str(i),'CELL_CPU_LOCAL_SAT_AUDIT':'1'}))
        while True:
            lines=[p.stdout.readline() for p in procs]
            if not any(lines):break
            assert all(lines),'semantic boundary count differs'
            a,b=map(json.loads,lines)
            if a['kind']=='summary':assert b['kind']=='summary';summaries=[a,b];continue
            assert {k:v for k,v in a.items() if k!='state'}=={k:v for k,v in b.items() if k!='state'}
            compare_native(program['source_core'],b['state'],a['state']);count+=1
        assert all(p.wait()==0 for p in procs),'semantic streamer failed'
    finally:
        for p in procs:
            if p.poll() is None:p.kill()
            p.wait()
        for f in logs:f.close()
    assert summaries
    return {'boundaries':count,'all_rows_values_equivalence_visibility':True,'source_outer_sat_and_finite_run_boundaries':True,
        'per_local_binding_original_lhs_checked':True,'streamer_sha256':fingerprint(binary),'summaries':summaries}

def main():
    p=argparse.ArgumentParser();g=p.add_mutually_exclusive_group(required=True)
    g.add_argument('--compiled',type=Path);g.add_argument('--demo-chain',type=int)
    p.add_argument('--instances',type=int,default=128);p.add_argument('--frontend',type=Path)
    p.add_argument('--feature',choices=['local-sat','retained'],default='local-sat')
    p.add_argument('--context-sets',default='always-run,type-analysis')
    p.add_argument('--output',type=Path,required=True);p.add_argument('--audit',action='store_true');a=p.parse_args()
    out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
    if a.compiled:
        path,program,provenance=load_pipeline_artifact(a.compiled.resolve());scope='authenticated source_sk_mk pipeline'
    else:
        assert a.demo_chain>=2 and a.instances>0
        declarations='(datatype E (N i64) (Seed E) (Known E) '+ ' '.join(f'(S{i} E)' for i in range(a.demo_chain))+') (ruleset s)\n'
        rules='(rule ((Seed x)) ((Known x) (S0 x)) :ruleset s)\n'+''.join(f'(rule ((S{i} x) (Known x)) ((S{i+1} x)) :ruleset s)\n' for i in range(a.demo_chain-1))
        source=declarations+rules+''.join(f'(Seed (N {i}))\n' for i in range(a.instances))+'(run-schedule (saturate s))\n'
        src=out/'source.egg';src.write_text(source);program=build(prepare(src,a.frontend));path=out/'program.json';path.write_text(json.dumps(program))
        provenance={'source_sha256':fingerprint(src)};scope='synthetic shared-context chain through native typed frontend; not eggcc performance'
    engine=ROOT/'engine/cell-runtime/target/release/eggfuse-cell-runtime'
    feature='CELL_CPU_LOCAL_SAT' if a.feature=='local-sat' else 'CELL_CPU_RETAINED_CONTEXTS'
    settings={**SETTINGS,'CELL_CPU_LOCAL_SAT':'0','CELL_CPU_LOCAL_SAT_AUDIT':'0','CELL_CPU_REFUTATION_PROFILE':'0',
        'CELL_CPU_RETAINED_CONTEXTS':'0','CELL_CPU_RETAINED_SETS':a.context_sets}
    old={k:os.environ.get(k) for k in settings}
    result={'scope':scope+'; standalone single-thread Cell CPU, merge programs ON','program':str(path),
        'program_sha256':fingerprint(path),'binary_sha256':fingerprint(engine),
        'feature':a.feature,'provenance':provenance,'lanes':{}}
    try:
        os.environ.update(settings)
        with mode(False):
            states=[]
            for name,flag in [('off','0'),('on','1')]:
                os.environ[feature]=flag;m,r=timed(path,engine,out/name)
                states.append(json.loads((out/name/'state.json').read_text()))
                result['lanes'][name]={'measurement':m,**metrics(r),'local_saturation':r['local_saturation'],
                    'retained_contexts':r['retained_contexts'],
                    'runs_and_repair_calls':len(r['steps']),'changed_runs':sum(s['changed'] for s in r['steps']),
                    'point_reads':r['multi_stage']['stats']['point_reads'],
                    'inverse_reads':r['multi_stage']['stats']['inverse_reads'],
                    'merge_port_reads':r['multi_stage']['stats']['merge_port_reads'],
                    'decoded_rows':r['stats']['decoded_rows'],
                    'candidate_scope':'ordinary delta/residual candidates plus merge-program candidates; fixed point reads reported separately',
                    'total_candidates':r['stats']['residual_candidates']+r['multi_stage']['stats']['merge_candidate_rows']}
                stats=r['local_saturation' if a.feature=='local-sat' else 'retained_contexts']['stats']
                print(name,m['inference_seconds'],len(r['steps']),{k:v for k,v in stats.items() if not isinstance(v,dict)},flush=True)
            result['endpoint']=compare_native(program['source_core'],states[1],states[0])
            result['ratio']=result['lanes']['on']['measurement']['inference_seconds']/result['lanes']['off']['measurement']['inference_seconds']
            (out/'results.json').write_text(json.dumps(result,indent=2)+'\n')
            if a.audit:
                if a.feature=='retained':
                    from experiments.cell_runtime.refutation_bench import audit as stage_audit
                    result['verification']=stage_audit(path,program,out/'audit',ROOT/'engine/cell-runtime/target/release/examples/project_stages',feature)
                else:result['verification']=audit(path,program,out/'audit')
                # Fresh process reproduces actual identities, not only isomorphism.
                os.environ[feature]='1'
                subprocess.run([str(engine),'--program',str(path),'--output',str(out/'replay')],check=True,capture_output=True)
                assert states[1]==json.loads((out/'replay/state.json').read_text())
                result['fresh_process_exact_replay']=True
                (out/'results.json').write_text(json.dumps(result,indent=2)+'\n');print('verified',result['verification'].get('boundaries',result['verification'].get('snapshots')),flush=True)
    finally:
        for k,v in old.items():
            if v is None:os.environ.pop(k,None)
            else:os.environ[k]=v
if __name__=='__main__':main()
