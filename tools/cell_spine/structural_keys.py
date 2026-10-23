"""Portable eggcc structural keys using exact base-258 words.

Reuse native rewrite_ties; lower only its key representation and operations.
A word is (code, scale, height, anchor) in Vec<BigRat>. Concatenation is
(a,s)*(b,t)=(a+s*b,s*t). Complete key words are prefix-free, so numeric
comparison is exactly the original lexicographic structural ordering.
"""
import argparse
from fractions import Fraction
import hashlib
import json
from pathlib import Path
import subprocess
from . import compiler as c, pass3_markers as pm, takeover as t
from .terms import text,walk,actions
B=258
PREFIX='__sk_'
HELPER='sk-portable'
FREEZE='sk-portable-freeze'
_WORDS=None


def q(x):
    x=Fraction(x)
    def z(n):return f'(bigint {n})' if -(1<<63)<=n<(1<<63) else f'(from-string "{n}")'
    return f'(bigrat {z(x.numerator)} {z(x.denominator)})'
ZERO=q(0);ONE=q(1)

def word(tokens,height=0,anchor=0):
    code=Fraction(0);scale=Fraction(1)
    for x in tokens:scale/=B;code+=x*scale
    result=f'(vec-of {q(code)} {q(scale)} {q(height)} {q(anchor)})'
    if _WORDS is not None:return _WORDS.setdefault(result,f'{PREFIX}word{len(_WORDS)}')
    return result

def label(s):return [b+1 for b in s.encode('utf8')]
def literal_key(s):return word([2,1]+label(s)+[0,0])
def field(x,i):return f'(vec-get {x} {i})'

def pick(sort,old,new,score_old,score_new):
    return f'(map-get (map-insert (map-insert (map-empty) {score_new} {new}) {score_old} {old}) (min {score_old} {score_new}))'

def combine(words,height=ZERO,anchor=ZERO):
    result=[];code=ZERO;scale=ONE
    for i,w in enumerate(words):
        a=f'{PREFIX}c{i}';b=f'{PREFIX}s{i}'
        result += [f'(let {a} (+ {code} (* {scale} {field(w,0)})))',f'(let {b} (* {scale} {field(w,1)}))']
        code,scale=a,b
    return result,f'(vec-of {code} {scale} {height} {anchor})'

def function(name,inputs,output,merge='old'):
    return f'(function {name} ({" ".join(inputs)}) {output} :merge {merge} :unextractable)'

def rule(body,head,set=HELPER,name=None):
    return f'(rule ({" ".join(body)}) ({" ".join(head)}) :ruleset {set}'+(f' :name {json.dumps(name)}' if name else '')+')'

def key_decl(sort):
    fn='CellStructuralKey-'+sort
    merge=pick('CellStructuralKey','old','new',field('old',0),field('new',0))
    lines=[function(fn,[sort],'CellStructuralKey',merge)]
    if sort=='Expr':lines.append('(sort SKExprChoices (Map BigRat Expr))')
    lines.append(rule([f'(= {PREFIX}k ({fn} {PREFIX}x))',f'(= {field(PREFIX+"k",3)} {ZERO})'],
        combine([word([1]),PREFIX+'k',word([0])],ZERO,ONE)[0]+
        [f'(set ({fn} {PREFIX}x) {combine([word([1]),PREFIX+"k",word([0])],ZERO,ONE)[1]})'],FREEZE))
    if sort in ('Term','IVTRes'):
        cmp='SKCompare-'+sort;need='SKNeedCompare-'+sort
        lines += [f'(relation {need} ({sort} {sort}))',function(cmp,[sort,sort],'i64','new')]
        base=[f'({need} {PREFIX}a {PREFIX}b)',f'(= {PREFIX}ka ({fn} {PREFIX}a))',f'(= {PREFIX}kb ({fn} {PREFIX}b))']
        for op,result in [('<',-1),('=',0),('>',1)]:
            lines.append(rule(base+[f'({op} {field(PREFIX+"ka",0)} {field(PREFIX+"kb",0)})'],[f'(set ({cmp} {PREFIX}a {PREFIX}b) {result})']))
    return lines

