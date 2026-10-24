"""Native LHS/RHS-index oracle; preserves the five real loop conditions.

The large existing native engine is reused. Snapshot leaves become opaque typed
IDs; this is a query replay, not a complete eggcc pass or closure simulation.
"""
import re
import subprocess


def loop_program(data,fresh=None):
    out=['(datatype V (Value i64))','(datatype T (Ty i64))','(datatype C (Ctx i64))',
         '(datatype E (N i64) (DoWhile E E) (Const V T C))',
         '(relation ContextOf (E C))','(relation HasArgType (E T))',
         '(relation Root (E i64))','(relation OldConst (E i64))',
         '(relation Found (i64 i64 i64 i64 i64))','(ruleset query)']
    for root,inp,body in data['loops']:
        out += [f'(union (N {root}) (DoWhile (N {inp}) (N {body})))',f'(Root (N {root}) {root})']
    for e,c in data['contexts']:out += [f'(ContextOf (N {e}) (Ctx {c}))']
    for e,t in data['types']:out += [f'(HasArgType (N {e}) (Ty {t}))']
    for const,c,t,ctx in data['constants']:
        out += [f'(union (N {const}) (Const (Value {c}) (Ty {t}) (Ctx {ctx})))',f'(OldConst (N {const}) {const})']
    lhs='''(= loop (DoWhile inputs body))
                     (ContextOf inputs input_ctx)
                     (ContextOf body output_ctx)
                     (= constant (Const c output_type output_ctx))
                     (HasArgType inputs input_type)
                     (Root loop root_id) (OldConst constant constant_id)
                     (= input_ctx (Ctx ci)) (= input_type (Ty ti)) (= c (Value vi))'''
    conditions=['']
    if fresh is not None:
        out += ['(relation FreshRoot (E E E))','(relation FreshContext (E C))',
                '(relation FreshType (E T))','(relation FreshConst (E V T C))']
        for i in fresh.get('loops',set()):
            root,inp,body=data['loops'][i];out += [f'(FreshRoot (N {root}) (N {inp}) (N {body}))']
        for i in fresh.get('contexts',set()):
            e,c=data['contexts'][i];out += [f'(FreshContext (N {e}) (Ctx {c}))']
        for i in fresh.get('types',set()):
            e,t=data['types'][i];out += [f'(FreshType (N {e}) (Ty {t}))']
        for i in fresh.get('constants',set()):
            c,v,t,ctx=data['constants'][i];out += [f'(FreshConst (N {c}) (Value {v}) (Ty {t}) (Ctx {ctx}))']
        conditions=['(FreshRoot loop inputs body)','(FreshContext inputs input_ctx)',
                    '(FreshContext body output_ctx)','(FreshType inputs input_type)',
                    '(FreshConst constant c output_type output_ctx)']
    for condition in conditions:out += [f'(rule ({lhs} {condition}) ((Found root_id ci ti vi constant_id)) :ruleset query)']
    out += ['(run query 1)','(print-function Found 100000000)']
    return '\n'.join(out)+'\n'


def check_loop(binary,data,expected,output,fresh=None):
    output.mkdir(parents=True,exist_ok=False)
    p=output/'native.egg';p.write_text(loop_program(data,fresh))
    run=subprocess.run([str(binary),str(p)],text=True,capture_output=True)
    (output/'native.stderr').write_text(run.stderr)
    run.check_returncode()
    actual={tuple(map(int,m.split())) for m in re.findall(r'^\s*\(Found ([^)\n]+)\)\s*->',run.stdout,re.M)}
    if actual!=expected:
        raise AssertionError(f'native query mismatch: missing={list(expected-actual)[:5]} extra={list(actual-expected)[:5]}')
    return {'exact_rhs_coordinate_set':True,'coordinates':len(actual),'mode':'native five-condition query replay; no sat-skip or full Pass 3 rerun'}


