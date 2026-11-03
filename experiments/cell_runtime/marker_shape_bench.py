"""Three-way pinned marker shape experiment using the existing timer/comparator."""
import argparse,collections,json,os,subprocess
from pathlib import Path
from eggfuse.engine.cell_runtime import load_pipeline_artifact
from eggfuse.engine.cpu import compare_native
from experiments.cell_runtime.suite import timed,fingerprint
from experiments.cell_runtime.compositions_bench import mode,metrics
from .marker_shape import NEW

ROOT=Path(__file__).resolve().parents[2]

def audit(a,b,core,tables,out,streamer):
    out.mkdir(parents=True,exist_ok=False)
    table_file=out/'tables.json';table_file.write_text(json.dumps(sorted(tables)))
    logs=[(out/(n+'.log')).open('w') for n in ('baseline','candidate')]
    procs=[subprocess.Popen([str(streamer),str(p),str(table_file)],stdout=subprocess.PIPE,stderr=log,text=True) for p,log in zip((a,b),logs)]
    checked=0
    try:
        while True:
            lines=[p.stdout.readline() for p in procs]
            if not any(lines):break
            if not all(lines):raise AssertionError('original-stage counts differ')
            left,right=map(json.loads,lines)
            try:
                if left['step'] is None or right['step'] is None:assert left['step']==right['step']
                else:
                    assert left['step']['ruleset']==right['step']['ruleset'],('ruleset',left['step']['ruleset'],right['step']['ruleset'])
                    assert left['step']['iterations']==right['step']['iterations'],('iteration',left['step']['iterations'],right['step']['iterations'])
                compare_native(core,right['state'],left['state'])
            except Exception:
                (out/'baseline-failure.json').write_text(json.dumps(left));(out/'candidate-failure.json').write_text(json.dumps(right));raise
            checked+=1
            if checked%100==0:print('checked',checked,'original-stage snapshots',flush=True)
        for p in procs:
            if p.wait()!=0:raise RuntimeError('stage exporter failed')
        result={'original_stage_snapshots_checked':checked,'exact_rows_values_equivalence_visibility':True,
                'streamer_sha256':fingerprint(streamer),'baseline_program_sha256':fingerprint(a),'candidate_program_sha256':fingerprint(b)}
    finally:
        for p in procs:
            if p.poll() is None:p.kill()
            p.wait()
        for log in logs:log.close()
    (out/'result.json').write_text(json.dumps(result,indent=2));return result

def main():
    p=argparse.ArgumentParser();p.add_argument('--root',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    p.add_argument('--engine',type=Path,default=ROOT/'engine/cell-runtime/target/release/eggfuse-cell-runtime')
    p.add_argument('--streamer',type=Path,default=ROOT/'engine/cell-runtime/target/release/examples/project_stages')
    p.add_argument('--timing-only',action='store_true',help='exploratory endpoint-checked timings; not full acceptance')
    args=p.parse_args();args.output.mkdir(parents=True,exist_ok=False)
    programs={};proof={}
    for name in ('baseline','remove','if'):
        path,program,provenance=load_pipeline_artifact(args.root/(name+'-compile'));programs[name]=(path,program)
        proof[name]=provenance
    base=programs['baseline'][1];base_core=base['source_core']
    auxiliary=set(base['reference_views']['marker_tables'])|{t['name'] for t in base['recipe']['tables'] if t['name'].startswith('CellSpineApplied')}|{NEW}
    tables={t['name'] for t in base_core['tables']}-auxiliary
    schemas={t['name']:t['schema'] for t in base_core['tables'] if t['name'] in tables}
    for _,program in programs.values():
        assert {t['name'] for t in program['source_core']['tables']}-auxiliary==tables,'original table inventory changed'
        assert {t['name']:t['schema'] for t in program['source_core']['tables'] if t['name'] in tables}==schemas
        assert program['recipe']['control']==base['recipe']['control'],'control changed'
    result={'scope':'same-binary standalone Cell CPU, source marker shape ablation; not pristine egglog or GPU',
        'provenance':proof,'audits':{},'lanes':{},'compositions_enabled':False,'general_solver_modified':False}
    with mode(False):
        if not args.timing_only:
            for name in ('remove','if'):
                result['audits'][name]=audit(programs['baseline'][0],programs[name][0],base_core,tables,args.output/(name+'-audit'),args.streamer)
        expected=None
        for name,(path,_) in programs.items():
            measurement,report=timed(path,args.engine,args.output/name)
            state=json.loads((args.output/name/'state.json').read_text())
            counts=collections.Counter(r['table'] for r in state['rows'] if not r['subsumed'])
            state['rows']=[r for r in state['rows'] if r['table'] in tables]
            if expected is None:expected=state
            else:compare_native(base_core,state,expected)
            helper=collections.Counter()
            for step in report['steps']:
                if step['ruleset']=='cell-spine-complete':helper.update(step['phases'])
            result['lanes'][name]={'measurement':measurement,**metrics(report),'visible_auxiliary_rows':{t:counts[t] for t in sorted(auxiliary) if counts[t]},'completion_phase_seconds':dict(helper)}
            print(name,measurement['inference_seconds'],flush=True)
            (args.output/'results.json').write_text(json.dumps(result,indent=2)+'\n')
    result['ratios']={name:result['lanes'][name]['measurement']['inference_seconds']/result['lanes']['baseline']['measurement']['inference_seconds'] for name in ('remove','if')}
    result['if_vs_remove']=result['lanes']['if']['measurement']['inference_seconds']/result['lanes']['remove']['measurement']['inference_seconds']
    (args.output/'results.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result['ratios']))
if __name__=='__main__':main()
