"""Full native-schedule differential tests of generic, device-local recipes."""
import argparse,copy,json,subprocess
from pathlib import Path
from run_generic_cells import run_library
from verify_native_source import verify

CASES={
'representative-order': '''(datatype E (N i64) (Choice E E))
(function F () E :merge (Choice old new)) (relation Trigger (E))
(ruleset consume) (ruleset merge)
(rule ((Trigger x)) ((set (F) (N 99))) :ruleset consume)
(rule ((= a (N 2)) (= b (N 10))) ((union a b)) :ruleset merge)
'''+' '.join(f'(N {i})' for i in range(13))+'''
(Trigger (N 2)) (set (F) (N 2))
(run consume 1) (run merge 1) (run consume 1)''',
'properties': '''(datatype E (V i64) (Op i64) (Bop E E E))
(relation Pure (E)) (relation Valid (E)) (relation PureOp (E))
(ruleset up) (ruleset down)
(rule ((= root (Bop op x y)) (PureOp op) (Pure x) (Pure y)) ((Pure root)) :ruleset up)
(rule ((= root (Bop op x y)) (Valid root)) ((Valid x) (Valid y)) :ruleset down)
(Bop (Op 1) (V 0) (V 1)) (PureOp (Op 1)) (Pure (V 0)) (Pure (V 1)) (Valid (Bop (Op 1) (V 0) (V 1)))
(run-schedule (saturate (run down) (run up)))''',
'correlated': '''(datatype E (V i64) (Bop E E))
(relation Contains (E E)) (relation Inv (E E)) (ruleset inv)
(rule ((= root (Bop x y)) (Contains body root) (Inv body x) (Inv body y)) ((Inv body root)) :ruleset inv)
(Bop (V 0) (V 1))
(Contains (V 10) (Bop (V 0) (V 1))) (Contains (V 11) (Bop (V 0) (V 1)))
(Inv (V 10) (V 0)) (Inv (V 11) (V 1))
(run-schedule (saturate (run inv)))''',
'numeric': '''(datatype E (V i64) (Bop E E))
(function Size (E) i64 :merge (min old new)) (relation Small (E))
(ruleset size) (ruleset improve) (ruleset consume)
(rule ((= root (Bop x y)) (= sx (Size x)) (= sy (Size y))) ((set (Size root) (+ 1 (+ sx sy)))) :ruleset size)
(rule ((= x (V 0))) ((set (Size x) 1)) :ruleset improve)
(rule ((= n (Size root)) (< n 10)) ((Small root)) :ruleset consume)
(Bop (V 0) (V 1)) (set (Size (V 0)) 4) (set (Size (V 1)) 7) (set (Size (Bop (V 0) (V 1))) 14)
(run size 1) (run consume 1) (run improve 1) (run-schedule (saturate (run size) (run consume)))''',
'recursive': '''(datatype E (V i64) (Get E i64))
(function Length (E) i64 :merge (max old new)) (relation AddAll (E E i64 i64)) (relation Same (E E))
(ruleset generate) (ruleset consumer)
(rule ((AddAll orig block offset pos) (< pos (Length block))) ((union (Get orig (+ offset pos)) (Get block pos)) (AddAll orig block offset (+ pos 1))) :ruleset generate)
(rule ((= g (Get a i)) (= g (Get b j))) ((Same a b)) :ruleset consumer)
(AddAll (V 0) (V 1) 5 0) (set (Length (V 1)) 3)
(run-schedule (saturate (run generate) (run consumer)))''',
'new-union-match': '''(datatype E (V i64) (Pair E E)) (relation P (E)) (relation Q (E)) (relation Found (E))
(ruleset a) (ruleset merge)
(rule ((= t (Pair x y)) (P x) (Q y) (= x y)) ((Found t)) :ruleset a)
(rule ((= x (V 0)) (= y (V 1))) ((union x y)) :ruleset merge)
(Pair (V 0) (V 1)) (P (V 0)) (Q (V 1))
(run-schedule (saturate (run a) (run merge)))''',
'identity': '''(datatype E (V i64) (Tag E) (Next E)) (function Chosen () E :merge new)
(ruleset make) (ruleset consume)
(rule ((= x (V n))) ((Tag x)) :ruleset make)
(rule ((= y (Tag x))) ((set (Chosen) y) (Next y)) :ruleset consume)
(V 0) (run-schedule (saturate (run make) (run consume)))''',
'custom-merge': '''(datatype E (V i64) (Choice E E)) (function Best (E) E :merge (Choice old new))
(relation Proposal (E E)) (relation Seen (E)) (ruleset propose) (ruleset consume)
(rule ((Proposal root candidate)) ((set (Best root) candidate)) :ruleset propose)
(rule ((= x (Choice a b))) ((Seen x)) :ruleset consume)
(Proposal (V 0) (V 1)) (Proposal (V 0) (V 2))
(run-schedule (saturate (run propose) (run consume)))''',
'visibility': '''(datatype E (V i64)) (relation P (E)) (relation Q (E)) (relation Early (E))
(ruleset both) (ruleset observe)
(rule ((= x (V n))) ((P x)) :ruleset both)
(rule ((P x)) ((Q x)) :ruleset both)
(rule ((Q x)) ((Early x)) :ruleset observe)
(V 0) (run both 1) (run observe 1)''',
'finite-repeat': '''(datatype E (N i64)) (ruleset grow)
(rule ((= x (N n)) (< n 20)) ((N (+ n 1))) :ruleset grow)
(N 0) (run-schedule (repeat 5 (run grow)))''',
'missing-and-large': '''(datatype E (V i64) (Bop E E)) (function Size (E) i64 :merge (min old new))
(ruleset size)
(rule ((= root (Bop x y)) (= sx (Size x)) (= sy (Size y))) ((set (Size root) (+ 1 (+ sx sy)))) :ruleset size)
(Bop (V 0) (V 1)) (Bop (V 0) (V 2)) (set (Size (V 0)) 10000000000000000) (set (Size (V 1)) 3)
(run-schedule (saturate (run size)))''',
}

