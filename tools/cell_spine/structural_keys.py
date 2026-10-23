"""Generate explicit .egg key maintenance for the shared native DAG extension.

Default policy is automatic-refutation's Expr-size / structural to-hoist choice.
Original source equivalence is not required; source_sk is the marker oracle.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
from . import compiler as c, pass3_markers as pm, takeover as t
from .terms import walk,egg_string
PREFIX='__sk_'
HELPER='cell-stable-keys'
FREEZE='cell-stable-freeze'
HEADER='; cell-structural-key-runtime/v1 '


def function(name,inputs,output,merge='old'):
    return f'(function {name} ({" ".join(inputs)}) {output} :merge {merge} :unextractable)'


def rule(body,head,ruleset=HELPER):
    return f'(rule ({" ".join(body)}) ({" ".join(head)}) :ruleset {ruleset})'


def key_decl(sort,comparisons):
    fn='CellStructuralKey-'+sort
    lines=[function(fn,[sort],'CellStructuralKey','(cell-key-min old new)'),
        rule([f'(= {PREFIX}k ({fn} {PREFIX}x))'],[f'(set ({fn} {PREFIX}x) (cell-key-anchor {PREFIX}k))'],FREEZE)]
    if sort=='Expr':lines.append('(sort SKExprChoices (Map bool Expr))')
    if sort in comparisons:
        lines += [f'(relation SKNeedCompare-{sort} ({sort} {sort}))',function('SKCompare-'+sort,[sort,sort],'i64','new')]
        lines.append(rule([f'(SKNeedCompare-{sort} {PREFIX}a {PREFIX}b)',f'(= {PREFIX}ka ({fn} {PREFIX}a))',f'(= {PREFIX}kb ({fn} {PREFIX}b))'],
            [f'(set (SKCompare-{sort} {PREFIX}a {PREFIX}b) (cell-key-cmp {PREFIX}ka {PREFIX}kb))']))
    return lines


def constructor_rules(name,sig,sorts):
    xs=[f'{PREFIX}x{i}' for i in range(len(sig['input']))]
    seed=f'(= {PREFIX}out ({name} {" ".join(xs)}))';body=[seed];keys=[]
    for i,(x,sort) in enumerate(zip(xs,sig['input'])):
        if sort in sorts:
            k=f'{PREFIX}k{i}';body.append(f'(= {k} (CellStructuralKey-{sort} {x}))');keys.append(k)
        elif sort in ('i64','String','f64','bool','Unit'):keys.append(f'(cell-key-base-{sort} {x})')
        else:return [rule([seed],[f'(panic {egg_string("unsupported structural key field "+sort+" in "+name)})'])]
    return [rule(body,[f'(set (CellStructuralKey-{sig["output"]} {PREFIX}out) (cell-key-node-{name} {" ".join(keys)}))'])]


def lower_rule(r):
    requests=[]
    def lower(e):
        if e['kind']!='call':return e
        if e['head']=='cell-key-cmp':
            a,b=e['args'];prefix='CellStructuralKey-'
            if a['kind']!='call' or b['kind']!='call' or a['head']!=b['head'] or not a['head'].startswith(prefix):raise ValueError('unsupported key comparison shape')
            sort=a['head'][len(prefix):];requests.append((sort,a['args'][0],b['args'][0]))
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
    return extra+[pm.render_rule(r,body,r['head'])],bool(requests)


def schedule(s,consumers):
    k=s['kind']
    if k=='run':return f'(seq (saturate {HELPER}) {s["text"]})' if s['ruleset'] in consumers else s['text']
    if k=='seq':return '(seq '+' '.join(schedule(c,consumers) for c in s['children'])+')'
    return f'({k} '+(str(s['count'])+' ' if k=='repeat' else '')+schedule(s['child'],consumers)+')'


def compile_source(source,output,metadata_binary=None,ast_binary=None,policy='automatic'):
    if policy not in ('automatic','full'):raise ValueError('unknown structural-key policy')
    output=Path(output);output.mkdir(parents=True,exist_ok=False)
    source=Path(source).resolve();original=source.read_text()
    if any(s in original for s in [PREFIX,'CellStructuralKey','cell-key-','SKNeed','SKCompare',HELPER,FREEZE,HEADER]):raise ValueError('reserved structural-key namespace in input')
    binary=metadata_binary or c.ROOT/'target/analyzer/release/examples/structural_key_source'
    subprocess.run([str(binary),str(source),str(output/'native')],cwd=c.ROOT,check=True)
    meta=json.loads((output/'native/metadata.json').read_text())
    ast=ast_binary or c.ROOT/'target/analyzer/release/examples/lhs_ast'
    selected='source' if policy=='automatic' else 'policy'
    subprocess.run([str(ast),str(output/f'native/{selected}.egg'),str(output/'policy.json')],cwd=c.ROOT,check=True)
    data=json.loads((output/'policy.json').read_text());rules={r['command_ordinal']:r for r in data['rules']}
    texts=meta['source_commands' if policy=='automatic' else 'commands']
    assert len(texts)==len(data['commands']), 'native command ordinals changed'
    roots=set();comparisons=set()
    if 'to-hoist' in data['signatures']:
        for name,inputs,output_sort in [('to-hoist',['Expr','Expr'],'Expr'),('Expr-size',['Expr'],'i64')]:
            sig=data['signatures'].get(name,{})
            if sig.get('kind')!='function' or sig.get('input')!=inputs or sig.get('output')!=output_sort:
                raise ValueError('reused eggcc policy requires function '+name+' with its eggcc schema')
        roots.add('Expr')
    if policy=='full':
        if meta['rewrites']['smaller_ties'] or meta['rewrites']['equal_cost_improvement']:comparisons.add('Term')
        if meta['rewrites']['ivt_ties']:comparisons.add('IVTRes')
        roots|=comparisons
    sorts=set(roots);all_constructors=meta['constructors'];all_sorts=set(meta['sorts'])
    while True:
        dependencies={s for sig in all_constructors.values() if sig['output'] in sorts for s in sig['input'] if s in all_sorts}
        if dependencies<=sorts:break
        sorts|=dependencies
    constructors={name:sig for name,sig in all_constructors.items() if sig['output'] in sorts}
    supported={n:sig for n,sig in constructors.items() if all(s in sorts or s in ('i64','f64','String','bool','Unit') for s in sig['input'])}
    spec=dict(storage='shared-native-dag',policy=policy,
        bases=sorted({s for sig in supported.values() for s in sig['input'] if s not in sorts}),
        nodes=[dict(primitive='cell-key-node-'+name,label=sig['output']+':'+name,arity=len(sig['input'])) for name,sig in sorted(supported.items())])
    compiled={};consumers=set()
    for ordinal,r in rules.items():
        compiled[ordinal],uses_key=lower_rule(r)
        if uses_key or any(a.get('kind')=='set' and a.get('head')=='to-hoist' for a in r['head']):consumers.add(r['ruleset'])
    # Maintain keys before any scheduled combined set containing a consumer.
    while True:
        parents={n for n,children in data.get('combined_rulesets',{}).items() if any(c in consumers for c in children)}
        if parents<=consumers:break
        consumers|=parents
    defined=set();emitted=set();lines=[f'(ruleset {HELPER})',f'(ruleset {FREEZE})'];frozen=False
    def initializer(s):
        while s['kind'] in ('seq','repeat'):
            if s['kind']=='seq':
                if len(s['children'])!=1:return False
                s=s['children'][0]
            else:
                if s['count']!=1:return False
                s=s['child']
        if s['kind']!='run' or ':until' in s['text']:return False
        rs=[r for r in rules.values() if r['ruleset']==s['ruleset']]
        return bool(rs) and all(not r['body'] for r in rs)
    def add_sort(s):
        if s in sorts and s not in defined:lines.extend(key_decl(s,comparisons));defined.add(s)
    for command in data['commands']:
        k=command['kind'];ordinal=command['ordinal'];src=texts[ordinal]
        if k=='rule':lines.extend(compiled[ordinal]);continue
        if k=='schedule':
            sched=command['schedule']
            if not frozen and initializer(sched):lines.append(src);continue
            if not frozen:
                lines.append(f'(run-schedule (seq (saturate {HELPER}) (run {FREEZE})))');frozen=True
            lines.append('(run-schedule '+schedule(sched,consumers)+')');continue
        if src.startswith('(function to-hoist '):
            cond='(cell-key-prefer-left (Expr-size old) (Expr-size new) (CellStructuralKey-Expr old) (CellStructuralKey-Expr new))'
            merge=f'(map-get (map-insert (map-insert (map-empty) true old) false new) {cond})'
            src=f'(function to-hoist (Expr Expr) Expr :merge {merge} :unextractable)'
        lines.append(src)
        if k=='declaration':
            for d in data['declarations']:
                if d['kind']=='sort' and (src.startswith('(sort '+d['name']+' ') or src=='(sort '+d['name']+')'):add_sort(d['name'])
                if d['kind']=='datatype' and src.startswith('(datatype '+d['name']+' '):add_sort(d['name'])
            present=[]
            if src.startswith(('(constructor ','(relation ')):present=[src.split()[1]]
            elif src.startswith('(datatype '):present=[v['name'] for d in data['declarations'] if d.get('name')==src.split()[1] for v in d.get('constructors',[])]
            elif src.startswith('(datatypes '):raise ValueError('expand mutually recursive datatypes with the flat exporter before conversion')
            for name in present:
                if name in constructors:
                    add_sort(constructors[name]['output']);lines.extend(constructor_rules(name,constructors[name],sorts));emitted.add(name)
    if emitted!=set(constructors):raise ValueError('unplaced native constructors: '+str(set(constructors)-emitted))
    result=HEADER+json.dumps(spec,separators=(',',':'))+'\n; All rules and maintenance schedules are explicit below. Requires the named native DAG extension.\n'+'\n'.join(lines)+'\n'
    (output/'source_sk.egg').write_text(result);(output/'source.egg').write_bytes(source.read_bytes())
    report=dict(schema='structural-key-source/v3',source=str(source),source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
        output_sha256=hashlib.sha256(result.encode()).hexdigest(),native_rulesets=[HELPER,FREEZE],policy=policy,
        key_roots=sorted(roots),key_sorts=sorted(sorts),key_constructors=len(constructors),declared_constructors=len(all_constructors),
        maintenance_before=sorted(consumers),runtime_extension='cell-structural-key-runtime/v1',
        representation='automatic-refutation Arc<Node> DAG, cached hashes plus exact equality, frozen input anchors',
        acceptance='source_sk == source_sk_mk; source.egg and old full-selection policy are not equivalence oracles',
        limits=['Requires the explicitly declared native runtime extension; not executable on bare pristine without registration.',
                'Default automatic policy changes to-hoist only; --policy full also changes known Smaller/IVT choices.',
                'Unsupported relevant container fields fail when instantiated.','No claim of arbitrary nonconfluent backend determinism.'])
    (output/'report.json').write_text(json.dumps(report,indent=2)+'\n');return report

if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--source',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    p.add_argument('--policy',choices=['automatic','full'],default='automatic')
    a=p.parse_args();print(json.dumps(compile_source(a.source,a.output,policy=a.policy)))