def base_prelude(literals):
    lines=['(sort CellStructuralKey (Vec BigRat))','(sort SKWordChoices (Map BigRat CellStructuralKey))',
        f'(ruleset {HELPER})',f'(ruleset {FREEZE})','(relation SKNeedInt (i64))','(relation SKNeedDigits (BigInt))',
        function('SKDigits',['BigInt'],'CellStructuralKey'),function('SKIntKey',['i64'],'CellStructuralKey'),
        function('SKStringKey',['String'],'CellStructuralKey'),function('SKFloatKey',['f64'],'CellStructuralKey'),
        function('SKBoolKey',['bool'],'CellStructuralKey'),function('SKUnitKey',['Unit'],'CellStructuralKey')]
    for v in literals:
        f='SKStringKey' if v['sort']=='String' else 'SKFloatKey'
        lines.append(f'(set ({f} {v["expression"]}) {literal_key(v["label"])})')
    for v in ['true','false']:lines.append(f'(set (SKBoolKey {v}) {literal_key("bool:"+v)})')
    lines.append(f'(set (SKUnitKey ()) {literal_key("Unit:()")})')
    n=PREFIX+'n';m=PREFIX+'m';w=PREFIX+'w'
    lines += [rule([f'(SKNeedInt {n})',f'(>= {n} 0)'],[f'(SKNeedDigits (bigint {n}))']),
        rule([f'(SKNeedInt {n})',f'(< {n} 0)'],[f'(SKNeedDigits (- (bigint 0) (bigint {n})))']),
        rule([f'(SKNeedDigits {m})',f'(>= {m} (bigint 10))'],[f'(SKNeedDigits (/ {m} (bigint 10)))'])]
    digit=lambda n:f'(vec-of (bigrat (+ {n} (bigint 49)) (bigint {B})) {q(Fraction(1,B))} {ZERO} {ZERO})'
    lines.append(rule([f'(SKNeedDigits {m})',f'(< {m} (bigint 10))'],[f'(set (SKDigits {m}) {digit(m)})']))
    act,tail=combine([w,digit(f'(% {m} (bigint 10))')])
    lines.append(rule([f'(SKNeedDigits {m})',f'(>= {m} (bigint 10))',f'(= {w} (SKDigits (/ {m} (bigint 10))))'],act+[f'(set (SKDigits {m}) {tail})']))
    for neg in [False,True]:
        mag=f'(- (bigint 0) (bigint {n}))' if neg else f'(bigint {n})'
        act,k=combine([word([2,1]+label('i64:'+('-' if neg else ''))),w,word([0,0])])
        lines.append(rule([f'(SKNeedInt {n})',f'({"<" if neg else ">="} {n} 0)',f'(= {w} (SKDigits {mag}))'],act+[f'(set (SKIntKey {n}) {k})']))
    return lines

def constructor_rules(name,sig,sorts):
    xs=[f'{PREFIX}x{i}' for i in range(len(sig['input']))];ks=[f'{PREFIX}k{i}' for i in range(len(xs))]
    seed=f'(= {PREFIX}out ({name} {" ".join(xs)}))';body=[seed];needs=[]
    for x,k,sort in zip(xs,ks,sig['input']):
        f='CellStructuralKey-'+sort if sort in sorts else {'i64':'SKIntKey','f64':'SKFloatKey','String':'SKStringKey','bool':'SKBoolKey','Unit':'SKUnitKey'}.get(sort)
        if f is None:return [rule([seed],[f'(panic {json.dumps("unsupported structural key field "+sort+" in "+name)})'])]
        if sort=='i64':needs.append(f'(SKNeedInt {x})')
        body.append(f'(= {k} ({f} {x}))')
    h=ZERO
    for k in ks:h=f'(max {h} (+ {field(k,2)} {ONE}))'
    hvar=PREFIX+'height';powvar=PREFIX+'pow'
    # [257 repeated height, 1]: exact order-preserving unary height code.
    hword=f'(vec-of (+ (- {ONE} {powvar}) (* {powvar} {q(Fraction(1,B))})) (* {powvar} {q(Fraction(1,B))}) {ZERO} {ZERO})'
    act,k=combine([word([2]),hword,word(label(sig['output']+':'+name)+[0])]+ks+[word([0])],hvar)
    head=[f'(let {hvar} {h})',f'(let {powvar} (pow {q(Fraction(1,B))} {hvar}))']+act+[f'(set (CellStructuralKey-{sig["output"]} {PREFIX}out) {k})']
    return ([rule([seed],needs)] if needs else [])+[rule(body,head)]

