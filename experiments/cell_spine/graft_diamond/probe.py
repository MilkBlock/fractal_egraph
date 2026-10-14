"""Detect history/context predecessor bias and test native/compact graft effects.

No discovery algorithm is changed by this probe. Logical source checks and
whole-state differential comparisons are kept separate from graph coverage.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT=Path(__file__).resolve().parents[3]
sys.path.insert(0,str(ROOT))
from tools.cell_spine import compiler as c, markers


INDEPENDENT='''(datatype T (V String) (Seed T T) (Add T T) (GateL T) (GateR T) (Left T) (Right T))
(ruleset all)
(rule ((Seed a b)) ((Add a b)) :ruleset all :name "base")
(rule ((Add a b) (GateL a)) ((Left a)) :ruleset all :name "left")
(rule ((Add a b) (GateR b)) ((Right b)) :ruleset all :name "right")
(let vx (V "x")) (let vy (V "y"))
(Seed vx vy) (run all 1)
'''
UNION='''(datatype T (V String) (Seed T T) (Add T T) (MergeGate T T) (GateR T) (Right T))
(ruleset all)
(rule ((Seed a b)) ((Add a b)) :ruleset all :name "base")
(rule ((MergeGate a b)) ((union a b)) :ruleset all :name "enable")
(rule ((Add a b) (GateR b) (= a b)) ((Right b)) :ruleset all :name "right")
(let vx (V "x")) (let vy (V "y"))
(Seed vx vy) (run all 1)
(GateR vy) (run all 1)
(fail (check (Right vy)))
'''
SYMMETRIC='''(datatype T (V String) (Add T T) (Gate T) (Mark T))
(ruleset all)
(rewrite (Add a b) (Add b a) :ruleset all :name "swap")
(rule ((Add a b) (Gate a)) ((Mark a)) :ruleset all :name "mark")
'''
UNION_COMPACT='''(datatype T (V String) (Add T T) (MergeGate T T) (Gate T) (Mark T))
(ruleset all)
(rule ((MergeGate a b)) ((union a b)) :ruleset all :name "enable")
(rule ((Add a b) (Gate b) (= a b)) ((Mark b)) :ruleset all :name "mark")
'''
DELAYED='''(datatype T (V String) (Add T T) (Mirror T T) (GateL T) (StartR T) (MidR T) (GateR T) (Left T) (Right T))
(ruleset all)
(rewrite (Add a b) (Mirror a b) :ruleset all :name "base")
(rule ((Add a b) (GateL a)) ((Left a)) :ruleset all :name "left")
(rule ((StartR b)) ((MidR b)) :ruleset all :name "delay-1")
(rule ((MidR b)) ((GateR b)) :ruleset all :name "delay-2")
(rule ((Add a b) (GateR b)) ((Right b)) :ruleset all :name "right")
'''


def run(args,log,env=None):
    with log.open('w') as output:
        r=subprocess.run(list(map(str,args)),cwd=ROOT,env=env,stdout=output,stderr=subprocess.STDOUT)
    if r.returncode:raise RuntimeError(f'{r.returncode}: see {log}\n{log.read_text()[-4000:]}')


def analyze(out,name,source,windows):
    path=out/(name+'.egg');path.write_text(source)
    dest=out/name
    run([ROOT/'target/release/egg_layout','analyze','--source',path,'--output',dest,'--save-history'],out/(name+'.log'),
        dict(os.environ,EGG_LAYOUT_GRAPHS='0',EGG_LAYOUT_ANALYSIS_THREADS='1',
             EGG_LAYOUT_CELL_WINDOWS='1' if windows else '0',RAYON_NUM_THREADS='1'))
    report=json.loads((dest/'analysis.json').read_text())['saturated_rule_composition']
    graph=json.loads((dest/'cell-egraph.json').read_text());jobs={j['candidate_id']:j for j in report['jobs']}
    rules={m['record']:m['rule_name'] for j in jobs.values() for m in j.get('origin',{}).get('comb_members',[])}
    members={i:[m['rule_name'] for m in j.get('origin',{}).get('comb_members',[])] for i,j in jobs.items()}
    transitions=[]
    for comp in report['cs']['compositions']:
        transitions.append(dict(consumer=rules.get(comp['consumer']),
            producers=[members.get(p,[]) for p in comp['producer_parts']],
            context=[members.get(p,[]) for p in comp['context_parts']],
            all_predecessors=[members.get(p,[]) for p in comp['parts']]))
    summaries=[]
    for node in graph['nodes']:
        j=jobs[node['unit']];folder=Path(j['folder'])
        if not folder.is_absolute():folder=ROOT/folder
        file=folder/'saturated-rule-composition.json'
        state=json.loads(file.read_text()) if file.exists() else None
        summaries.append(dict(unit=node['unit'],kind=j['candidate_kind'],state=j['state'],
            members=members[node['unit']],parents=[members.get(p,[]) for p in node['parents']],
            effects=sorted({r['op'] for r in state['rows']}) if state else [],snapshot=str(file) if state else None))
    return dict(transitions=transitions,entries=summaries,windows=report['cs']['windows'])


def find(result,names):
    return next(e for e in result['entries'] if sorted(e['members'])==sorted(names) and e['snapshot'])


def compare(a,b):
    p=subprocess.run([ROOT/'target/release/egg_layout','saturated-rule-composition-compare',a,b],cwd=ROOT,capture_output=True,text=True,check=True)
    return json.loads(p.stdout)


def compact(out,name,header,training,orders,guarded=False):
    source=out/(name+'.egg');source.write_text(header+training)
    bake=out/(name+'-bake')
    run([sys.executable,'-m','tools.cell_spine.compiler','--source',source,'--output',bake,'--skip-build'],out/(name+'-bake.log'))
    base=json.loads((bake/'work/validation.json').read_text());cases=[]
    for label,order in orders:
        def program(forked):
            suffix='!' if forked else ''
            schedule=markers.RUNTIME_SCHEDULE if forked else '(run-schedule (saturate all))'
            p=f'(let vx (V{suffix} "x")) (let vy (V{suffix} "y"))\n(Add{suffix} vx vy)\n'+schedule+'\n'
            if guarded:p+=f'(Gate{suffix} vy)\n'+schedule+'\n'
            for side in order:
                p+=(f'(MergeGate{suffix} vx vy)' if side=='union' else f'(Gate{suffix} v{side})')+'\n'+schedule+'\n'
            return p
        checks=[]
        for value in ('x','y'):
            expected=('union' in order) if guarded else value in order
            fact=f'(check (Mark (V "{value}")))'
            checks.append(fact if expected else f'(fail {fact})')
        cases.append(dict(name=label,reference=header+program(False)+'\n'+'\n'.join(checks),
                          compressed=program(True),restore='(run-schedule (saturate Export!))\n'+'\n'.join(checks)))
    manifest=dict(constructors=base['constructors'],library=base['library'],cases=cases)
    path=out/(name+'-verify.json');path.write_text(json.dumps(manifest))
    result=out/(name+'-result.json')
    run([ROOT/'target/release/examples/spine_validate',path,result],out/(name+'-verify.log'))
    return json.loads(result.read_text())


def delayed_compact(out):
    """Within one native saturation, right is enabled later but needs no left."""
    source=out/'delayed.egg';source.write_text(DELAYED+'''(let vx (V "x")) (let vy (V "y"))
(Add vx vy) (GateL vx) (StartR vy) (run-schedule (saturate all)) (check (Left vx) (Right vy))''')
    bake=out/'delayed-bake'
    run([sys.executable,'-m','tools.cell_spine.compiler','--source',source,'--output',bake,'--skip-build'],out/'delayed-bake.log')
    manifest=json.loads((bake/'work/validation.json').read_text())
    reference=DELAYED+'''(let vx (V "x")) (let vy (V "y")) (Add vx vy) (StartR vy)
(run-schedule (saturate all)) (check (Right vy)) (fail (check (Left vx)))'''
    compressed='(let vx (V! "x")) (let vy (V! "y")) (Add! vx vy) (StartR! vy)\n'+markers.RUNTIME_SCHEDULE
    manifest['cases']=[dict(name='delayed-right-only',reference=reference,compressed=compressed,
        restore='(run-schedule (saturate Export!)) (check (Right (V "y"))) (fail (check (Left (V "x"))))')]
    path=out/'delayed-verify.json';path.write_text(json.dumps(manifest))
    result=out/'delayed-result.json';run([ROOT/'target/release/examples/spine_validate',path,result],out/'delayed-verify.log')
    return json.loads(result.read_text())


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True)
    p.add_argument('--history-only',action='store_true');a=p.parse_args()
    out=a.output.resolve();out.mkdir(parents=True,exist_ok=False);reports={}
    orders=[('left',['left']),('right',['right']),('left-right',['left','right']),('right-left',['right','left'])]
    for name,order in orders:
        source=INDEPENDENT
        for side in order:source+=('(GateL vx)' if side=='left' else '(GateR vy)')+'\n(run all 1)\n'
        for side,value in [('left','vx'),('right','vy')]:
            check=f'(check ({side.title()} {value}))';source+=(check if side in order else f'(fail {check})')+'\n'
        reports[name]=analyze(out,name,source,True)
    reports['same-batch']=analyze(out,'same-batch',INDEPENDENT+'(GateL vx) (GateR vy) (run all 1) (check (Left vx) (Right vy))',True)
    reports['without-windows']=analyze(out,'without-windows',(out/'left-right.egg').read_text(),False)
    reports['union-disabled']=analyze(out,'union-disabled',UNION,True)
    reports['union-enabled']=analyze(out,'union-enabled',UNION+'(MergeGate vx vy) (run all 2) (check (Right vy))',True)
    checks={
        'right_window_equals_right_only':compare(find(reports['left-right'],['base','right'])['snapshot'],find(reports['right'],['base','right'])['snapshot']),
        'left_window_equals_left_only':compare(find(reports['right-left'],['base','left'])['snapshot'],find(reports['left'],['base','left'])['snapshot']),
        'both_orders_equal':compare(find(reports['left-right'],['base','left','right'])['snapshot'],find(reports['right-left'],['base','right','left'])['snapshot'])}
    if any(c['status']!='equivalent' for c in checks.values()):raise ValueError('graft closure differs across order')
    if 'Left' in find(reports['left-right'],['base','right'])['effects']:raise ValueError('right window leaked left effects')
    if 'Right' in find(reports['right-left'],['base','left'])['effects']:raise ValueError('left window leaked right effects')
    if any(c['consumer']=='right' for c in reports['union-disabled']['transitions']):raise ValueError('equality guard fired before union')
    coverage={}
    for order,last in [('left-right','right'),('right-left','left')]:
        entry=find(reports[order],['base',last]);coverage[order]=dict(independent_window_exists=True,
            linked_to_base=['base'] in entry['parents'],other_side_effect_absent=('Left' if last=='right' else 'Right') not in entry['effects'])
    coverage['without-windows']=dict(independent_right_exists=any(sorted(e['members'])==['base','right'] for e in reports['without-windows']['entries']))
    result=dict(scope='actual native capture/ripen and generated spine differential checks; graph coverage is separate from soundness',history=reports,closure_checks=checks,coverage=coverage)
    if not a.history_only:
        # Both orientations are initial witnesses so the two marked instances
        # can share one certified family under different port bindings.
        seed='(let vx (V "x")) (let vy (V "y")) (Add vx vy) (Add vy vx) (Gate vx) (Gate vy) (run-schedule (saturate all))'
        result['symmetric_compact']=compact(out,'symmetric',SYMMETRIC,seed,[('left',['x']),('right',['y']),('left-right',['x','y']),('right-left',['y','x'])])
        one=result['symmetric_compact']['cases'][:2]
        owners=[{k for k,n in row['active_summary_tables'].items() if n} for row in one]
        if owners[0]!=owners[1]:raise ValueError('symmetry probe did not exercise the same family at both bindings')
        seed='(let vx (V "x")) (let vy (V "y")) (Add vx vy) (Gate vy) (MergeGate vx vy) (run-schedule (saturate all))'
        result['union_compact']=compact(out,'union-compact',UNION_COMPACT,seed,[('before-union',[]),('after-union',['union'])],True)
        result['delayed_compact']=delayed_compact(out)
    (out/'results.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(dict(closure_checks=checks,compact_checks={k:len(v['cases']) for k,v in result.items() if k.endswith('_compact')})))


if __name__=='__main__':main()
