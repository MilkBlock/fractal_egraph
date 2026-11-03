"""Materialize structural keys only for source-declared selection consumers.

Unannotated old/new merges keep their semantics. Type liveness follows constructor
fields; value demand follows actual candidates and their constructor children.
"""
import argparse
import hashlib
import json
import re
from pathlib import Path
import subprocess
from . import compiler as c, pass3_markers as pm, key_demand, takeover as t
from .terms import text, egg_string
PREFIX='__sk_'
HELPER='cell-stable-keys'
VALIDATE='cell-key-ready-check'
REQUEST='cell-key-request-'
HEADER='; cell-structural-key-runtime/v1 '


def function(name,inputs,output,merge='old'):
    return f'(function {name} ({" ".join(inputs)}) {output} :merge {merge} :unextractable)'


def rule(body,head,ruleset=HELPER):
    return f'(rule ({" ".join(body)}) ({" ".join(head)}) :ruleset {ruleset})'


def need(choice,value):
    i=choice['index'];sort=choice['sort']
    return [f'(CellChoiceNeed{i} {value})',f'(set (CellChoiceReady{i} {value}) false)',f'(CellKeyNeed-{sort} {value})']


def key_decl(sort):
    fn='CellStructuralKey-'+sort
    return [function(fn,[sort],'CellStructuralKey','(cell-key-min old new)'),
            f'(relation CellKeyNeed-{sort} ({sort}))',
            rule([f'(= {PREFIX}k ({fn} {PREFIX}x))'],
                 [f'(set ({fn} {PREFIX}x) (cell-key-anchor {PREFIX}k))'])]


def constructor_rules(name,sig,sorts):
    xs=[f'{PREFIX}x{i}' for i in range(len(sig['input']))]
    body=[f'(CellKeyNeed-{sig["output"]} {PREFIX}out)',f'(= {PREFIX}out ({name} {" ".join(xs)}))']
    key_body=list(body);keys=[];children=[]
    for i,(x,sort) in enumerate(zip(xs,sig['input'])):
        if sort in sorts:
            k=f'{PREFIX}k{i}';key_body.append(f'(= {k} (CellStructuralKey-{sort} {x}))');keys.append(k)
            children.append(f'(CellKeyNeed-{sort} {x})')
        elif sort in key_demand.BASES:keys.append(f'(cell-key-base-{sort} {x})')
        else:raise ValueError('unsupported key field '+sort+' in '+name)
    return ([rule(body,children)] if children else [])+[
        rule(key_body,[f'(set (CellStructuralKey-{sig["output"]} {PREFIX}out) (cell-key-node-{name} {" ".join(keys)}))'])]


def choice_helpers(choice,sig):
    i=choice['index'];sort=choice['sort'];v=PREFIX+'choice';keys=[PREFIX+f'arg{j}' for j in range(len(sig['input']))]
    body=[f'(CellChoiceNeed{i} {v})',f'(= {PREFIX}key (CellStructuralKey-{sort} {v}))']
    if choice.get('score'):body.append(f'(= {PREFIX}score ({choice["score"]} {v}))')
    return [f'(relation CellChoiceNeed{i} ({sort}))',function(f'CellChoiceReady{i}',[sort],'bool','(or old new)'),
        rule([f'(= {v} ({choice["function"]} {" ".join(keys)}))'],need(choice,v)),
        rule(body,[f'(set (CellChoiceReady{i} {v}) true)']),
        rule([f'(CellChoiceNeed{i} {v})',f'(= (CellChoiceReady{i} {v}) false)'],
             [f'(panic {egg_string("unavailable structural key or score for "+choice["function"])})'],VALIDATE)]


