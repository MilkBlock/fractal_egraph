"""Soundness contract for speculative, witnessed multi-stage batch recipes."""
from .recipe import digest,require


def validate_batch(plan,recipe,regions):
    require(plan['schema']=='eggfuse-batch-program/v2','unknown batch program')
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
    tables={t['name']:t for t in recipe['tables']}
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
            require(all(e['kind'] in ('copy','construct','compute','set','union') for e in r['effects']),'batch requires an unsupported or partial RHS operation')
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
            for receipt in step.get('receipts',[]):
                ei=receipt['effect'];require(type(ei) is int and 0<=ei<len(r['effects']),'invalid receipt effect')
                effect=r['effects'][ei];require(effect['kind']=='set','receipt is not a set result')
                sort=tables[effect['call']['table']]['schema'][-1]
                def fresh(v,sort):
                    require(set(v)=={'slot'} and type(v['slot']) is int and 0<=v['slot']<len(t['slots']),'invalid receipt slot')
                    idx=v['slot'];require(idx not in defined and t['slots'][idx]['sort']==sort,'receipt overwrites live slot or sort')
                    defined.add(idx);producers[idx]=si
                fresh(receipt['before'],sort);fresh(receipt['actual'],sort)
                for e in receipt['merge']:
                    call=e['call']
                    signature=tables[call['table']]['schema'] if 'table' in call else [dict(name=n,kind=n) for n in call['inputs']+[call['output']]]
                    require(len(signature)==len(e['args'])+1,'merge receipt arity differs')
                    for arg,sort_ in zip(e['args'],signature[:-1]):
                        require((arg['slot'] in defined and t['slots'][arg['slot']]['sort']==sort_) if 'slot' in arg else arg.get('sort')==sort_,'merge receipt uses unready/untyped input')
                    fresh(e['dst'],signature[-1])
            # Successors are suggestions only. The generated merger rechecks
            # every table atom and primitive using actual post-merge identities.
            # No proposed set value or unvalidated local fact authorizes a step.
    return plan


def certifiable(rule,tables):
    """Immutable binding with idempotent effects, not arbitrary function merge."""
    ts={t['name']:t for t in tables};atoms=[a for a in rule['query'] if 'table' in a['call']]
    if len(atoms)!=1 or ts[atoms[0]['call']['table']]['kind']!='constructor':return False
    for e in rule['effects']:
        if e['kind'] in ('copy','compute','construct'):continue
        if e['kind']=='set' and ts[e['call']['table']].get('merge') in ('(min old new)','(min new old)','(max old new)','(max new old)'):continue
        return False
    return True
