"""Soundness contract for speculative, witnessed multi-stage batch recipes."""
from .recipe import digest,require


def validate_batch(plan,recipe,regions):
    require(plan['schema']=='eggfuse-batch-program/v1','unknown batch program')
    require(plan['sha256']==digest({k:v for k,v in plan.items() if k!='sha256'}),'batch checksum differs')
    require(plan['recipe_sha256']==recipe['sha256'],'batch source differs')
    subtrees=[]
    def walk(s):
        if 'saturate' in s:subtrees.append(s)
        for b in s.get('sequence',[]):walk(b)
        for k in ('body','saturate'):
            if k in s:walk(s[k])
    for s in recipe['control']:walk(s)
    def leaves(s):
        if 'run' in s:
            require(not any(s.get(k) for k in ('until_run','until','assert')),'batch crosses observable probe')
            return [s['run']]
        if 'sequence' in s:return [n for b in s['sequence'] for n in leaves(b)]
        require('saturate' in s,'batch crosses finite Repeat')
        return leaves(s['saturate'])
    for scope in plan['scopes']:
        require(scope['control'] in subtrees and scope['rulesets']==sorted(set(leaves(scope['control']))),'batch scope differs')
    for t in plan['templates']:
        require(0<=t['scope']<len(plan['scopes']) and t['steps'] and t['steps'][0]['rule']==t['seed'],'invalid batch seed/scope')
        allowed={r for s in regions['rulesets'] if s['name'] in plan['scopes'][t['scope']]['rulesets'] for r in s['rules']}
        defined=set(range(t['input_width']));producers={i:-1 for i in defined};used=set()
        seed=recipe['rules'][regions['rules'][t['seed']]['recipe_rule']]
        require(t['input_width']==sum(s['definition']=='query' for s in seed['slots']),'batch input width differs')
        for si,step in enumerate(t['steps']):
            rid=step['rule'];require(rid in allowed and rid not in used,'batch rule outside saturation scope or repeated');used.add(rid)
            require(step['recipe_rule']==regions['rules'][rid]['recipe_rule'],'batch rule mapping differs')
            r=recipe['rules'][step['recipe_rule']];mapping=step['mapping']
            require(len(mapping)==len(r['slots']),'batch mapping width differs')
            require(all(type(i) is int and 0<=i<si for i in step['requires']),'invalid local dependency')
            for i,(v,s) in enumerate(zip(mapping,r['slots'])):
                if 'slot' in v:
                    index=v['slot'];require(type(index) is int and 0<=index<len(t['slots']) and t['slots'][index]['sort']==s['sort'],'batch slot sort differs')
                    if si==0 and i<t['input_width']:require(index==i,'seed input wiring differs')
                else:require(v.get('sort')==s['sort'] and 'literal' in v,'batch literal type differs')
            table_slots={v['slot'] for a in r['query'] if 'table' in a['call'] for v in a['args'] if 'slot' in v}
            deps=set()
            for i,s in enumerate(r['slots']):
                v=mapping[i]
                if s['definition']=='query' and i in table_slots and 'slot' in v:
                    require(v['slot'] in defined,'query reads an undefined batch slot')
                    if producers[v['slot']]>=0:deps.add(producers[v['slot']])
            ancestors=set(step['requires']);todo=list(ancestors)
            while todo:
                for parent in t['steps'][todo.pop()]['requires']:
                    if parent not in ancestors:ancestors.add(parent);todo.append(parent)
            require(deps<=ancestors,'missing local producer dependency')
            for i,s in enumerate(r['slots']):
                v=mapping[i]
                if s['definition']!='query':
                    require('slot' in v and v['slot'] not in defined,'batch effect overwrites a live SSA slot')
                    defined.add(v['slot']);producers[v['slot']]=si
                elif 'slot' in v and v['slot'] not in defined:
                    require(i not in table_slots,'unbound table input')
                    defined.add(v['slot']);producers[v['slot']]=si
            # Successors are suggestions only. The generated merger rechecks
            # every table atom and primitive using actual post-merge identities.
            # No proposed set value or unvalidated local fact authorizes a step.
    return plan
