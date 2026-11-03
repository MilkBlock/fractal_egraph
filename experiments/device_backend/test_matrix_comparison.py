"""Exact matrix guards and private union closure versus independent native."""
import argparse,json,random
from pathlib import Path
from run import run
from compare_runs import compare

SCHEMA='''(datatype Term (V i64))
(datatype C (Cost Term i64) (Choose C C) (Hole i64))
(ruleset cmp)
(rule ((= lhs (Choose (Cost a ca) (Cost b cb))) (< ca cb)) ((union lhs (Cost a ca))) :ruleset cmp)
(rule ((= lhs (Choose (Cost a ca) (Cost b cb))) (> ca cb)) ((union lhs (Cost b cb))) :ruleset cmp)
(rule ((= lhs (Choose (Cost a ca) (Cost b ca)))) ((union lhs (Cost a ca))) :ruleset cmp)
'''
SAT='(run-schedule (saturate (run cmp)))\n'

def tests(build,out):
    out.mkdir(parents=True,exist_ok=False);results={}
    def pair(name,source,admitted=True,accepted=True):
        file=out/(name+'.egg');file.write_text(source);paths=[out/(name+'-normal'),out/(name+'-matrix')]
        reports=[run(file,build,path,matrix_saturate=mode) for path,mode in zip(paths,[False,True])]
        r=reports[1];calls=[c for c in r['matrix_saturate']['calls'] if c.get('kind')=='comparison_saturate']
        assert bool(r['matrix_saturate']['comparison_programs'])==admitted,name
        assert any(c['accepted'] for c in calls)==accepted,name
        result=compare(*paths,device_states=True,semantic_device=True,saturation_macros=True)
        result['native_checks']=[x['checks'][-1] for x in reports];result['calls']=calls;results[name]=result
        print(name,[(c['accepted'],c['local_iterations'],c['local_unions']) for c in calls],flush=True)
        return r
    pair('recursive-choices',SCHEMA+'''(let a (Cost (V 1) 1)) (let b (Cost (V 2) 3))
(let c (Choose a b)) (Choose c b)
'''+SAT+'(check (= c a))')
    pair('multiple-costs-per-class',SCHEMA+'''(let a (Cost (V 1) 1)) (let b (Cost (V 2) 9))
(let middle (Cost (V 3) 5)) (union a b) (Choose a middle)
'''+SAT+'(check (= a middle))')
    pair('equal-cost-left',SCHEMA+'''(let a (Cost (V 1) 7)) (let b (Cost (V 2) 7)) (let c (Choose a b))
'''+SAT+'(check (= c a)) (run cmp 1)')
    schema=SCHEMA
    schema=schema.replace('(rule ((= lhs (Choose (Cost a ca) (Cost b ca)))) ((union lhs (Cost a ca)))', '(rule ((= lhs (Choose (Cost a ca) (Cost b ca)))) ((union lhs (Cost b ca)))')
    pair('equal-cost-right',schema+'''(let a (Cost (V 1) 7)) (let b (Cost (V 2) 7)) (let c (Choose a b))
'''+SAT+'(check (= c b))')
    values=[-(1<<63),-(1<<63)+1,-(1<<53)-1,-65537,-256,-1,0,1,255,256,65535,65536,(1<<53)+1,10**16,(1<<63)-2,(1<<63)-1]
    source=SCHEMA+''.join(f'(let c{i} (Cost (V {i}) {v}))\n' for i,v in enumerate(values))
    source+=''.join(f'(Choose c{i} c{j})\n' for i in range(len(values)) for j in range(len(values)))+SAT
    pair('all-signed-i64-boundaries',source)
    pair('hidden-cost-fallback',SCHEMA+'(let a (Cost (V 1) 1)) (let b (Cost (V 2) 3)) (Choose a b) (subsume (Cost (V 1) 1))\n'+SAT,accepted=False)
    pair('hidden-choice-fallback',SCHEMA+'(let a (Cost (V 1) 1)) (let b (Cost (V 2) 3)) (Choose a b) (subsume (Choose a b))\n'+SAT,accepted=False)
    pair('downstream-congruence',SCHEMA+'''(datatype W (Witness C))
(let a (Cost (V 1) 1)) (let b (Cost (V 2) 3)) (let c (Choose a b))
(let wa (Witness a)) (let wc (Witness c))
'''+SAT+'(check (= wa wc))')
    pair('function-callback-rejected',SCHEMA+'''(function F (C) i64 :merge (min old new))
(let a (Cost (V 1) 1)) (let b (Cost (V 2) 3)) (let c (Choose a b))
(set (F a) 5) (set (F c) 2)
'''+SAT+'(check (= (F a) 2))',admitted=False,accepted=False)
    pair('extra-equality-feedback-rejected',SCHEMA+'''(constructor Box (C) C)
(let a (Cost (V 1) 1)) (let b (Cost (V 2) 3)) (Choose a b)
'''+SAT,admitted=False,accepted=False)
    wrong=SCHEMA.replace('(< ca cb)','(TEMP ca cb)').replace('(> ca cb)','(< ca cb)').replace('(TEMP ca cb)','(> ca cb)')
    pair('different-order-policy-rejected',wrong+'(Choose (Cost (V 1) 1) (Cost (V 2) 3))\n'+SAT,admitted=False,accepted=False)
    source=SCHEMA+'''(let a (Cost (V 1) 1)) (let b (Cost (V 2) 3)) (let hole (Hole 0)) (let c (Choose hole b))
(ruleset external) (rule ((= x (Hole 0)) (= y (Cost (V 1) 1))) ((union x y)) :ruleset external)
'''+SAT+'(run external 1)\n'+SAT+'(check (= c a))'
    pair('external-union-rebind',source)
    pair('ordinary-run-stays-ordinary',SCHEMA+'(let a (Cost (V 1) 1)) (let b (Cost (V 2) 3)) (Choose (Choose a b) b) (run cmp 1)',accepted=False)
    pair('empty-choice-set',SCHEMA+'(Cost (V 1) 1)\n'+SAT)
    # Varied multi-valued classes and cyclic choice graphs exercise the exact
    # interval-existence theorem and repeated congruence, not just singleton min.
    rng=random.Random(841)
    source=SCHEMA+''.join(f'(let h{i} (Hole {i}))\n' for i in range(12))
    source+=''.join(f'(union h{rng.randrange(12)} (Cost (V {i}) {rng.choice(values)}))\n' for i in range(36))
    source+=''.join(f'(union h{rng.randrange(12)} (Choose h{rng.randrange(12)} h{rng.randrange(12)}))\n' for _ in range(24))+SAT
    pair('cyclic-multivalue-closure',source)
    source=SCHEMA+'(let c0 (Cost (V 0) 1)) (let b (Cost (V 1) 5))\n'
    source+=''.join(f'(let c{i+1} (Choose c{i} b))\n' for i in range(96))+SAT+'(check (= c96 c0))'
    pair('deep-choice-chain',source)
    renamed=SCHEMA.replace('Cost','Annotation').replace('Choose','Lower').replace('cmp','select')
    pair('renamed-rule-family',renamed+'(Lower (Annotation (V 1) 2) (Annotation (V 2) 3))\n'+SAT.replace('cmp','select'))
    source=SCHEMA+'(Choose (Cost (V 1) 1) (Cost (V 2) 3))\n(run-schedule (saturate (saturate (run cmp))))'
    pair('nested-saturate-checkpoint',source)
    resumed=run(out/'nested-saturate-checkpoint.egg',build,out/'nested-resumed',resume=out/'nested-saturate-checkpoint-matrix',matrix_saturate=True)
    assert resumed['complete_schedule'] and resumed['checks'][-1]['exact_rows_values_equivalence_visibility']
    r=run(out/'recursive-choices.egg',build,out/'macro-resumed',resume=out/'recursive-choices-matrix',matrix_saturate=True)
    assert r['complete_schedule'] and r['checks'][-1]['exact_rows_values_equivalence_visibility'];results['resume']=True
    (out/'results.json').write_text(json.dumps(results,indent=2)+'\n');return results

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--build',required=True,type=Path);p.add_argument('--output',required=True,type=Path);a=p.parse_args();r=tests(a.build,a.output);print('passed',len(r),'cases')
