#!/usr/bin/env python3
"""Incremental factorized CICM Uop matcher vs verbatim native LHS.

Standalone interface prototype, not a production eggcc optimization.
"""
import argparse
from collections import defaultdict
from copy import deepcopy
import hashlib
from itertools import product
import json
from pathlib import Path
import re
import subprocess

ROOT=Path(__file__).resolve().parents[2]
SOURCE=ROOT.parent/'check_statewalk/dag_in_context/src/optimizations/conditional_invariant_code_motion.egg'
VARIABLES='if_e pred orig_ins thn els tylist outer_ctx e1 o x ty t1 c1 e2 y t2 c2 orig_ins_len'.split()
TYPES='Expr Expr Expr Expr Expr TypeList Assumption Expr UnaryOp Expr BaseType Term i64 Expr Expr Term i64 i64'.split()


def lhs_source():
    text=SOURCE.read_text()
    marker=text.index('(= if_e (If pred orig_ins thn els))')
    start=text.rfind('(rule (',0,marker)
    stop=text.index('\n      (\n        ; pull the term out',marker)
    body=text[start+len('(rule '):stop].strip()
    assert body.startswith('(') and body.endswith(')')
    assert '(> 10 (Expr-size e1))' in body and '(= t1 t2)' in body
    return body


def original_rule():
    text=SOURCE.read_text()
    marker=text.index('(= if_e (If pred orig_ins thn els))')
    start=text.rfind('(rule (',0,marker)
    end=text.index(':ruleset cicm)',marker)+len(':ruleset cicm)')
    return text[start:end]


class Model:
    def __init__(self):
        self.parent={}
        self.rows={}
        self.frames=[]
        self.pending=[]
        self.buckets=defaultdict(set)
        self.row_key={}
        self.dependents=defaultdict(set)
        self.reindexed=0
        self.retracted=0

    def find(self,x):
        return self.find(self.parent[x]) if x in self.parent else x

    def key(self,row):
        size=row.get('size')
        if not row['visible'] or not row['pure'] or size is None or size>=10:
            return None
        return tuple(self.find(row[k]) for k in ['term','pred','inputs','op','ty'])

    def index(self,name):
        row=self.rows[name]
        old=self.row_key.pop(name,None)
        if old is not None:
            self.buckets[old].remove(name)
            self.retracted+=1
        key=self.key(row)
        if key is not None:
            key=(row['side'],)+key
            self.buckets[key].add(name)
            self.row_key[name]=key
        self.reindexed+=1

    def insert(self,row):
        row=deepcopy(row)
        value=row.pop('initial_size')
        row['size']=None if value is None else min(value,10)
        self.rows[row['name']]=row
        for field in ['term','pred','inputs','op','ty']:
            self.dependents[self.find(row[field])].add(row['name'])
        self.index(row['name'])

    def update(self,event):
        kind,*args=event
        if kind=='size':
            name,value=args
            assert 0<=value<=1_000_000, 'fixture size outside verified nonnegative domain'
            row=self.rows[name];before=self.key(row)
            row['size']=min(10,value) if row['size'] is None else min(row['size'],value,10)
            if before!=self.key(row):self.index(name)
        elif kind in ['pure','subsume']:
            name=args[0];row=self.rows[name]
            row['pure' if kind=='pure' else 'visible']=kind=='pure'
            self.index(name)
        elif kind=='pending':self.pending.append(tuple(args))
        elif kind=='commit':
            for a,b in self.pending:self.union(a,b)
            # Delayed records remain, as in the selected source commit rule.
        elif kind=='union':self.union(*args)
        else:raise ValueError(event)

    def union(self,a,b):
        a,b=self.find(a),self.find(b)
        if a==b:return
        # This fixture only unions Term IDs and predicate/input Expr IDs.
        # No ExtractedExpr-key merging or general constructor congruence is modeled.
        allowed={self.find(r[k]) for r in self.rows.values() for k in ['term','pred','inputs']}
        assert a in allowed and b in allowed and a[:2]==b[:2], 'unsupported union interface'
        touched=self.dependents.pop(b,set())
        self.parent[b]=a
        self.dependents[a].update(touched)
        for name in touched:self.index(name)

    def frame_rows(self):
        for f in self.frames:
            if f['then_type']==f['else_type'] and f['length'] is not None:
                yield f

    def binding(self,f,l,r):
        # All named source variables, including distinct costs and operand slots.
        return (f['name'],self.find(f['pred']),self.find(f['inputs']),f['then'],f['else'],
                f['then_type'],f['outer'],l['name'],l['op'],l['operand'],l['ty'],
                self.find(l['term']),l['cost'],r['name'],r['operand'],self.find(r['term']),r['cost'],f['length'])

    def matches(self):
        matches=set();products=0;blocks=0
        for f in self.frame_rows():
            pred,inputs=self.find(f['pred']),self.find(f['inputs'])
            for key,lefts in self.buckets.items():
                side,term,p,i,op,ty=key
                if not side or (p,i)!=(pred,inputs) or not lefts:continue
                rights=self.buckets.get((False,term,p,i,op,ty),())
                if not rights:continue
                blocks+=1
                for l,r in product(lefts,rights):
                    products+=1
                    matches.add(self.binding(f,self.rows[l],self.rows[r]))
        return matches,dict(factor_blocks=blocks,emitted_pairs=products)

    def brute_force(self):
        result=set();pairs=0
        for f in self.frame_rows():
            for l,r in product(self.rows.values(),repeat=2):
                pairs+=1
                if not l['side'] or r['side']:continue
                kl,kr=self.key(l),self.key(r)
                if kl is None or kr is None or kl!=kr:continue
                if (kl[1],kl[2])!=(self.find(f['pred']),self.find(f['inputs'])):continue
                result.add(self.binding(f,l,r))
        return result,pairs