def check_if(binary,data,expected,output):
    output.mkdir(parents=True,exist_ok=False)
    out=['(datatype BT (StateT))','(datatype T (Ty i64) (Base BT))','(datatype C (Ctx i64))',
         '(datatype E (N i64) (Arg T C) (Get E i64) (If E E E E))',
         '(relation HasType (E T))','(relation Label (E i64))',
         '(relation Found (i64 i64 i64 i64))','(ruleset query)']
    labels=set()
    for root,pred,inp,th,el in data['ifs']:
        out += [f'(union (N {root}) (If (N {pred}) (N {inp}) (N {th}) (N {el})))']
        labels.update([root,inp])
    for arg,t,c in data['args']:out += [f'(union (N {arg}) (Arg (Ty {t}) (Ctx {c})))']
    for p,i,v in data['gets']:out += [f'(union (N {v}) (Get (N {p}) {i}))']
    for v in data['state_values']:out += [f'(HasType (N {v}) (Base (StateT)))']
    for v in sorted(labels):out += [f'(Label (N {v}) {v})']
    out += ['''(rule ((= outputs (If pred inputs then_ else_))
                     (= (Get then_ i) (Get (Arg arg_ty then_ctx) j))
                     (= (Get else_ i) (Get (Arg arg_ty else_ctx) j))
                     (HasType (Get then_ i) (Base (StateT)))
                     (Label outputs root_id) (Label inputs input_id))
                    ((Found root_id input_id i j)) :ruleset query)''',
            '(run query 1)','(print-function Found 100000000)']
    path=output/'native.egg';path.write_text('\n'.join(out)+'\n')
    run=subprocess.run([str(binary),str(path)],capture_output=True,text=True)
    (output/'native.stderr').write_text(run.stderr);run.check_returncode()
    actual={tuple(map(int,m.split())) for m in re.findall(r'^\s*\(Found ([^)\n]+)\)\s*->',run.stdout,re.M)}
    assert actual==expected, f'If native mismatch: missing={list(expected-actual)[:5]}, extra={list(actual-expected)[:5]}'
    return {'exact_rhs_coordinate_set':True,'coordinates':len(actual),'mode':'native If LHS projection, no rewrite effects committed'}


def check_comparator(binary,data,expected,output):
    output.mkdir(parents=True,exist_ok=False)
    out=['(datatype Term (Atom i64))','(datatype P (Ref i64) (TCPair Term i64) (Smaller P P))',
         '(relation Label (P i64))','(relation Found (i64 i64 i64))','(ruleset query)']
    labels=set()
    for klass,term,cost in data['pairs']:
        out += [f'(union (Ref {klass}) (TCPair (Atom {term}) {cost}))'];labels.add(klass)
    for lhs,a,b in data['smaller']:
        out += [f'(union (Ref {lhs}) (Smaller (Ref {a}) (Ref {b})))'];labels.update([lhs,a,b])
    for label in sorted(labels):out += [f'(Label (Ref {label}) {label})']
    for code,cmp,target in ((0,'<','a'),(1,'>','b'),(2,'=','a')):
        out += [f'''(rule ((= lhs (Smaller a b)) (= a (TCPair t1 c1)) (= b (TCPair t2 c2))
                           ({cmp} c1 c2) (Label lhs root_id) (Label {target} target_id))
                          ((Found {code} root_id target_id)) :ruleset query)''']
    out += ['(run query 1)','(print-function Found 100000000)']
    path=output/'native.egg';path.write_text('\n'.join(out)+'\n')
    run=subprocess.run([str(binary),str(path)],capture_output=True,text=True)
    (output/'native.stderr').write_text(run.stderr);run.check_returncode()
    actual={tuple(map(int,m.split())) for m in re.findall(r'^\s*\(Found ([^)\n]+)\)\s*->',run.stdout,re.M)}
    wanted={(code,*pair) for code,key in enumerate(('lt','gt','eq')) for pair in expected[key]}
    assert actual==wanted,f'comparator mismatch: missing={list(wanted-actual)[:5]}, extra={list(actual-wanted)[:5]}'
    return {'exact_rule_specific_union_targets':True,'requests':len(actual),'mode':'native comparison LHS oracle; unions not committed'}