def compile_source(source,output,metadata_binary=None,ast_binary=None,policy='automatic'):
    if policy!='automatic':raise ValueError('application-specific key policies were removed; choices must be explicit in source.egg')
    output=Path(output);output.mkdir(parents=True,exist_ok=False)
    source=Path(source).resolve();original=source.read_text()
    reserved=[PREFIX,'CellStructuralKey','cell-key-','CellKeyNeed-','CellChoiceMap','CellChoiceNeed','CellChoiceReady',HELPER]
    # Declarations are comments, not new egglog syntax. No source name is a
    # builtin consumer; the ordinary AST must not collide with generated names.
    if any(s in original for s in reserved):raise ValueError('reserved structural-key namespace in input')
    binary=metadata_binary or c.ROOT/'target/analyzer/release/examples/structural_key_source'
    subprocess.run([str(binary),str(source),str(output/'native')],cwd=c.ROOT,check=True)
    meta=json.loads((output/'native/metadata.json').read_text())
    ast=ast_binary or c.ROOT/'target/analyzer/release/examples/lhs_ast'
    subprocess.run([str(ast),str(output/'native/source.egg'),str(output/'policy.json')],cwd=c.ROOT,check=True)
    data=json.loads((output/'policy.json').read_text());rules={r['command_ordinal']:r for r in data['rules']}
    texts=meta['source_commands'];assert len(texts)==len(data['commands'])
    all_constructors={n:s for n,s in meta['constructors'].items() if not s['output'].startswith('@')}
    demand=key_demand.analyze(data,all_constructors,original)
    roots=set(demand['roots']);sorts=set(demand['sorts'])
    constructors={n:s for n,s in all_constructors.items() if s['output'] in sorts}
    choices={x['function']:dict(x,index=i) for i,x in enumerate(demand['selected'])}
    request_sets={s:REQUEST+str(i) for i,s in enumerate(demand['consumers'])}
    native_sets=[HELPER,VALIDATE]+list(request_sets.values()) if sorts else []
    spec=dict(storage='shared-native-dag',policy=policy,native_rulesets=native_sets,choices=demand['selected'],
        key_semantics='first complete constructor witness is anchored; union merges established keys by min; not an eclass canonical form',
        bases=sorted({t for s in constructors.values() for t in s['input'] if t not in sorts}),
        nodes=[dict(primitive='cell-key-node-'+n,label=s['output']+':'+n,arity=len(s['input'])) for n,s in sorted(constructors.items())])
    # Combined rulesets request all their member writers before their one frozen
    # Run. Requests write only auxiliary tables, never original constructor facts.
    consumers={s:{name} for s,name in request_sets.items()}
    changed=True
    while changed:
        changed=False
        for parent,children in data['combined_rulesets'].items():
            needed=set().union(*(consumers.get(s,set()) for s in children))
            if needed and needed!=consumers.get(parent):consumers[parent]=needed;changed=True
    def schedule(s):
        k=s['kind']
        if k=='run':
            requests=consumers.get(s['ruleset'],set())
            return ('(seq '+' '.join(f'(run {r})' for r in sorted(requests))+f' (saturate {HELPER}) (run {VALIDATE}) '+s['text']+')') if requests else s['text']
        if k=='seq':return '(seq '+' '.join(schedule(x) for x in s['children'])+')'
        return '('+k+' '+(str(s['count'])+' ' if k=='repeat' else '')+schedule(s['child'])+')'
    lines=[f'(ruleset {s})' for s in native_sets];defined=set();emitted=set()
    def add_sort(s):
        if s in sorts and s not in defined:lines.extend(key_decl(s));defined.add(s)
    for command in data['commands']:
        k=command['kind'];ordinal=command['ordinal'];src=texts[ordinal]
        if k=='rule':
            r=rules[ordinal];lines.append(pm.render_rule(r,r['body'],r['head']))
            if r['ruleset'] in request_sets:
                heads=[h for a in r['head'] if a['kind']=='set' and a['head'] in choices for h in need(choices[a['head']],text(a['value']))]
                if heads:lines.append(rule([t.fact_text(f) for f in r['body']],heads,request_sets[r['ruleset']]))
            continue
        if k=='schedule':lines.append('(run-schedule '+schedule(command['schedule'])+')');continue
        if ordinal in demand['top_writes']:
            action=demand['top_writes'][ordinal];args=[];choice=choices[action['head']]
            for i,e in enumerate(action['args']+[action['value']]):
                name=f'{PREFIX}input{ordinal}_{i}';lines.append(f'(let {name} {text(e)})');args.append(name)
            lines.extend(need(choice,args[-1]))
            lines.append(f'(run-schedule (seq (saturate {HELPER}) (run {VALIDATE})))')
            lines.append(f'(set ({action["head"]} {" ".join(args[:-1])}) {args[-1]})')
            continue
        choice=None
        if k=='declaration' and src.startswith('(function '):
            choice=choices.get(src.split()[1])
            if choice:
                sort=choice['sort'];score=choice.get('score')
                a,b=(f'({score} old)',f'({score} new)') if score else ('0','0')
                if choice['order']=='min':a,b=b,a
                cond=f'(cell-key-prefer-left {a} {b} (CellStructuralKey-{sort} old) (CellStructuralKey-{sort} new))'
                merge=f'(cell-key-select old new {cond})'
                src,count=re.subn(r':merge '+choice['original_merge']+r'(?=[ )])',':merge '+merge,src)
                if count!=1:raise ValueError('cannot locate selected merge: '+choice['function'])
        lines.append(src)
        if choice:lines.extend(choice_helpers(choice,data['signatures'][choice['function']]))
        if k=='declaration':
            for d in data['declarations']:
                if d['kind']=='sort' and (src.startswith('(sort '+d['name']+' ') or src=='(sort '+d['name']+')'):add_sort(d['name'])
                if d['kind']=='datatype' and src.startswith('(datatype '+d['name']+' '):add_sort(d['name'])
            present=[]
            if src.startswith('(constructor '):present=[src.split()[1]]
            elif src.startswith('(datatype '):present=[v['name'] for d in data['declarations'] if d.get('name')==src.split()[1] for v in d.get('constructors',[])]
            elif src.startswith('(datatypes ') and sorts:raise ValueError('expand mutually recursive datatypes with the flat exporter before conversion')
            for name in present:
                if name in constructors:
                    add_sort(constructors[name]['output']);lines.extend(constructor_rules(name,constructors[name],sorts));emitted.add(name)
    if emitted!=set(constructors):raise ValueError('unplaced native constructors: '+str(set(constructors)-emitted))
    # An empty key pass must not perturb an already-expanded program's names,
    # variable order or query layout. Surface rewrites still use the existing
    # normalization needed by the next source-marker compiler.
    preserved=False
    if not choices:
        subprocess.run([str(ast),str(source),str(output/'original-ast.json')],cwd=c.ROOT,check=True)
        preserved=not json.loads((output/'original-ast.json').read_text())['rewrites']
    body=original if preserved else '\n'.join(lines)+'\n'
    result=HEADER+json.dumps(spec,separators=(',',':'))+'\n'+body
    (output/'source_sk.egg').write_text(result);(output/'source.egg').write_bytes(source.read_bytes())
    report=dict(schema='structural-key-source/v5',source=str(source),source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
        output_sha256=hashlib.sha256(result.encode()).hexdigest(),native_rulesets=native_sets,policy=policy,
        key_roots=sorted(roots),key_sorts=sorted(sorts),key_constructors=len(constructors),declared_constructors=len(all_constructors),
        maintenance_before=sorted(consumers),runtime_extension='cell-structural-key-runtime/v1',key_demand=demand,
        representation='anchored constructor-witness keys; candidate-rooted demand and positive readiness validation',
        original_choices_preserved=not choices,original_body_preserved=preserved,pruned_key_sorts=sorted({s['output'] for s in all_constructors.values()}-sorts),selected_choices=list(choices),
        acceptance=('original facts/equality/visibility preserved' if not choices else 'explicit source selection policy; compare SK with its marker lowering'),
        limits=['No implicit selection policy; suggestions do not activate keys.',
            'Needed unsupported fields and fresh RHS candidates reject compilation.',
            'Demand includes all currently eligible candidates, not only numeric ties.',
            'Unavailable finite key witnesses or scores fail before the original choice Run.',
            'No claim of arbitrary nonconfluent backend determinism.'])
    (output/'report.json').write_text(json.dumps(report,indent=2)+'\n');return report

if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--source',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    p.add_argument('--policy',choices=['automatic'],default='automatic')
    a=p.parse_args();print(json.dumps(compile_source(a.source,a.output,policy=a.policy)))