def row(name,side,term='t:A',size=3,pure=True,op='op:neg',ty='ty:int',pred='e:p0',inputs='e:i0',cost=11):
    return dict(name=name,side=side,term=term,initial_size=size,pure=pure,op=op,ty=ty,
                pred=pred,inputs=inputs,cost=cost,operand='e:arg_'+name,visible=True)


def fixture():
    rows=[row('e:L1',True,size=4),row('e:L2',True,size=9,cost=12),row('e:L3',True,size=200),
          row('e:R1',False,size=12,cost=50),row('e:R2',False,term='t:B'),
          row('e:R3',False,op='op:abs'),row('e:R4',False,ty='ty:bool'),
          row('e:R5',False,pure=False),row('e:R6',False,pred='e:p1',inputs='e:i1'),
          row('e:R7',False,size=None),row('e:R8',False,pred='e:p1')]
    frames=[dict(name='e:F0',pred='e:p0',inputs='e:i0',then='e:th0',else_='e:el0',then_type='tl:0',else_type='tl:0',outer='ctx:0',length=2),
            dict(name='e:F1',pred='e:p1',inputs='e:i1',then='e:th1',else_='e:el1',then_type='tl:0',else_type='tl:0',outer='ctx:1',length=2),
            dict(name='e:bad',pred='e:p0',inputs='e:i0',then='e:thbad',else_='e:elbad',then_type='tl:0',else_type='tl:1',outer='ctx:bad',length=2)]
    for f in frames:f['else']=f.pop('else_')
    events=[('size','e:R1',10),('size','e:R1',9),('size','e:R1',25),('size','e:R7',1),
            ('pure','e:R5'),('pending','e:p0','e:p1'),('commit',),('union','t:A','t:B'),
            ('union','e:i0','e:i1'),('subsume','e:L1'),('size','e:L3',8),('size','e:L3',6)]
    return rows,frames,events


def value(x):
    if isinstance(x,int):return str(x)
    prefix,name=x.split(':',1)
    ctor={'e':'Leaf','t':'T','op':'Op','ty':'BT','tl':'TL','ctx':'Outer'}[prefix]
    return f'({ctor} "{name}")'


def symbol(name):return '$'+name.replace(':','_')