def core_for(source,build):
    p=subprocess.run([str((build/'frontend').resolve()),str(source.resolve())],input='',text=True,capture_output=True,check=True)
    return json.loads(p.stdout.splitlines()[0])

def tests(build,out):
    out.mkdir(parents=True,exist_ok=False);programs=[];instances=[];sources=[]
    for name,text in CASES.items():
        source=out/(name+'.egg');source.write_text(text);sources.append(source)
        core=core_for(source,build);initial=core.pop('initial');idx=len(programs);programs.append(core)
        instances.append(dict(unit=idx,program=idx,initial=initial))
    library=dict(schema='analyzer-device-cells/v1',programs=programs,instances=instances)
    (out/'library.json').write_text(json.dumps(library))
    result=run_library(library,out/'build',out/'gpu')
    assert result['complete'],result['rejected']
    checks={}
    for i,source in enumerate(sources):
        folder=out/'gpu'/f'program-{i}'/f'cell-{i}'
        checks[source.stem]=verify(source,build,folder,out/f'native-{i}')
        print(source.stem,checks[source.stem],flush=True)
    # Same kernel, distinct input bindings and missing facts: no cross-instance
    # sharing of mutable state, even when every local integer ID is identical.
    property_id=list(CASES).index('properties')
    lib=copy.deepcopy(library);lib['programs']=[programs[property_id]];lib['instances']=[]
    for i in range(32):
        initial=copy.deepcopy(instances[property_id]['initial'])
        if i%2:initial=[r for r in initial if r['table']!='PureOp']
        lib['instances'].append(dict(unit=i,program=0,initial=initial))
    batch=run_library(lib,out/'build',out/'batch')
    assert batch['complete']
    for i in range(32):
        text=CASES['properties'] if not i%2 else CASES['properties'].replace('(PureOp (Op 1))','(PureOp (Op 1)) (delete (PureOp (Op 1)))')
        source=out/f'batch-{i%2}.egg';source.write_text(text)
        if i<2:verify(source,build,out/'batch/program-0'/f'cell-{i}',out/f'batch-native-{i}')
        state=json.loads((out/'batch/program-0'/f'cell-{i}'/'gpu-state.json').read_text())
        ref=json.loads((out/'batch/program-0'/f'cell-{i%2}'/'gpu-state.json').read_text())
        assert state['rows']==ref['rows'] and state['parents']==ref['parents']
    (out/'checks.json').write_text(json.dumps(dict(checks=checks,batch_instances=32),indent=2))

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--build',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args();tests(a.build,a.output)
