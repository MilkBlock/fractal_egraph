"""Loaded port recipes: all eight real seeds, partial coverage and corruption."""
import argparse,copy,json
from pathlib import Path
from run import run
from tools.cell_spine.executable import compile_library,digest,load

def tests(build,evidence,out):
    out.mkdir(parents=True,exist_ok=False)
    core=json.loads((evidence/'training-reference/core.json').read_text());analysis=json.loads((evidence/'analysis-v2/analysis.json').read_text());source=evidence/'training.egg'
    artifact=compile_library(core,analysis,source.read_text(),spines=True)
    recipes=artifact['spine_program']['recipes'];assert len(recipes)==8 and not artifact['templates']
    results={}
    for name,partial in [('all',False),('missing-top',True)]:
        lib=copy.deepcopy(artifact)
        if partial:lib['spine_program']['recipes']=[r for r in recipes if r['anchor']!='Top'];lib['sha256']=digest({k:v for k,v in lib.items() if k!='sha256'})
        file=out/(name+'.json');file.write_text(json.dumps(lib,indent=2)+'\n');load(file,core)
        report=run(source,build,out/name,cell_program=file,stage_contexts=True,stage_context_audit=True,check_at_end=True)
        calls=report['joint_polyspine']['calls'];assert report['checks'][-1]['exact_rows_values_equivalence_visibility'];assert all(c['accepted'] for c in calls)
        generic=sum(c['statistics'][48] for c in calls);direct=sum(c['spine_program']['actions'] for c in calls);fallback=sum(c['spine_program']['fallback_actions'] for c in calls)
        assert direct>0 and all(c['statistics'][49]==0 for c in calls)
        if partial:assert generic>0 and fallback>0
        else:assert generic==fallback==0
        results[name]={'native':report['checks'][-1],'generic_terms_bindings':generic,'direct_actions':direct,'fallback_actions':fallback}
        print(name,results[name],flush=True)
    for name,mutate in [
        ('wire',lambda r:r['parameters'][0].update(column=99)),
        ('arithmetic',lambda r:r['numeric_program']['coefficients'].__setitem__(3,987)),
        ('effects',lambda r:r['operations'].clear()),
        ('evidence',lambda r:r['evidence'].update(applications=0)),
    ]:
        bad=copy.deepcopy(artifact);mutate(bad['spine_program']['recipes'][0]);bad['sha256']=digest({k:v for k,v in bad.items() if k!='sha256'})
        file=out/(name+'.json');file.write_text(json.dumps(bad))
        try:load(file,core)
        except ValueError:results[name+'-rejected']=True
        else:raise AssertionError(name)
    bad=copy.deepcopy(artifact);bad['spine_program']['recipes']*=2;bad['sha256']=digest({k:v for k,v in bad.items() if k!='sha256'});file=out/'duplicate.json';file.write_text(json.dumps(bad))
    try:load(file,core)
    except ValueError:results['duplicate-rejected']=True
    else:raise AssertionError('duplicate')
    (out/'results.json').write_text(json.dumps(results,indent=2)+'\n');return results

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--build',type=Path,required=True);p.add_argument('--evidence',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    print('passed',len(tests(a.build,a.evidence,a.output)),'boundary recipe checks')