def native_program(body,rows,frames,events,snapshots,execute_rhs=False,immediate_subsume=False):
    lines=['(sort Expr)', '(sort Assumption)', '(sort Type)', '(sort Term)',
           '(datatype UnaryOp (Op String))',
           '(constructor Leaf (String) Expr)', '(constructor Uop (UnaryOp Expr) Expr)',
           '(constructor If (Expr Expr Expr Expr) Expr)', '(constructor TermSubst (Assumption Expr Term) Expr)',
           '(constructor Concat (Expr Expr) Expr)', '(constructor Single (Expr) Expr)',
           '(constructor SubTuple (Expr i64 i64) Expr)', '(constructor Arg (Type Assumption) Expr)',
           '(constructor Subst (Assumption Expr Expr) Expr)', '(constructor Get (Expr i64) Expr)','(datatype BaseType (BT String))',
           '(datatype TypeList (TL String) (TLConcat TypeList TypeList) (TCons BaseType TypeList) (TNil))',
           '(constructor Base (BaseType) Type)', '(constructor TupleT (TypeList) Type)',
           '(constructor T (String) Term)',
           '(datatype TermAndCost (TCPair Term i64) (Smaller TermAndCost TermAndCost))',
           '(constructor Outer (String) Assumption)', '(constructor InIf (bool Expr Expr) Assumption)',
           '(relation HasArgType (Expr Type))','(relation HasType (Expr Type))',
           '(relation ContextOf (Expr Assumption))','(relation ExprIsPure (Expr))',
           '(function Expr-size (Expr) i64 :merge (min old new))',
           '(function ExtractedExpr (Expr) TermAndCost :merge (Smaller old new))',
           '(function TypeList-length (TypeList) i64 :merge old)',
           '(constructor DelayedSubstUnion (Expr Expr) Expr :unextractable)',
           '(ruleset commit)','(rule ((DelayedSubstUnion lhs rhs)) ((union lhs rhs)) :ruleset commit)']
    if execute_rhs:lines.extend(['(ruleset execute)', original_rule().replace(':ruleset cicm)', ':ruleset execute)')])
    for r in rows:
        e=symbol(r['name']);lines.append(f'(let {e} (Uop {value(r["op"])} {value(r["operand"])}))')
        lines.extend([f'(HasType {e} (Base {value(r["ty"])}))',
                      f'(set (ExtractedExpr {e}) (TCPair {value(r["term"])} {r["cost"]}))',
                      f'(ContextOf {e} (InIf {str(r["side"]).lower()} {value(r["pred"])} {value(r["inputs"])}))'])
        if r['initial_size'] is not None:lines.append(f'(set (Expr-size {e}) {r["initial_size"]})')
        if r['pure']:lines.append(f'(ExprIsPure {e})')
    for f in frames:
        lines.extend([f'(let {symbol(f["name"])} (If {value(f["pred"])} {value(f["inputs"])} {value(f["then"])} {value(f["else"])}))',
                      f'(HasArgType {value(f["then"])} (TupleT {value(f["then_type"])}))',
                      f'(HasArgType {value(f["else"])} (TupleT {value(f["else_type"])}))',
                      f'(ContextOf {symbol(f["name"])} {value(f["outer"])} )',
                      f'(set (TypeList-length {value(f["then_type"])}) {f["length"]})'])
    named={r['name'] for r in rows}|{f['name'] for f in frames}
    def ref(x):return symbol(x) if x in named else value(x)
    for stage,bindings in enumerate(snapshots):
        if stage:
            kind,*args=events[stage-1]
            if kind=='size':lines.append(f'(set (Expr-size {ref(args[0])}) {args[1]})')
            elif kind=='pure':lines.append(f'(ExprIsPure {ref(args[0])})')
            elif kind=='subsume':
                r=next(r for r in rows if r['name']==args[0])
                lines.append(f'(subsume (Uop {value(r["op"])} {value(r["operand"])}))')
            elif kind=='pending':lines.append(f'(DelayedSubstUnion {ref(args[0])} {ref(args[1])})')
            elif kind=='commit':lines.append('(run commit 1)')
            elif kind=='union':lines.append(f'(union {ref(args[0])} {ref(args[1])})')
        # Fresh collector avoids historical matches surviving subsume in an old relation.
        lines.extend([f'(ruleset observe{stage})',f'(relation Hit{stage} ({" ".join(TYPES)}))',
                      f'(rule {body} ((Hit{stage} {" ".join(VARIABLES)})) :ruleset observe{stage})',
                      f'(run observe{stage} 1)',f'(print-size Hit{stage})'])
        for binding in sorted(bindings):lines.append(f'(check (Hit{stage} {" ".join(ref(x) for x in binding)}))')
        if execute_rhs:
            lines.extend(['(run execute 1)', f'(ruleset live{stage})',
                          f'(relation Live{stage} (UnaryOp Expr))',
                          f'(rule ((Uop o x)) ((Live{stage} o x)) :ruleset live{stage})',
                          f'(run live{stage} 1)'])
            for binding in sorted(bindings):
                v={name:ref(x) for name,x in zip(VARIABLES,binding)}
                ins=f'(Concat {v["orig_ins"]} (Single (TermSubst {v["outer_ctx"]} {v["orig_ins"]} {v["t1"]})))'
                ty=f'(TupleT (TLConcat {v["tylist"]} (TCons {v["ty"]} (TNil))))'
                branches=[]
                for side,branch,e,operand in [('true','thn','e1','x'),('false','els','e2','y')]:
                    ctx=f'(InIf {side} {v["pred"]} {ins})'
                    st=f'(SubTuple (Arg {ty} {ctx}) 0 {v["orig_ins_len"]})'
                    branches.append(f'(Subst {ctx} {st} {v[branch]})')
                    lines.append(f'(check (= (Get (Arg {ty} {ctx}) {v["orig_ins_len"]}) (Subst {ctx} {st} {v[e]})))')
                    if immediate_subsume:
                        lines.append(f'(fail (check (Live{stage} {v["o"]} (Subst {ctx} {st} {v[operand]}))))')
                lines.append(f'(check (= {v["if_e"]} (If {v["pred"]} {ins} {branches[0]} {branches[1]})))')
    return '\n'.join(lines)+'\n'


