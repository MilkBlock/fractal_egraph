"""Single-run off/on receipt ablation; existing pipeline, timer and metrics."""
import argparse, json, os, subprocess
from pathlib import Path
from eggfuse.engine.cell_runtime import load_pipeline_artifact
from experiments.cell_runtime.suite import timed, fingerprint
from experiments.cell_runtime.compositions_bench import mode, metrics
ROOT=Path(__file__).resolve().parents[2]

def audit(path, program, output, streamer, feature='CELL_CPU_MARKER_REFUTATION'):
    output.mkdir(); tables=output/'tables.json'
    tables.write_text(json.dumps([t['name'] for t in program['source_core']['tables']]))
    logs=[(output/(m+'.log')).open('w') for m in ('off','on')]
    procs=[]; checked=0
    try:
        for value,log in zip(('0','1'),logs):
            procs.append(subprocess.Popen([str(streamer),str(path),str(tables),'--all-stages','--bindings'],
                env={**os.environ,feature:value,'CELL_CPU_SIEVE':'0'},
                stdout=subprocess.PIPE,stderr=log,text=True))
        while True:
            lines=[p.stdout.readline() for p in procs]
            if not any(lines):break
            assert all(lines),'stage count differs'
            a,b=map(json.loads,lines)
            assert a['state']==b['state'],f'full graph differs at {checked}'
            assert a['bindings']==b['bindings'],f'full bindings differ at {checked}'
            keys=('ruleset','iterations','path','epoch','logical_matches','changed')
            if a['step'] is None:assert b['step'] is None
            else:assert all(a['step'][k]==b['step'][k] for k in keys)
            checked+=1
        assert all(p.wait()==0 for p in procs),'streamer failed'
    finally:
        for p in procs:
            if p.poll() is None:p.kill()
            p.wait()
        for f in logs:f.close()
    return {'snapshots':checked,'all_tables_including_markers':True,
        'exact_graphs_bindings_visibility_and_schedule':True,'streamer_sha256':fingerprint(streamer)}

def main():
    p=argparse.ArgumentParser();p.add_argument('--pipeline',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    p.add_argument('--engine',type=Path,default=ROOT/'engine/cell-runtime/target/release/eggfuse-cell-runtime')
    p.add_argument('--scope',choices=['marker','multistage'],default='marker')
    p.add_argument('--audit',action='store_true');a=p.parse_args();a.output.mkdir(parents=True,exist_ok=False)
    path,program,provenance=load_pipeline_artifact(a.pipeline.resolve());lanes={};states=[];schedules=[]
    old=os.environ.get('CELL_CPU_MARKER_REFUTATION');old_sieve=os.environ.get('CELL_CPU_SIEVE');old_scope=os.environ.get('CELL_CPU_REFUTATION_SCOPE')
    try:
        with mode(False):
            os.environ['CELL_CPU_SIEVE']='0'
            os.environ['CELL_CPU_REFUTATION_SCOPE']=a.scope
            for name,value in [('off','0'),('on','1')]:
                os.environ['CELL_CPU_MARKER_REFUTATION']=value
                m,r=timed(path,a.engine,a.output/name)
                states.append(json.loads((a.output/name/'state.json').read_text()))
                schedules.append([{k:s[k] for k in ('ruleset','iterations','path','epoch','logical_matches','changed')} for s in r['steps']])
                lanes[name]={'measurement':m,**metrics(r),'refutation':r['marker_refutation']}
                print(name,m['inference_seconds'],r['marker_refutation'],flush=True)
            assert states[0]==states[1] and schedules[0]==schedules[1]
            result={'scope':'same-binary single-thread Cell CPU; online recipe refutation, not static Bake closure','refutation_scope':a.scope,
                'program':str(path),'provenance':provenance,'lanes':lanes,'exact_endpoint_and_schedule':True,
                'ratio':lanes['on']['measurement']['inference_seconds']/lanes['off']['measurement']['inference_seconds']}
            (a.output/'results.json').write_text(json.dumps(result,indent=2)+'\n')
            if a.audit:
                result['verification']=audit(path,program,a.output/'audit',ROOT/'engine/cell-runtime/target/release/examples/project_stages')
                (a.output/'results.json').write_text(json.dumps(result,indent=2)+'\n')
                print(result['verification'],flush=True)
    finally:
        for k,v in [('CELL_CPU_MARKER_REFUTATION',old),('CELL_CPU_SIEVE',old_sieve),('CELL_CPU_REFUTATION_SCOPE',old_scope)]:
            if v is None:os.environ.pop(k,None)
            else:os.environ[k]=v
if __name__=='__main__':main()
