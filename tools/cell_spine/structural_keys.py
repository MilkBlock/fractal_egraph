"""Generate explicit .egg key maintenance for the shared native DAG extension.

Derive keys from constructor schemas. Never recognize an application/table name
or change an original merge/choice policy.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
from . import compiler as c, pass3_markers as pm
PREFIX='__sk_'
HELPER='cell-stable-keys'
FREEZE='cell-stable-freeze'
HEADER='; cell-structural-key-runtime/v1 '


def function(name,inputs,output,merge='old'):
    return f'(function {name} ({" ".join(inputs)}) {output} :merge {merge} :unextractable)'


def rule(body,head,ruleset=HELPER):
    return f'(rule ({" ".join(body)}) ({" ".join(head)}) :ruleset {ruleset})'


def key_decl(sort):
    fn='CellStructuralKey-'+sort
    lines=[function(fn,[sort],'CellStructuralKey','(cell-key-min old new)'),
        rule([f'(= {PREFIX}k ({fn} {PREFIX}x))'],[f'(set ({fn} {PREFIX}x) (cell-key-anchor {PREFIX}k))'],FREEZE)]
    return lines


def constructor_rules(name,sig,sorts):
    xs=[f'{PREFIX}x{i}' for i in range(len(sig['input']))]
    seed=f'(= {PREFIX}out ({name} {" ".join(xs)}))';body=[seed];keys=[]
    for i,(x,sort) in enumerate(zip(xs,sig['input'])):
        if sort in sorts:
            k=f'{PREFIX}k{i}';body.append(f'(= {k} (CellStructuralKey-{sort} {x}))');keys.append(k)
        elif sort in ('i64','String','f64','bool','Unit'):keys.append(f'(cell-key-base-{sort} {x})')
        else:raise ValueError("unsupported structural key field "+sort+" in "+name)
    return [rule(body,[f'(set (CellStructuralKey-{sig["output"]} {PREFIX}out) (cell-key-node-{name} {" ".join(keys)}))'])]


def schedule(s,consumers):
    k=s['kind']
    if k=='run':return f'(seq (saturate {HELPER}) {s["text"]})' if s['ruleset'] in consumers else s['text']
    if k=='seq':return '(seq '+' '.join(schedule(c,consumers) for c in s['children'])+')'
    return f'({k} '+(str(s['count'])+' ' if k=='repeat' else '')+schedule(s['child'],consumers)+')'


def compile_source(source,output,metadata_binary=None,ast_binary=None,policy='automatic'):
    if policy != 'automatic':raise ValueError('application-specific key policies were removed; choices must be explicit in source.egg')
    output=Path(output);output.mkdir(parents=True,exist_ok=False)
    source=Path(source).resolve();original=source.read_text()
    if any(s in original for s in [PREFIX,'CellStructuralKey','cell-key-','SKNeed','SKCompare',HELPER,FREEZE,HEADER]):raise ValueError('reserved structural-key namespace in input')
    binary=metadata_binary or c.ROOT/'target/analyzer/release/examples/structural_key_source'
    subprocess.run([str(binary),str(source),str(output/'native')],cwd=c.ROOT,check=True)
    meta=json.loads((output/'native/metadata.json').read_text())
    ast=ast_binary or c.ROOT/'target/analyzer/release/examples/lhs_ast'
    subprocess.run([str(ast),str(output/'native/source.egg'),str(output/'policy.json')],cwd=c.ROOT,check=True)
    data=json.loads((output/'policy.json').read_text());rules={r['command_ordinal']:r for r in data['rules']}
    texts=meta['source_commands']
    assert len(texts)==len(data['commands']), 'native command ordinals changed'
    all_constructors={n:s for n,s in meta['constructors'].items() if not s['output'].startswith('@')};sorts={s['output'] for s in all_constructors.values()}
    # A complete key needs every field to have a key. Exclude unsupported sort
    # closures rather than inserting a panic into an otherwise valid program.
    bases={'i64','f64','String','bool','Unit'};excluded={}
    while True:
        bad={sig['output'] for sig in all_constructors.values()
             if sig['output'] in sorts and any(t not in sorts|bases for t in sig['input'])}
        if not bad:break
        for t in bad:excluded[t]='constructor field has no portable structural key'
        sorts-=bad
    roots=set(sorts)
    constructors={name:sig for name,sig in all_constructors.items() if sig['output'] in sorts}
    supported={n:sig for n,sig in constructors.items() if all(s in sorts or s in ('i64','f64','String','bool','Unit') for s in sig['input'])}
    spec=dict(storage='shared-native-dag',policy=policy,native_rulesets=[HELPER,FREEZE],
        bases=sorted({s for sig in supported.values() for s in sig['input'] if s not in sorts}),
        nodes=[dict(primitive='cell-key-node-'+name,label=sig['output']+':'+name,arity=len(sig['input'])) for name,sig in sorted(supported.items())])
    compiled={};consumers=set()
    for ordinal,r in rules.items():
        compiled[ordinal]=[pm.render_rule(r,r['body'],r['head'])]
    consumers.update(data['rule_rulesets'])
    # Maintain keys at explicit source boundaries.
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
        if s in sorts and s not in defined:lines.extend(key_decl(s));defined.add(s)
    for command in data['commands']:
        k=command['kind'];ordinal=command['ordinal'];src=texts[ordinal]
        if k=='rule':lines.extend(compiled[ordinal]);continue
        if k=='schedule':
            sched=command['schedule']
            if not frozen and initializer(sched):lines.append(src);continue
            if not frozen:
                lines.append(f'(run-schedule (seq (saturate {HELPER}) (run {FREEZE})))');frozen=True
            lines.append('(run-schedule '+schedule(sched,consumers)+')');continue
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
    report=dict(schema='structural-key-source/v4',source=str(source),source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
        output_sha256=hashlib.sha256(result.encode()).hexdigest(),native_rulesets=[HELPER,FREEZE],policy=policy,
        key_roots=sorted(roots),key_sorts=sorted(sorts),key_constructors=len(constructors),declared_constructors=len(all_constructors),
        maintenance_before=sorted(consumers),runtime_extension='cell-structural-key-runtime/v1',
        representation='automatic-refutation Arc<Node> DAG, cached hashes plus exact equality, frozen input anchors',
        excluded_sorts=excluded, original_choices_preserved=True,
        acceptance='original facts/equality/visibility preserved; auxiliary structural-key tables may differ',
        limits=['Requires the explicitly declared native runtime extension; not executable on bare pristine without registration.',
                'No application-specific selection or merge rewrite.',
                'Sorts transitively depending on unsupported fields keep the original execution without structural keys.','No claim of arbitrary nonconfluent backend determinism.'])
    (output/'report.json').write_text(json.dumps(report,indent=2)+'\n');return report

if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--source',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    p.add_argument('--policy',choices=['automatic'],default='automatic')
    a=p.parse_args();print(json.dumps(compile_source(a.source,a.output,policy=a.policy)))
