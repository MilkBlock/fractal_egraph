#!/usr/bin/env python3
"""Compile real Math Cell grafts to a constructor/equality response operator."""
import argparse
from copy import deepcopy
import hashlib
import json
from pathlib import Path
import subprocess

from artifacts import ROOT,DEFAULT_CELLS,load_family,prepare_probe_source,parsed_probes,environment_source,compact_evidence
from closure import Closure
from polynomial import Poly,ONE
from response import Response


def call(args,log):
    result=subprocess.run(list(map(str,args)),capture_output=True,text=True)
    log.write_text(result.stdout+result.stderr)
    if result.returncode:raise RuntimeError(result.stderr[-2500:])


def negative_checks(oracle,manifest,folder):
    original=manifest['cases'][15]
    checks=[]
    case=deepcopy(original);case['compiled_actions']=[s for s in case['compiled_actions'] if not s.startswith('(union')]
    checks.append(('lost-union-effects',case))
    case=deepcopy(original)
    removed={s for s in case['expected_nodes'] if s.startswith('(Sub ')}
    assert removed
    case['expected_nodes']=[s for s in case['expected_nodes'] if s not in removed]
    case['expected_classes']=[[s for s in group if s not in removed] for group in case['expected_classes']]
    case['expected_classes']=[group for group in case['expected_classes'] if group]
    checks.append(('omitted-constructor-row',case))
    case=deepcopy(original);case['expected_classes'][0]+=case['expected_classes'].pop(1)
    checks.append(('false-equality',case))
    case=deepcopy(original);case['compiled_actions'].append('(run 1)')
    checks.append(('hidden-rule-execution',case))
    for name,case in checks:
        case.pop('replay_source',None)
        path=folder/f'negative-{name}.json'
        path.write_text(json.dumps(dict(environment_source=manifest['environment_source'],cases=[case]))+'\n')
        result=subprocess.run([str(oracle),str(path),str(folder/f'negative-{name}-result.json')],capture_output=True,text=True)
        if result.returncode==0:raise AssertionError(f'negative oracle accepted {name}')
        (folder/f'negative-{name}.log').write_text(result.stderr)
    return [name for name,_ in checks]


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--cells',type=Path,default=DEFAULT_CELLS)
    parser.add_argument('--output',type=Path,default=ROOT/'out/structural-cell-operator')
    parser.add_argument('--parser',type=Path,default=ROOT/'target/release/examples/lhs_ast')
    parser.add_argument('--oracle',type=Path,default=ROOT/'target/release/examples/structural_cell_verify')
    parser.add_argument('--compile-only',action='store_true')
    args=parser.parse_args();out=args.output;out.mkdir(parents=True,exist_ok=True)
    cells=load_family(args.cells);cell=cells[36]
    source,probes=prepare_probe_source(cell)
    (out/'parser-input.egg').write_text(source)
    call([args.parser,out/'parser-input.egg',out/'ast.json'],out/'parser.log')
    ast=json.loads((out/'ast.json').read_text());parsed=parsed_probes(ast,probes)
    env=out/'environment.egg';env.write_text(environment_source(cell,ast))
    stages=parsed['stages']
    assert [s['before_record'] for s in stages]==[9,11,16,49]
    assert [a['action']['kind'] for a in stages[2]['actions']]==['expr','expr','union']
    gates=[('outer_mul',stages[0]['actions']),('sub_context',stages[1]['actions']),
           ('square',stages[2]['actions'][:2]),('connect',stages[2]['actions'][2:])]
    # Reconstruct the real opaque boundary from native-parsed recorded terms.
    base={t['term']:{'kind':'expr','expr':t['expr']} for t in parsed['typed_tokens']
          if t['expr']['kind']=='call' and t['expr']['head']=='RipenInput'}
    assert len(base)==4
    closure=Closure(ast['signatures'],ast['rules'])
    for action in base.values():closure.action(action)
    for i,(_,actions) in enumerate(gates):
        for action in actions:closure.action(action['action'],Poly.var(i))
    sweeps=closure.saturate(lambda i,n,e: print(f'symbolic sweep {i}: {n} typed terms, {e} possible equalities',flush=True))
    observations=closure.observations();response=Response(observations)
    cases=[]
    for assignment in range(1<<len(gates)):
        values=response.evaluate(assignment)
        assert values=={name:p.evaluate(assignment) for name,p in observations.items()}
        nodes,classes=closure.world(assignment,values)
        actions=list(base)+[action['source'] for i,(_,part) in enumerate(gates) if assignment&(1<<i) for action in part]
        case=dict(name=f'boundary-{assignment:04b}',actions=actions,
                  expected_nodes=[closure.render(n) for n in nodes],
                  expected_classes=[[closure.render(n) for n in c] for c in classes],
                  compiled_actions=closure.compiled_actions(assignment,values))
        if assignment==15:case['replay_source']=cells[36]['paths']['entry']
        cases.append(case)
    # Same endpoint reached through recorded further commutativity has no new closure.
    repeat=dict(cases[-1]);repeat['name']='recorded-successor-59';repeat['replay_source']=cells[59]['paths']['entry'];cases.append(repeat)
    transitions=[]
    for before in range(16):
        old=response.evaluate(before)
        for gate,(_,packet) in enumerate(gates):
            if before&(1<<gate):continue
            after=before|(1<<gate);new=response.evaluate(after)
            # Finite difference is an exact polynomial update, not a derivative
            # linearization: a fresh boundary bit moves from precisely 0 to 1.
            assert all(p.difference(gate).evaluate(before)==new[name]-old[name] for name,p in observations.items())
            delta=closure.delta_actions(old,new)
            cases.append(dict(name=f'graft-{before:04b}-gate{gate}',
                              actions=closure.compiled_actions(before,old)+[a['source'] for a in packet],
                              expected_nodes=cases[after]['expected_nodes'],
                              expected_classes=cases[after]['expected_classes'],compiled_actions=delta))
            transitions.append(dict(before=before,gate=gate,after=after,delta_actions=len(delta)))
    manifest=dict(environment_source=str(env.resolve()),cases=cases)
    mp=out/'manifest.json';mp.write_text(json.dumps(manifest,indent=2)+'\n')
    report=dict(scope='Exact finite constructor/equality boundary response of recorded Math Cell36',
                cells=[compact_evidence(c) for c in cells.values()],
                gates=[dict(index=i,name=name,actions=[a['source'] for a in actions]) for i,(name,actions) in enumerate(gates)],
                omitted_stage=dict(record=49,reason='Recorded nodes and union must be derived by the source rules and congruence; checked against complete replay.'),
                symbolic_sweeps=sweeps,response=response.report(),
                transitions=transitions,
                polynomials={name:p.to_json() for name,p in observations.items()},
                readable_polynomials={name:p.to_string([g[0] for g in gates]) for name,p in observations.items()},
                rules_with_new_term_spellings={name:len(terms) for name,terms in closure.rule_effects.items() if terms},
                environment_sha256=hashlib.sha256(env.read_bytes()).hexdigest(),
                limitations=[
                    'Missing-gate worlds are positive input variants, not failure/phase-equivalent replays of the recorded checks.',
                    'Requires finite ground-term spelling closure and an acyclic quotient for compact emission; finite native egraphs alone do not ensure this.',
                    'Rank measures a four-gate fact/equality response, not physical egraph storage compression.',
                    'No arbitrary parent-DAG composition, generic guarded/function/subsume rules, runtime speedup, or GPU execution is claimed.'])
    if not args.compile_only:
        call([args.oracle,mp,out/'native.json'],out/'native.log')
        report['native']=json.loads((out/'native.json').read_text())
        report['negative_checks']=negative_checks(args.oracle,manifest,out)
        report['oracle_sha256']=hashlib.sha256(args.oracle.read_bytes()).hexdigest()
    (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    (out/'compiled-all.egg').write_text(env.read_text()+'\n; Matrix-selected structural effect. No rule schedule.\n'+'\n'.join(cases[15]['compiled_actions'])+'\n')
    print(json.dumps(response.report(),indent=2))


if __name__=='__main__':main()