def lower_rule(r):
    requests=[]
    def lower(e):
        if e['kind']!='call':return e
        if e['head']=='cell-key-cmp':
            a,b=e['args'];prefix='CellStructuralKey-'
            if a['kind']!='call' or b['kind']!='call' or a['head']!=b['head'] or not a['head'].startswith(prefix):raise ValueError('unsupported key comparison shape')
            sort=a['head'][len(prefix):]
            if sort not in ('Term','IVTRes'):raise ValueError('comparison sort outside reused policy')
            requests.append((sort,a['args'][0],b['args'][0]))
            return t.call('SKCompare-'+sort,[a['args'][0],b['args'][0]])
        return dict(e,args=[lower(x) for x in e['args']])
    body=[];request_body=[]
    for f in r['body']:
        guarded=any(e.get('head')=='cell-key-cmp' for root in (f['args'] if f['kind']=='eq' else [f['expr']]) for e in walk(root))
        if not guarded:request_body.append(f)
        body.append(dict(f,args=[lower(e) for e in f['args']]) if f['kind']=='eq' else dict(f,expr=lower(f['expr'])))
    extra=[]
    for i,(sort,a,b) in enumerate(requests):
        head=[dict(kind='expr',expr=t.call('SKNeedCompare-'+sort,[a,b]))]
        extra.append(pm.render_rule(dict(r,name=f'sk-request-{r["ruleset"]}-{r["ordinal"]}-{i}'),request_body,head,HELPER))
    return extra+[pm.render_rule(r,body,r['head'])]

def schedule(s):
    k=s['kind']
    if k=='run':return f'(seq (saturate {HELPER}) {s["text"]})'
    if k=='seq':return '(seq '+' '.join(schedule(c) for c in s['children'])+')'
    return f'({k} '+(str(s['count'])+' ' if k=='repeat' else '')+schedule(s['child'])+')'

def compile_source(source,output,metadata_binary=None,ast_binary=None):
    global _WORDS
    old=_WORDS;_WORDS={}
    try:return _compile_source(source,output,metadata_binary,ast_binary)
    finally:_WORDS=old

