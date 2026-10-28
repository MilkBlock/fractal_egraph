"""Native Analyze -> portable recipe -> new GPU worker, with staged publication.

Builds are supplied; this test runs only tiny real native/GPU workloads once.
It compares complete rows, values, equivalence and visibility, not just cost.
"""
import argparse,copy,hashlib,json,os,subprocess
from pathlib import Path
from run import run
from test_joint_polyspine import BASE,SCHEDULE,rejected_transaction
from tools.cell_spine.executable import compile_library,load,digest


def tests(build,analyzer,out,evidence_from=None,resume=False):
    out.mkdir(parents=True,exist_ok=resume);results={}
    previous=json.loads((out/'results.json').read_text()) if resume and (out/'results.json').exists() else {}
    training=out/'training.egg'
    training.write_text(BASE+'(Add (Num 2) (Num 3))\n'+SCHEDULE)
    if evidence_from is None:
        with (out/'analyze.log').open('w') as log:
            subprocess.run([str(analyzer.resolve()),'analyze','--source',str(training),'--output',str(out/'analysis')],env=dict(os.environ,EGG_LAYOUT_GRAPHS='0'),stdout=log,stderr=subprocess.STDOUT,check=True)
        run(training,build,out/'training-reference',joint_polyspine=True,block_merge=True,check_at_end=True)
        evidence_from=out
    # Native evidence/unchanged reference are reusable. Each case below still
    # starts a new worker and checks its complete graph against native.
    core=json.loads((evidence_from/'training-reference/core.json').read_text())
    analysis=json.loads((evidence_from/'analysis/analysis.json').read_text())
    library=compile_library(core,analysis,training.read_text());artifact=out/'cells.json';artifact.write_text(json.dumps(library,indent=2)+'\n')
    assert {t['anchor'] for t in library['templates']}=={'Num','Add'}
    assert any('no native analyzer' in r['reason'] for r in library['rejected'])
    assert load(artifact,core)==library
    results['analyzer']={'templates':len(library['templates']),'evidence':library['evidence'],'sha256':library['sha256']}
    cases={
        'cold-tree':(BASE+'(let a (Num 10)) (let b (Num 20)) (let x (Add a b)) (Add x a)\n'+SCHEDULE,4),
        # Early Cell completion would let parent Add see a=1 and b=100 in the
        # FIRST terms Run. Native cannot: a has no function until helpers;
        # b has already become 1 by the time terms is scheduled again.
        'stage-visibility':(BASE+'''(let a (Num 1)) (let b (External 2))
(set (ExtractedExpr b) (TCPair (TermNum 99) 100))
(PotentialExtractedExpr b (TCPair (TermNum 2) 1))
(let t (Add a b))
'''+SCHEDULE+'(check (PotentialExtractedExpr t (TCPair (TermAdd (TermNum 1) (TermNum 2)) 3)))',2),
        'existing-function':(BASE+'''(let a (Num 1)) (let b (Num 2)) (let t (Add a b))
(set (ExtractedExpr t) (TCPair (TermNum 999) 0))
'''+SCHEDULE,2),
        'multiple-spines':(BASE+'''(let a (Num 1)) (let b (Num 2))
(union (Add a b) (Add b a))
'''+SCHEDULE,2),
        'multiple-cost-witnesses':(BASE+'''(let a (External 1)) (let p (TCPair (TermNum 10) 1))
(union p (TCPair (TermNum 11) 3)) (set (ExtractedExpr a) p) (Add a a)
'''+SCHEDULE,0),
        'large-sentinel':(BASE+'''(let a (External 1)) (let b (External 2))
(set (ExtractedExpr a) (TCPair (TermNum 10) 10000000000000000))
(set (ExtractedExpr b) (TCPair (TermNum 20) 1)) (Add a b)
'''+SCHEDULE,0),
        'unresolved-input':(BASE+'(Add (External 1) (Num 2))\n'+SCHEDULE,1),
        'unobserved-spine':(BASE+'(Triple (Num 1) (Num 2) (Num 3))\n'+SCHEDULE,3),
        'finite-runs':(BASE+'(Add (Num 1) (Num 2)) (run terms 1) (run terms-helpers 1) (run terms-helpers-helpers 1)',0),
        'late-union':(BASE+'''(let a (Num 1)) (let b (Num 2)) (Add a b)
(ruleset outside) (rule ((= a (Num 1)) (= b (Num 2))) ((union a b)) :ruleset outside)
'''+SCHEDULE+'(run outside 1)\n'+SCHEDULE,3),
        'alias-invalidates-recipe':(BASE+'''(let a (Num 1))
(let old (TCPair (TermNum 1) 1))
(union old (Smaller (TCPair (TermNum 9) 9) (TCPair (TermNum 0) 0)))
(Add a a)
'''+SCHEDULE,1),
        'finite-entry-sentinel':(BASE+'(Add (Num 1) (Num 2)) (run terms 1) (run terms-helpers 1)\n'+SCHEDULE,0),
    }
    for name,(source,expected) in cases.items():
        file=out/(name+'.egg');file.write_text(source)
        report_path=out/name/'report.json'
        if resume and report_path.exists():
            report=json.loads(report_path.read_text())
            sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
            assert report['source_sha256']==sha(file) and report['cell_program']['artifact_sha256']==library['sha256']
            assert report['build']['worker_sha256']==sha(build/'worker')
            assert report['build']['frontend_sha256']==sha(build/'frontend')
            runtime=lambda p:Path(p).suffix!='.rs'
            assert all(sha(Path(p))==h for p,h in report['build']['sources'].items() if runtime(p)),'cached backend source changed'
        else:
            report=run(file,build,out/name,cell_program=artifact,stage_contexts=True,stage_context_audit=True,check_at_end=True)
        calls=report['joint_polyspine']['calls'];assert all(c['accepted'] for c in calls),(name,calls)
        assert report['checks'][-1]['exact_rows_values_equivalence_visibility']
        hits=sum(c['cell_program']['hits'] for c in calls)
        assert hits>=expected,(name,hits,expected)
        if name in ['multiple-cost-witnesses','large-sentinel','finite-runs','finite-entry-sentinel']:assert hits==0,(name,hits)
        if expected:
            assert sum(c['cell_program']['groups_skipped'] for c in calls)>0
            if name!='alias-invalidates-recipe':assert sum(c['cell_program']['comparison_rows_skipped'] for c in calls)>0
        if name=='finite-runs':assert not calls
        if name=='alias-invalidates-recipe':assert sum(c['cell_program']['invalidations'] for c in calls)>0
        results[name]={'native':report['checks'][-1],'recipes':[c['cell_program'] for c in calls],'phases':[c['private_phases'] for c in calls]}
        print(name,results[name],flush=True)
    for name,code,source in [
        ('overflow',5,BASE+'(Num 2) (let x (External 1)) (set (ExtractedExpr x) (TCPair (TermNum 1) 9223372036854775807)) (Add x x)'+SCHEDULE),
        ('negative-cost',8,BASE+'(Num 2) (PotentialExtractedExpr (External 1) (TCPair (TermNum 1) -1))'+SCHEDULE),
    ]:
        file=out/(name+'.egg');unchanged=file.exists() and file.read_text()==source;file.write_text(source)
        if resume and unchanged and name+'-rollback' in previous:
            results[name+'-rollback']=previous[name+'-rollback']
        else:
            results[name+'-rollback']=rejected_transaction(build,file,out/(name+'-rollback'),code,stage_contexts=True,block_merge=True,cell_program=artifact)
    for name,mutate in [('corrupt-hash',lambda x:x['templates'][0].update(slots=1)),('forged-recipe',lambda x:x['templates'][0]['stages'][0]['operations'].clear())]:
        bad=copy.deepcopy(library);mutate(bad)
        if name=='forged-recipe':bad['sha256']=digest({k:v for k,v in bad.items() if k!='sha256'})
        file=out/(name+'.json');file.write_text(json.dumps(bad))
        try:load(file,core)
        except ValueError:results[name]=True
        else:raise AssertionError(name)
    changed=copy.deepcopy(core);next(s for s in changed['rulesets'] if s['name']=='terms')['rules'][0]['head']=[]
    try:load(artifact,changed)
    except ValueError:results['changed-environment']=True
    else:raise AssertionError('changed environment accepted')
    (out/'results.json').write_text(json.dumps(results,indent=2)+'\n');return results

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--build',type=Path,required=True);p.add_argument('--analyzer',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--evidence-from',type=Path);p.add_argument('--resume',action='store_true');a=p.parse_args()
    print('passed',len(tests(a.build,a.analyzer,a.output,a.evidence_from,a.resume)),'executable Cell checks')
