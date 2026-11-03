"""Deferred helper sets and equality blocks in the actual GPU runtime."""
import argparse,json,os,subprocess
from pathlib import Path
from run import run
from compare_runs import compare
from test_joint_polyspine import BASE,SCHEDULE,rejected_transaction
HERE=Path(__file__).resolve().parent

def tests(build,out):
    out.mkdir(parents=True,exist_ok=False);results={}
    cases={
        'merge-enables-parent':BASE+'''(let x (External 1)) (let y (Num 2))
(PotentialExtractedExpr x (TCPair (TermNum 9) 3))
(let t (Add x y))
'''+SCHEDULE+'(check (PotentialExtractedExpr t (TCPair (TermAdd (TermNum 9) (TermNum 2)) 5)))',
        'shared-two-block-inputs':BASE+'''(let x (External 1)) (let y (External 2))
(let a (TCPair (TermNum 4) 4)) (let b (TCPair (TermNum 2) 2))
(let c (TCPair (TermNum 1) 1)) (let left (Smaller a b)) (let right (Smaller left c))
(set (ExtractedExpr x) left) (set (ExtractedExpr y) right)
(let t (Add x y)) (Triple t x y)
'''+SCHEDULE+'(check (PotentialExtractedExpr t (TCPair (TermAdd (TermNum 2) (TermNum 1)) 4)))',
        'repeated-custom-merges':BASE+'''(let x (External 1))
(PotentialExtractedExpr x (TCPair (TermNum 6) 6))
(PotentialExtractedExpr x (TCPair (TermNum 5) 5))
(PotentialExtractedExpr x (TCPair (TermNum 4) 4)) (Add x x)
'''+SCHEDULE,
        'cross-boundary-union':BASE+'''(let x (External 1)) (let h (Hole 0))
(set (ExtractedExpr x) (Smaller h (TCPair (TermNum 2) 2))) (Add x x)
(ruleset outside) (rule ((= h (Hole 0))) ((union h (TCPair (TermNum 1) 1))) :ruleset outside)
'''+SCHEDULE+'(run outside 1)\n'+SCHEDULE,
        'finite-run':BASE+'(Add (Num 1) (Num 2)) (run terms 1) (run terms-helpers 1) (run terms-helpers-helpers 1)',
    }
    for name,source in cases.items():
        file=out/(name+'.egg');file.write_text(source)
        before=run(file,build,out/(name+'-eager'),stage_contexts=True,block_merge=False,stage_context_audit=True)
        after=run(file,build,out/(name+'-blocks'),stage_contexts=True,block_merge=True,stage_context_audit=True)
        check=compare(out/(name+'-eager'),out/(name+'-blocks'),device_states=True,semantic_device=True,saturation_macros=True)
        assert before['checks'][-1]['exact_rows_values_equivalence_visibility'] and after['checks'][-1]['exact_rows_values_equivalence_visibility']
        calls=after['joint_polyspine']['calls'];effects=[c['block_effects'] for c in calls]
        for c in effects:
            assert c['enabled'] and c['set_requests']==c['sets_merged']==c['sets_carried_through_comparison']
            assert c['function_keys_published']<=c['set_requests']
            assert c['union_blocks']==c['publication_waits_removed']
        if name!='finite-run':
            assert calls and all(c['accepted'] for c in calls)
            assert sum(c['producer_audit_rows'] for c in effects)>0
            assert sum(c['union_changes'] for c in effects)>0
        else:assert not calls
        results[name]={'graph':check['final_check'],'native':after['checks'][-1],'blocks':effects}
        print(name,'passed',flush=True)
    for name,text,code in [('overflow',BASE+'(let x (External 1)) (set (ExtractedExpr x) (TCPair (TermNum 1) 9223372036854775807)) (Add x x)'+SCHEDULE,5),('panic',BASE+'(PotentialExtractedExpr (External 1) (TCPair (TermNum 1) -1))'+SCHEDULE,8)]:
        f=out/(name+'.egg');f.write_text(text)
        results[name+'-rollback']=rejected_transaction(build,f,out/(name+'-transaction'),code,stage_contexts=True,block_merge=True)
    binary=out/'merge-probe'
    subprocess.run(['xcrun','swiftc','-O',str(HERE/'test_block_merge.swift'),'-o',str(binary)],check=True)
    raw=subprocess.check_output([str(binary),str(out/'merge-enables-parent-blocks/program.json')],env=dict(os.environ,MTL_DEBUG_LAYER='1'),text=True)
    results['stale-block-union-join']=json.loads(raw)
    assert results['stale-block-union-join']['passed']
    (out/'results.json').write_text(json.dumps(results,indent=2)+'\n');return results

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--build',required=True,type=Path);p.add_argument('--output',required=True,type=Path);a=p.parse_args();r=tests(a.build,a.output);print('passed',len(r),'block effect checks')
