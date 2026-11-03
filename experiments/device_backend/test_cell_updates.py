"""Real Pass-3 rule environment: portable multi-candidate update instructions.

Only input facts/schedules are changed for adversarial fixtures. The source
rules/declarations are copied verbatim from the saved production Pass-3 file.
"""
import argparse,copy,hashlib,json,os,subprocess
from pathlib import Path
from run import run
from eggfuse.compiler.targets.metal.compiler import compile_program
from compare_runs import compare
from test_joint_polyspine import SCHEDULE,rejected_transaction
from eggfuse.compiler.recipes.executable import compile_library,load,digest

TRAIN='''(let a (Const (Int 1) (Base (IntT)) (InFunc "cell-training")))
(let b (Arg (Base (IntT)) (InFunc "cell-training")))
(let c (Bop (Add) a b))
(Top (Select) a b c) (Uop (Neg) c)
(let t (Concat (Single a) (Single b))) (Get t 0)
(set (ExtractedExpr c) (TCPair (TermArg) 99))
(PotentialExtractedExpr c (TCPair (TermConst (Int 5)) 4))
'''
ROOT='(let root (Empty (Base (IntT)) (InFunc "update-held-out")))\n'

def tests(build,analyzer,source,out,evidence_from=None,spines=False):
    out.mkdir(parents=True,exist_ok=False);text=source.read_text();base=text[:text.index('(ruleset initialization)')]
    training=out/'training.egg';training.write_text(base+TRAIN+SCHEDULE)
    if evidence_from is None:
        with (out/'analyze.log').open('w') as log:
            subprocess.run([str(analyzer.resolve()),'analyze','--source',str(training),'--output',str(out/'analysis')],check=True,stdout=log,stderr=subprocess.STDOUT,env=dict(os.environ,EGG_LAYOUT_GRAPHS='0'))
        run(training,build,out/'training-reference',stage_contexts=True,check_at_end=True)
        analysis=out/'analysis/analysis.json';core=out/'training-reference/core.json'
    else:
        assert training.read_text()==(evidence_from/'training.egg').read_text()
        analysis=evidence_from/'analysis-v2/analysis.json';core=evidence_from/'training-reference/core.json'
    core=json.loads(core.read_text());analysis=json.loads(analysis.read_text())
    lib=compile_library(core,analysis,training.read_text(),updates_only=True,spines=spines)
    assert lib['update_evidence']['existing_value_requests']>0 and not lib['templates']
    artifact=out/'updates.json';artifact.write_text(json.dumps(lib,indent=2)+'\n')
    results={'provenance':{'pass3_source_sha256':hashlib.sha256(text.encode()).hexdigest(),'artifact_sha256':lib['sha256'],
                         'observed_old_value_requests':lib['update_evidence']['existing_value_requests']}}
    cases={
        'two-improvements':ROOT+'''(set (ExtractedExpr root) (TCPair (TermConst (Int 10)) 10))
(PotentialExtractedExpr root (TCPair (TermConst (Int 7)) 7))
(PotentialExtractedExpr root (TCPair (TermConst (Int 5)) 5))
'''+SCHEDULE,
        'correlated-bags':ROOT+'''(let old (TCPair (TermConst (Int 10)) 10)) (union old (TCPair (TermConst (Int 12)) 12))
(let new (TCPair (TermConst (Int 3)) 3)) (union new (TCPair (TermConst (Int 9)) 9))
(set (ExtractedExpr root) old) (PotentialExtractedExpr root new) (Bop (Add) root root)
'''+SCHEDULE,
        'late-improvements':ROOT+'''(set (ExtractedExpr root) (TCPair (TermConst (Int 10)) 10))
(PotentialExtractedExpr root (TCPair (TermConst (Int 7)) 7))
(ruleset later) (rule ((= e (Empty ty ctx))) ((PotentialExtractedExpr e (TCPair (TermConst (Int 5)) 5))) :ruleset later)
'''+SCHEDULE+'(run later 1)\n'+SCHEDULE,
        'comparison-enables-update':ROOT+'''(set (ExtractedExpr root) (Smaller (TCPair (TermConst (Int 10)) 10) (TCPair (TermConst (Int 7)) 7)))
(PotentialExtractedExpr root (TCPair (TermConst (Int 5)) 5))
'''+SCHEDULE,
        'multiple-spines':'''(let a (Const (Int 1) (Base (IntT)) (InFunc "held-out")))
(let b (Arg (Base (IntT)) (InFunc "held-out")))
(let root (Bop (Add) a b)) (union root (Uop (Neg) a))
(set (ExtractedExpr root) (TCPair (TermConst (Int 99)) 99))
'''+SCHEDULE,
        'large-costs':ROOT+'''(set (ExtractedExpr root) (TCPair (TermConst (Int 99)) 9223372036854775807))
(PotentialExtractedExpr root (TCPair (TermConst (Int 9)) 10000000000000001))
(PotentialExtractedExpr root (TCPair (TermConst (Int 8)) 9999999999999999))
'''+SCHEDULE,
        'late-cost-alias':ROOT+'''(let old (TCPair (TermConst (Int 10)) 10)) (set (ExtractedExpr root) old)
(PotentialExtractedExpr root (TCPair (TermConst (Int 5)) 5))
(ruleset later) (rule ((= p (TCPair t c)) (= c 5)) ((union p (TCPair (TermConst (Int 1)) 1))) :ruleset later)
'''+SCHEDULE+'(run later 1)\n'+SCHEDULE,
        'finite-entry-sentinel':'''(let a (Const (Int 1) (Base (IntT)) (InFunc "finite-entry")))
(let b (Arg (Base (IntT)) (InFunc "finite-entry"))) (Bop (Add) a b)
(run terms 1) (run terms-helpers 1)
'''+SCHEDULE,
        'finite-run':ROOT+'''(set (ExtractedExpr root) (TCPair (TermConst (Int 10)) 10))
(PotentialExtractedExpr root (TCPair (TermConst (Int 5)) 5))
(run terms 1) (run terms-helpers 1) (run terms-helpers-helpers 1)
''',
    }
    for name,facts in cases.items():
        file=out/(name+'.egg');file.write_text(base+facts)
        report=run(file,build,out/name,cell_program=artifact,stage_contexts=True,stage_context_audit=True,check_at_end=True)
        calls=report['joint_polyspine']['calls'];assert report['checks'][-1]['exact_rows_values_equivalence_visibility']
        assert all(c['accepted'] and c['statistics'][49]==0 for c in calls),'original helper matcher executed'
        if spines:assert all(c['statistics'][48]==0 for c in calls),'covered terms used original binder'
        if name=='finite-run':assert not calls
        else:
            assert sum(c['cell_updates']['actions'] for c in calls)>0
            assert sum(c['cell_updates']['existing_result_actions'] for c in calls)>0
        if name=='correlated-bags':assert sum(c['cell_updates']['multiple_witness_actions'] for c in calls)>0
        results[name]={'native':report['checks'][-1],'updates':[c['cell_updates'] for c in calls],
                       'original_helper_bindings':sum(c['statistics'][49] for c in calls),'original_terms_bindings':sum(c['statistics'][48] for c in calls),'spines':[c['spine_program'] for c in calls]}
        print(name,results[name],flush=True)
    # Fresh and existing-owner programs can coexist without double execution.
    combined=compile_library(core,analysis,training.read_text(),updates=True)
    full=out/'combined.json';full.write_text(json.dumps(combined,indent=2)+'\n')
    mixed=run(out/'multiple-spines.egg',build,out/'mixed-programs',cell_program=full,stage_contexts=True,stage_context_audit=True,check_at_end=True)
    assert mixed['checks'][-1]['exact_rows_values_equivalence_visibility']
    assert sum(c['cell_program']['hits'] for c in mixed['joint_polyspine']['calls'])>0
    assert sum(c['cell_updates']['actions'] for c in mixed['joint_polyspine']['calls'])>0
    results['mixed-programs']={'native':mixed['checks'][-1]}
    for name,body,code in [
        ('negative',ROOT+'(PotentialExtractedExpr root (TCPair (TermArg) -1))',8),
        ('overflow',ROOT+'(set (ExtractedExpr root) (TCPair (TermArg) 9223372036854775807)) (Bop (Add) root root)',5),
    ]:
        file=out/(name+'.egg');file.write_text(base+body+SCHEDULE)
        results[name+'-rollback']=rejected_transaction(build,file,out/(name+'-rollback'),code,stage_contexts=True,block_merge=True,cell_program=artifact)
    # A forged yet rehashed program must fail typed validation, not execute.
    bad=copy.deepcopy(lib);bad['update_program']['stages'][1]['operations']=[];bad['sha256']=digest({k:v for k,v in bad.items() if k!='sha256'})
    file=out/'forged.json';file.write_text(json.dumps(bad))
    try:load(file,core)
    except ValueError:results['forged-update-rejected']=True
    else:raise AssertionError('forged update accepted')
    missing=copy.deepcopy(lib);missing['update_evidence']={};missing['sha256']=digest({k:v for k,v in missing.items() if k!='sha256'})
    file=out/'missing-evidence.json';file.write_text(json.dumps(missing))
    try:load(file,core)
    except ValueError as e:assert 'old-value evidence' in str(e);results['missing-evidence-rejected']=True
    else:raise AssertionError('missing update evidence accepted')
    for config in [{},{'joint_polyspine':True},{'block_merge':True}]:
        try:compile_program(core,cell_program=lib,**config)
        except ValueError as e:assert 'joint execution and block merge' in str(e)
        else:raise AssertionError('invalid update execution mode')
    results['invalid-configuration-rejected']=True
    (out/'results.json').write_text(json.dumps(results,indent=2)+'\n');return results

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--build',type=Path,required=True);p.add_argument('--analyzer',type=Path,required=True);p.add_argument('--source',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--evidence-from',type=Path);p.add_argument('--spines',action='store_true');a=p.parse_args()
    print('passed',len(tests(a.build,a.analyzer,a.source,a.output,a.evidence_from,a.spines)),'update checks')