def main():
    p=argparse.ArgumentParser()
    p.add_argument('--output',type=Path,default=ROOT/'out/cicm-interface')
    p.add_argument('--binary',type=Path,default=ROOT/'target/release/examples/native_math_baseline')
    args=p.parse_args();args.output.mkdir(parents=True,exist_ok=True)
    body=lhs_source();rows,frames,events=fixture();m=Model();m.frames=frames
    for r in rows:m.insert(r)
    snapshots=[];stages=[]
    for i,event in enumerate([None]+events):
        old=m.reindexed
        if event:m.update(event)
        actual,metrics=m.matches();reference,pairs=m.brute_force()
        assert actual==reference
        snapshots.append(actual)
        stages.append(dict(stage=i,event=event,matches=len(actual),reindexed_rows=m.reindexed-old,
                           brute_force_pair_attempts=pairs,**metrics))
    copy_model=Model();copy_model.frames=frames
    for r in reversed(rows):copy_model.insert(r)
    for event in events:copy_model.update(event)
    assert copy_model.matches()[0]==snapshots[-1]
    try:
        copy_model.union('e:L1','e:L2')
    except AssertionError:
        pass
    else:
        raise AssertionError('unsupported Expr-key union was accepted')
    # Exhaustive cap identities on a bounded domain plus larger values.
    values=list(range(33))+[1000,1000000]
    for a,b in product(values,repeat=2):
        cap=lambda x:min(10,x)
        assert cap(min(a,b))==min(cap(a),cap(b))
        assert cap(a+b)==cap(cap(a)+cap(b))
        assert (a<10)==(cap(a)<10)
    smoke=args.output/'fresh-subsume.egg'
    smoke.write_text('(datatype E (A) (B E))\n(relation Seen (E))\n(relation Done ())\n(A)\n'
                     '(ruleset make)\n(rule ((= x (A))) ((B x) (subsume (B x)) (Done)) :ruleset make)\n'
                     '(run make 1)\n(ruleset see)\n(rule ((= x (B y))) ((Seen x)) :ruleset see)\n'
                     '(run see 1)\n(print-size Seen)\n(print-size Done)\n(subsume (B (A)))\n'
                     '(relation SeenAfter (E))\n(ruleset see-after)\n'
                     '(rule ((= x (B y))) ((SeenAfter x)) :ruleset see-after)\n'
                     '(run see-after 1)\n(print-size SeenAfter)\n')
    probe=subprocess.run([str(args.binary.resolve()),str(smoke.resolve())],capture_output=True,text=True)
    smoke.with_suffix('.log').write_text(probe.stdout+probe.stderr)
    assert probe.returncode==0,probe.stderr
    visibility=list(map(int,re.findall(r'^\s*(\d+)\s*$',probe.stdout,re.M)))
    assert len(visibility)==3 and visibility[2]==0,visibility
    immediate_subsume=visibility[:2]==[0,1]
    rhs_failure=None
    for execute_rhs in [False,True]:
        program=native_program(body,rows,frames,events,snapshots,execute_rhs,immediate_subsume)
        path=args.output/f'cicm-uop-rhs{int(execute_rhs)}.egg';path.write_text(program)
        result=subprocess.run([str(args.binary.resolve()),str(path.resolve())],capture_output=True,text=True)
        path.with_suffix('.log').write_text(result.stdout+result.stderr)
        if result.returncode and execute_rhs and not immediate_subsume:
            assert 'CheckError' in result.stderr and visibility[:2]==[1,0], result.stderr
            assert re.search(r'\(check \(= \$e_F\d+ \(If',result.stderr), result.stderr
            rhs_failure='Original RHS did not establish its final If union; fresh-create/subsume probe also skips the following Done action.'
            continue
        assert result.returncode==0,result.stderr
        counts=list(map(int,re.findall(r'^\s*(\d+)\s*$',result.stdout,re.M)))
        assert counts==[len(s) for s in snapshots],(counts,[len(s) for s in snapshots])
    report=dict(scope='Standalone incremental interface for the verbatim CICM Uop LHS; original RHS separately attempted and status reported; production pipeline not replaced',
                source=str(SOURCE),source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),
                lhs_sha256=hashlib.sha256(body.encode()).hexdigest(),
                binary_sha256=hashlib.sha256(args.binary.read_bytes()).hexdigest(),
                named_binding_variables=VARIABLES,cap_law_input_pairs=len(values)**2,
                native_binding_sets_equal=True,original_rhs_construction_and_union_checked=rhs_failure is None,
                original_rhs_failure=rhs_failure,
                existing_node_subsume_checked=True,
                original_rhs_fresh_subsume_checked=immediate_subsume,
                fresh_subsume_visible_rows=visibility[0],fresh_subsume_following_action_count=visibility[1],stages=stages)
    (args.output/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2))


if __name__=='__main__':main()