def _compile_source(source,output,metadata_binary=None,ast_binary=None):
    output=Path(output);output.mkdir(parents=True,exist_ok=False)
    source=Path(source).resolve();original=source.read_text()
    if any(s in original for s in [PREFIX,'CellStructuralKey','SKNeed','SKCompare','sk-portable']):raise ValueError('reserved structural-key namespace in input')
    binary=metadata_binary or c.ROOT/'target/analyzer/release/examples/structural_key_source'
    subprocess.run([str(binary),str(source),str(output/'native')],cwd=c.ROOT,check=True)
    meta=json.loads((output/'native/metadata.json').read_text())
    ast=ast_binary or c.ROOT/'target/analyzer/release/examples/lhs_ast'
    subprocess.run([str(ast),str(output/'native/policy.egg'),str(output/'policy.json')],cwd=c.ROOT,check=True)
    data=json.loads((output/'policy.json').read_text());rules={r['command_ordinal']:r for r in data['rules']}
    assert len(meta['commands'])==len(data['commands']), 'native command ordinals changed'
    if 'to-hoist' in data['signatures']:
        for name,inputs,output_sort in [('to-hoist',['Expr','Expr'],'Expr'),('Expr-size',['Expr'],'i64')]:
            sig=data['signatures'].get(name,{})
            if sig.get('kind')!='function' or sig.get('input')!=inputs or sig.get('output')!=output_sort:
                raise ValueError('reused eggcc policy requires function '+name+' with its eggcc schema')
    constructors=meta['constructors'];sorts=set(meta['sorts']);defined=set();emitted=set();lines=base_prelude(meta['literals']);frozen=False
    # Identify only true empty-LHS initialization; do not freeze after inference.
    def initializer(s):
        while s['kind'] in ('seq','repeat'):
            if s['kind']=='seq':
                if len(s['children'])!=1:return False
                s=s['children'][0]
            else:
                if s['count']!=1:return False
                s=s['child']
        if s['kind']!='run' or ':until' in s['text']:return False
        rs=[r for r in data['rules'] if r['ruleset']==s['ruleset']]
        return bool(rs) and all(not r['body'] for r in rs)
    def add_sort(s):
        if s in sorts and s not in defined:lines.extend(key_decl(s));defined.add(s)
    for command in data['commands']:
        k=command['kind'];ordinal=command['ordinal'];src=meta['commands'][ordinal]
        if k=='rule':lines.extend(lower_rule(rules[ordinal]));continue
        if k=='schedule':
            sched=command['schedule']
            if not frozen and initializer(sched):lines.append(src);continue
            if not frozen:
                lines.append(f'(run-schedule (seq (saturate {HELPER}) (run {FREEZE})))');frozen=True
            lines.append(f'(run-schedule (seq {schedule(sched)} (saturate {HELPER})))');continue
        if src.startswith('(function to-hoist '):
            old=f'(CellStructuralKey-Expr old)';new=f'(CellStructuralKey-Expr new)'
            score=lambda value,key:f'(- {field(key,0)} (bigrat (bigint (Expr-size {value})) (bigint 1)))'
            merge=pick('Expr','old','new',score('old',old),score('new',new))
            # Native metadata provides this known function's original schema.
            src=f'(function to-hoist (Expr Expr) Expr :merge {merge} :unextractable)'
        lines.append(src)
        if k=='declaration':
            # Native parser declarations avoid guessing constructor signatures.
            for d in data['declarations']:
                if d['kind']=='sort' and src.startswith('(sort '+d['name']+' '):add_sort(d['name'])
                if d['kind']=='sort' and src=='(sort '+d['name']+')':add_sort(d['name'])
                if d['kind']=='datatype' and src.startswith('(datatype '+d['name']+' '):add_sort(d['name'])
            present=[]
            if src.startswith('(constructor '):present=[src.split()[1]]
            elif src.startswith('(relation '):present=[src.split()[1]]
            elif src.startswith('(datatype '):present=[v['name'] for d in data['declarations'] if d.get('name')==src.split()[1] for v in d.get('constructors',[])]
            elif src.startswith('(datatypes '):raise ValueError('expand mutually recursive datatypes with the existing flat exporter before standalone conversion')
            for name in present:
                if name in constructors:
                    add_sort(constructors[name]['output'])
                    lines.extend(constructor_rules(name,constructors[name],sorts));emitted.add(name)
    if emitted!=set(constructors):raise ValueError('unplaced native constructors: '+str(set(constructors)-emitted))
    lines=lines[:2]+[f'(let {name} {expr})' for expr,name in _WORDS.items()]+lines[2:]
    result='; egg-layout structural-keys/standalone-v1\n; '+json.dumps({'native_rulesets':[HELPER,FREEZE],'policy':'eggcc-v1 order, exact rational word representation'})+'\n'+'\n'.join(lines)+'\n'
    (output/'source_sk.egg').write_text(result);(output/'source.egg').write_bytes(source.read_bytes())
    report=dict(schema='standalone-structural-keys/v1',source=str(source),source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
        output_sha256=hashlib.sha256(result.encode()).hexdigest(),native_rulesets=[HELPER,FREEZE],rewrites=meta['rewrites'],
        constructors=len(constructors),sorts=sorted(sorts),literals=len(meta['literals']),
        reused_policy=meta['origin'],representation='prefix-free base-258 words in Vec<BigRat>; no custom sort or primitive',
        limits=['String/f64 must originate in source literals; computed String/f64 primitives are rejected.',
                'Actual unsupported container constructor fields fail; unused declarations are allowed.',
                'Key maintenance contributes native schedule progress, as an explicit part of the transformed policy.',
                'Rational words expand structural witnesses; no claim of compact DAG storage or speedup.'])
    (output/'report.json').write_text(json.dumps(report,indent=2)+'\n');return report

if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--source',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();print(json.dumps(compile_source(a.source,a.output)))
