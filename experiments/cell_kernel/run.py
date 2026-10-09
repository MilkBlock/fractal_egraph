#!/usr/bin/env python3
"""Source-checked fixed-shape Cell lowering, CPU/Metal, and native effect oracle."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import struct
import subprocess
import time
from lowering import cicm_program, metal_source, swift_source

ROOT=Path(__file__).resolve().parents[2]
SPEC=importlib.util.spec_from_file_location('cicm_fixture',ROOT/'experiments/cicm_interface/verify.py')
CICM=importlib.util.module_from_spec(SPEC);SPEC.loader.exec_module(CICM)
SIZE_SOURCE=ROOT.parent/'check_statewalk/dag_in_context/src/utility/expr_size.egg'


def source_rules():
    source=SIZE_SOURCE.read_text()
    rules=[]
    for ctor in ['Bop op x y','Uop op x']:
        marker=source.index(f'(= expr ({ctor}))')
        start=source.rfind('(rule',0,marker)
        end=source.index(':ruleset always-run)',marker)+len(':ruleset always-run)')
        rules.append(source[start:end])
    # This pilot recognizes precisely these two arithmetic transfers.
    compact=[' '.join(rule.split()) for rule in rules]
    expected=[
        '(rule ((= expr (Bop op x y)) (= sum (+ (Expr-size y) (Expr-size x)))) ((set (Expr-size expr) (+ sum 1))) :ruleset always-run)',
        '(rule ((= expr (Uop op x)) (= sum (Expr-size x))) ((set (Expr-size expr) (+ sum 1))) :ruleset always-run)',
    ]
    assert compact==expected, 'size rule changed: review lowering'
    assert '(= sum (+ (Expr-size y) (Expr-size x)))' in compact[0]
    assert '(= sum (Expr-size x))' in compact[1]
    assert all('(set (Expr-size expr) (+ sum 1))' in rule for rule in compact)
    assert '(function Expr-size (Expr) i64 :merge (min old new)' in source
    return rules


def compile_plan(dest):
    rules=source_rules();lhs=CICM.lhs_source()
    known=json.loads((ROOT/'experiments/cicm_interface/results.json').read_text())
    assert hashlib.sha256(lhs.encode()).hexdigest()==known['lhs_sha256'], 'CICM LHS changed: review lowering'
    plan=dict(kind='fixed-shape Cell kernel',source_rules=rules,consumer_lhs=lhs,
              shape='left=Uop(Bop(a,a)); right=Uop(Bop(b,c)); seeded boundary sizes, absent prior Bop sizes',
              affine_coefficients=[[2,0,0,0],[0,0,1,1]],bop_bias=[1,1],uop_bias=[1,1],
              bound='all ABI values in [0,2^30], s1=s0; arithmetic sums fit uint32 and checked native i64',
              original_source_sha256=hashlib.sha256(SIZE_SOURCE.read_bytes()+CICM.SOURCE.read_bytes()).hexdigest())
    (dest/'plan.json').write_text(json.dumps(plan,indent=2)+'\n')
    program,roots=cicm_program()
    plan['kind']='shared typed operator program'
    plan['program']=program.to_dict(list(roots.values()))
    plan['outputs']=roots
    (dest/'plan.json').write_text(json.dumps(plan,indent=2)+'\n')
    (dest/'kernel.metal').write_text(metal_source(program,roots))
    template=(ROOT/'experiments/cell_kernel/backend.swift').read_text()
    (dest/'backend.generated.swift').write_text(swift_source(template,program,roots))
    return rules,lhs


def records(n):
    rows=[]
    for i in range(n):
        x=(i*7+3)%8;y=(i*5+1)%8;z=(i*3+2)%8
        row=[i,x,x,y,z,5 if i%5==0 else 1_000_000,8 if i%7==0 else 1_000_000,
             15,i,i,0,0,0,0,100000+3*i,100000+3*i,200000+3*i,200000+3*i]
        case=i%16
        if case in [1,2,3,4]:row[7]&=~(1<<(case-1))
        if case in [5,6,7,8,9]:row[9+2*(case-5)]+=1
        rows.append(row)
    return rows


def reference(row):
    i,a,_,b,c,oldl,oldr,flags,*keys=row
    bl=2*a+1;br=b+c+1;l=min(oldl,bl+1);r=min(oldr,br+1)
    selected=l<10 and r<10 and flags&15==15 and all(keys[j]==keys[j+1] for j in range(0,10,2))
    return [i,bl,l,br,r,int(selected)]


def leaf(n):return f'(Leaf {n})'


def payload(r):
    i,*_=r
    pred=r[14] if r[7]&4 else 300000+3*i
    inp=r[16]
    return dict(id=i,pred=leaf(pred),inputs=leaf(inp),then=leaf(400000+2*i),else_=leaf(400001+2*i),
                op_l=f'(Op {r[10]})',op_r=f'(Op {r[11]})',ty_l=f'(BT {r[12]})',ty_r=f'(BT {r[13]})',
                term_l=f'(T {r[8]})',term_r=f'(T {r[9]})',cost_l=i+11,cost_r=i+101,
                outer=f'(Outer {i})',frame=f'$f{i}',bl=f'$bl{i}',br=f'$br{i}',l=f'$l{i}',r=f'$r{i}',
                a=leaf(10*i),b=leaf(10*i+1),c=leaf(10*i+2))


DECLS='''(datatype UnaryOp (Op i64))
(datatype BinaryOp (BOp i64))
(datatype Expr (Leaf i64) (Bop BinaryOp Expr Expr) (Uop UnaryOp Expr) (If Expr Expr Expr Expr))
(datatype BaseType (BT i64))
(datatype TypeList (TL i64))
(datatype Type (Base BaseType) (TupleT TypeList))
(datatype Term (T i64))
(datatype TermAndCost (TCPair Term i64) (Smaller TermAndCost TermAndCost))
(datatype Assumption (Outer i64) (InIf bool Expr Expr))
(relation HasArgType (Expr Type))
(relation HasType (Expr Type))
(relation ContextOf (Expr Assumption))
(relation ExprIsPure (Expr))
(function Expr-size (Expr) i64 :merge (min old new))
(function ExtractedExpr (Expr) TermAndCost :merge (Smaller old new))
(function TypeList-length (TypeList) i64 :no-merge)
(relation Instance (i64 Expr Expr Expr Expr Expr))
(relation Selected (i64))
(relation KernelResult (i64 i64 i64 i64 i64))
(ruleset result)
(rule ((Instance id frame bl l br r)
       (= sl (Expr-size l)) (= sr (Expr-size r))
       (= sbl (Expr-size bl)) (= sbr (Expr-size br)))
      ((KernelResult id sbl sl sbr sr)) :ruleset result)
(set (TypeList-length (TL 0)) 2)
(set (TypeList-length (TL 1)) 2)
'''


def setup(rows,rules,lhs,native):
    lines=[DECLS,f'(relation FullBinding (i64 {" ".join(CICM.TYPES)}))']
    if not native:
        lines.extend([f'(relation KernelPayload (i64 {" ".join(CICM.TYPES)}))',
                      '(relation Computed (i64 i64 i64 i64 i64 i64))', '(ruleset commit)',
                      '(rule ((Computed id sbl sl sbr sr chosen) (Instance id frame bl l br r)) '
                      '((set (Expr-size bl) sbl) (set (Expr-size l) sl) '
                      '(set (Expr-size br) sbr) (set (Expr-size r) sr)) :ruleset commit)',
                      f'(rule ((Computed id sbl sl sbr sr 1) (KernelPayload id {" ".join(CICM.VARIABLES)})) '
                      f'((Selected id) (FullBinding id {" ".join(CICM.VARIABLES)})) :ruleset commit)'])
    if native:
        # Add owning instance to the ORIGINAL rule body; all original predicates remain.
        body=lhs[:-1]+' (Instance id if_e bl e1 br e2))'
        lines+=['(ruleset always-run)','(ruleset select)',*rules,
                f'(rule {body} ((Selected id) (FullBinding id {" ".join(CICM.VARIABLES)})) :ruleset select)']
    for r in rows:
        p=payload(r);i=r[0]
        lines += [f'(let {p["bl"]} (Bop (BOp 0) {p["a"]} {p["a"]}))',
                  f'(let {p["br"]} (Bop (BOp 0) {p["b"]} {p["c"]}))',
                  f'(let {p["l"]} (Uop {p["op_l"]} {p["bl"]}))',
                  f'(let {p["r"]} (Uop {p["op_r"]} {p["br"]}))',
                  f'(let {p["frame"]} (If {p["pred"]} {p["inputs"]} {p["then"]} {p["else_"]}))',
                  f'(Instance {i} {p["frame"]} {p["bl"]} {p["l"]} {p["br"]} {p["r"]})',
                  f'(HasArgType {p["then"]} (TupleT (TL 0)))',
                  f'(HasArgType {p["else_"]} (TupleT (TL {0 if r[7]&8 else 1})))',
                  f'(ContextOf {p["frame"]} {p["outer"]})']
        for e,size in [(p['a'],r[1]),(p['b'],r[3]),(p['c'],r[4]),(p['l'],r[5]),(p['r'],r[6])]:
            lines.append(f'(set (Expr-size {e}) {size})')
        for side,e,ty,term,cost,pp,ii,pure in [('true',p['l'],p['ty_l'],p['term_l'],p['cost_l'],r[14],r[16],r[7]&1),
                                              ('false',p['r'],p['ty_r'],p['term_r'],p['cost_r'],r[15],r[17],r[7]&2)]:
            lines.extend([f'(HasType {e} (Base {ty}))',f'(set (ExtractedExpr {e}) (TCPair {term} {cost}))',
                          f'(ContextOf {e} (InIf {side} {leaf(pp)} {leaf(ii)}))'])
            if pure:lines.append(f'(ExprIsPure {e})')
        if not native:lines.append(f'(KernelPayload {binding(r)})')
    return '\n'.join(lines)+'\n'


def binding(r):
    p=payload(r)
    return f'{r[0]} {p["frame"]} {p["pred"]} {p["inputs"]} {p["then"]} {p["else_"]} (TL 0) {p["outer"]} {p["l"]} {p["op_l"]} {p["bl"]} {p["ty_l"]} {p["term_l"]} {p["cost_l"]} {p["r"]} {p["br"]} {p["term_r"]} {p["cost_r"]} 2'


def commit(rows,output,folder):
    assert len(rows)==len(output)
    assert [r[0] for r in rows]==[o[0] for o in output]
    path=folder/'computed.tsv'
    path.write_text('\n'.join('\t'.join(map(str,o)) for o in output)+'\n')
    return f'(input Computed "{path.resolve()}")\n(run commit 1)\n(run result 1)\n'


def call(cmd,log):
    t=time.perf_counter()
    proc=subprocess.run(list(map(str,cmd)),capture_output=True,text=True)
    elapsed=time.perf_counter()-t
    log.write_text(proc.stdout+proc.stderr)
    if proc.returncode:raise RuntimeError(f'{cmd[0]} failed; see {log}: {proc.stderr[-1000:]}')
    return elapsed,proc.stdout


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--output',type=Path,default=ROOT/'out/cell-kernel')
    parser.add_argument('--build',action='store_true',help='Build generated Swift CPU/Metal driver before measurement')
    parser.add_argument('--count',type=int,default=1024)
    parser.add_argument('--backend',type=Path,default=ROOT/'out/cell-kernel-tools/backend')
    parser.add_argument('--oracle',type=Path,default=ROOT/'target/release/examples/cell_kernel_oracle')
    parser.add_argument('--devices',nargs='+',choices=['cpu','gpu'],default=['cpu','gpu'])
    args=parser.parse_args();dest=args.output;dest.mkdir(parents=True,exist_ok=True)
    assert 1<=args.count<=10000
    t=time.perf_counter();rules,lhs=compile_plan(dest);rows=records(args.count)
    fixture_seconds=time.perf_counter()-t
    build_seconds=0
    if args.build:
        args.backend.parent.mkdir(parents=True,exist_ok=True)
        build_seconds,_=call(['swiftc','-O','-module-cache-path','/tmp/cell-kernel-module-cache',dest/'backend.generated.swift','-o',args.backend],dest/'build.log')
    expected=list(map(reference,rows))
    expected_result=[r[:5] for r in expected];expected_selected=[[r[0]] for r in expected if r[5]]
    assert len({(r[14],r[16]) for r in rows})==len(rows)
    summaries={}
    for mode in ['native',*args.devices]:
        begin=time.perf_counter();folder=dest/mode;folder.mkdir(exist_ok=True)
        prep=time.perf_counter();setup_path=folder/'setup.egg'
        setup_path.write_text(setup(rows,rules,lhs,mode=='native'))
        setup_generation_seconds=time.perf_counter()-prep
        backend_seconds=0;backend_report=None;output=expected
        if mode!='native':
            t=time.perf_counter();data=folder/'input.bin'
            data.write_bytes(b''.join(struct.pack('<18q',*r) for r in rows))
            input_generation_seconds=time.perf_counter()-t
            command=[args.backend,mode,data,folder/'output.bin']
            if mode=='gpu':command+=['--metal-source',dest/'kernel.metal']
            backend_seconds,text=call(command,folder/'backend.log')
            backend_report=json.loads(text)
            t=time.perf_counter();raw=(folder/'output.bin').read_bytes()
            assert len(raw)==args.count*6*8
            output=[list(r) for r in struct.iter_unpack('<6q',raw)]
            decode_seconds=time.perf_counter()-t
            assert output==expected
        else:input_generation_seconds=decode_seconds=0
        t=time.perf_counter();action_path=folder/'actions.egg'
        action_path.write_text('(run-schedule (saturate (run always-run)))\n(run select 1)\n(run result 1)\n' if mode=='native' else commit(rows,output,folder))
        action_generation_seconds=time.perf_counter()-t
        validation_start=time.perf_counter()
        checks=[]
        for r,o in zip(rows,expected):
            if o[5]: checks.append(f'(check (FullBinding {binding(r)}))')
            else: checks.append(f'(fail (check (Selected {r[0]})))')
        checks_path=folder/'checks.egg';checks_path.write_text('\n'.join(checks)+'\n')
        validation_generation_seconds=time.perf_counter()-validation_start
        native_seconds,_=call([args.oracle,setup_path,action_path,folder/'native.json',checks_path],folder/'native.log')
        wall=time.perf_counter()-begin
        d=json.loads((folder/'native.json').read_text())
        assert d['rows']['KernelResult']==expected_result
        assert d['rows']['Selected']==expected_selected
        assert d['table_sizes']['FullBinding']==len(expected_selected)
        assert d['table_sizes']['Expr-size']==7*args.count
        validation_seconds=validation_generation_seconds+sum(d['timing_seconds'].get('validation_'+part,0) for part in ['read','parse','execute'])
        summaries[mode]=dict(end_to_end_seconds=wall,validation_seconds=validation_seconds,
                             end_to_end_excluding_validation_seconds=wall-validation_seconds,native_process_seconds=native_seconds,
                             setup_generation_seconds=setup_generation_seconds,
                             input_generation_seconds=input_generation_seconds,
                             action_generation_seconds=action_generation_seconds,
                             backend_process_seconds=backend_seconds,output_decode_seconds=decode_seconds,
                             backend=backend_report,native=d['timing_seconds'],
                             rows_equal=True,selected=len(expected_selected),size_rows=d['table_sizes']['Expr-size'])
    report=dict(scope='Fixed-shape synthetic instance batch from real eggcc size rules/CICM Uop LHS; no full CICM transformation or instance discovery',
                count=args.count,fixture_generation_seconds=fixture_seconds,build_seconds=build_seconds,
                native_binary_sha256=hashlib.sha256(args.oracle.read_bytes()).hexdigest(),
                backend_binary_sha256=hashlib.sha256(args.backend.read_bytes()).hexdigest(),
                modes=summaries)
    (dest/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2))


if __name__=='__main__':main()
