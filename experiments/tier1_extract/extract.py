"""Extract existing Comb e-classes; no candidates, history recomposition or new rewrites."""
from collections import Counter,defaultdict
from pathlib import Path
import argparse
import hashlib
import json


def select(nodes):
    classes=defaultdict(list)
    for ident,n in nodes.items():
        if not n.get('subsumed',False):classes[n['eclass']].append(ident)
    costs={};chosen={}
    while True:
        changed=False
        for c,ids in classes.items():
            alternatives=[]
            for ident in ids:
                n=nodes[ident];children=[nodes[x]['eclass'] for x in n['children']]
                if all(x in costs for x in children):alternatives.append((1+sum(costs[x] for x in children),n['op'],tuple(children),ident))
            if not alternatives:continue
            best=min(alternatives)
            if c not in costs or best[0]<costs[c]:costs[c]=best[0];changed=True
            if best[0]==costs[c]:chosen[c]=best[-1]
        if not changed:break
    return chosen,costs


def extract(data,rule_dictionary=None):
    nodes=data['native_egraph']['nodes'];chosen,costs=select(nodes)
    roots={t['id'] for t in data['templates']}
    assert all(c in chosen for c in roots),'some Comb e-class has no finite extraction'
    usage=Counter(i['template'] for i in data['instances'])
    order=[];seen=set();active=set()
    def visit(c):
        if c in seen:return
        if c in active:raise ValueError('cyclic selected representation')
        active.add(c)
        for child in nodes[chosen[c]]['children']:visit(nodes[child]['eclass'])
        active.remove(c);seen.add(c)
        if c in roots:order.append(c)
    for c in sorted(roots):visit(c)
    names={c:f'$comb_{i:04d}' for i,c in enumerate(order)}
    def expression(c,reference=True):
        if reference and c in names:return names[c]
        n=nodes[chosen[c]]
        # Serialized primitive nodes already contain egglog literal syntax.
        if c.split('-',1)[0] in ['String','i64','f64','bool']:return n['op']
        return '('+n['op']+(' '+' '.join(expression(nodes[x]['eclass']) for x in n['children']) if n['children'] else '')+')'
    text=['; Direct extraction of the saved native tier-1 Comb e-classes.',
          '; One definition per class; parent Comb references are shared.',
          '; These are tier-1 combination expressions, NOT tier-0 rewrite rules.',
          '(include "experiments/tier1_effects/tier1_rule_comb_ir.egg")','']
    rule_dictionary=rule_dictionary or {}
    records=[]
    for c in order:
        n=nodes[chosen[c]];body=expression(c,False)
        meta={'eclass':c,'selected_native_node':chosen[c],'name':names[c],'kind':n['op'],'instance_references':usage[c],'tree_cost':costs[c]}
        if n['op'] in ['SmoothComb','CoarseComb']:
            rule_node=nodes[chosen[nodes[n['children'][1]]['eclass']]]
            assert rule_node['op']=='Rule'
            literal=nodes[chosen[nodes[rule_node['children'][0]]['eclass']]]['op']
            rule_id=json.loads(literal)
            if rule_dictionary:
                assert rule_id in rule_dictionary,rule_id
                meta['tier0_rule_id']=rule_id
                meta['tier0_definition']=rule_dictionary[rule_id]['definition']
        parents=[]
        def parent_refs(node_id):
            child=nodes[node_id]['eclass']
            if child in names:parents.append(names[child]);return
            for child_id in nodes[chosen[child]]['children']:parent_refs(child_id)
        if n['op'] in ['SmoothComb','CoarseComb']:parent_refs(n['children'][0])
        meta['parent_combs']=parents

        text.append('; @tier1-extract '+json.dumps(meta,ensure_ascii=False))
        if 'tier0_definition' in meta:
            text.append('; tier-0 source step (not a fused whole-comb rewrite):')
            text.extend('; '+line for line in meta['tier0_definition'].replace('\\n','\n').splitlines())
        text.append(f'(let {names[c]} {body})')
        records.append({**meta,'expression':body})
    assert len(records)==len(roots)==len(set(order))
    return '\n'.join(text)+'\n',{'scope':'direct extraction of native Math tier-1 templates; no composer or subgraph enumeration', 'capture':data.get('capture'),
        'selection':'minimum unit tree cost per e-class, deterministic tie-break; shared Comb definitions across roots, not a globally optimal DAG cost solver',
        'comb_eclasses':len(roots),'kind_counts':dict(Counter(r['kind'] for r in records)),
        'instance_count':len(data['instances']),'existing_enode_selection_verified':all(r['selected_native_node'] in nodes and nodes[r['selected_native_node']]['eclass']==r['eclass'] for r in records),
        'definitions':records}

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--input',default=str(Path(__file__).with_name('native_templates.json')));a=p.parse_args()
    path=Path(a.input);data=json.loads(path.read_text());rules_path=Path(__file__).with_name('tier0_rule_dictionary.json');rules=json.loads(rules_path.read_text()) if rules_path.exists() else {};text,report=extract(data,rules);out=Path(__file__).parent
    report['input_sha256']=hashlib.sha256(path.read_bytes()).hexdigest()
    (out/'existing_combs.egg').write_text(text)
    (out/'extraction.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
    print({k:report[k] for k in ['comb_eclasses','kind_counts','instance_count','existing_enode_selection_verified']})
