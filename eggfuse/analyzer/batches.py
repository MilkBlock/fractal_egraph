"""Compose uniquely wired rule invocations over one correlated seed context.

Only saturation scopes authorize early successor firing. Ambiguous wiring is
left to the exact residual matcher, never sampled. Rule dependency cycles are
not unfolded by this pass; the original GPU control loop handles recurrence.
"""
import copy
from eggfuse.ir.recipe import digest


def eligible(rule,tables):
    if any(s['sort']['kind'] in ('set','f64') for s in rule['slots']):return False
    for a in rule['query']:
        if 'primitive' in a['call'] and a['call']['primitive'] not in ('+','-','*','/','%','min','max','<','>','<=','>=','!=','guard','not','and','or','xor','=>','bool-=','bool-<','bool->','bool-<=','bool->='):return False
    for e in rule['effects']:
        k=e['kind']
        if k not in ('copy','construct','compute','set','union'):return False
        if k=='set':
            t=tables[e['call']['table']]
            if t['kind']!='function' or t.get('merge') not in ('(min old new)','(min new old)','(max old new)','(max new old)'):return False
        if k=='compute' and (e['call']['output']!='i64' or e['call']['primitive'] not in ('+','-','*','/','%','min','max')):return False
        if k=='union':return False # defer original union action phase; residual rule stays exact
    return True


def scopes(recipe):
    result=[];seen=set()
    def visit(s):
        if 'saturate' in s:
            names=[]
            def leaves(x):
                if 'run' in x:
                    if x.get('until_run') or x.get('until') or x.get('assert'):return False
                    names.append(x['run']);return True
                if 'sequence' in x:return all(leaves(b) for b in x['sequence'])
                if 'repeat' in x:return False
                return leaves(x['saturate'])
            if leaves(s['saturate']) and digest(s) not in seen:
                seen.add(digest(s));result.append(dict(control=copy.deepcopy(s),rulesets=sorted(set(names))))
        for b in s.get('sequence',[]):visit(b)
        for k in ('body','saturate'):
            if k in s:visit(s[k])
    for s in recipe['control']:visit(s)
    return result


def wire(rule,facts):
    """At most one complete symbolic substitution, with its fact producers.

    Finding a second distinct substitution proves ambiguity and is a semantic
    rejection, not a search budget. All runtime alternatives remain residual.
    """
    table_atoms=[a for a in rule['query'] if 'table' in a['call']]
    answers={}
    def recurse(i,env,deps):
        if len(answers)>1:return
        if i==len(table_atoms):
            answers[digest(env)]=(env,deps);return
        a=table_atoms[i]
        for f in facts:
            if a['call']['table']!=f['table']:continue
            e=dict(env);ok=True
            for want,actual in zip(a['args'],f['args']):
                if 'slot' not in want:
                    if want!=actual:ok=False;break
                elif want['slot'] in e and e[want['slot']]!=actual:ok=False;break
                else:e[want['slot']]=actual
            if ok:recurse(i+1,e,deps|{f['producer']})
    recurse(0,{},set())
    if len(answers)!=1:return None
    return next(iter(answers.values()))


def analyze(recipe,region_rules):
    tables={t['name']:t for t in recipe['tables']};rules=recipe['rules']
    scopes_=scopes(recipe);templates=[]
    for scope_id,scope in enumerate(scopes_):
        candidates=[r for r in region_rules if any(rs['name'] in scope['rulesets'] and r['recipe_rule'] in rs['rule_ids'] for rs in recipe['rulesets'])]
        # Original rule order is a deterministic priority, not a choice of one
        # representative binding. Only unique complete wiring is inlined.
        for seed in candidates:
            sr=rules[seed['recipe_rule']]
            if not eligible(sr,tables):continue
            width=sum(s['definition']=='query' for s in sr['slots'])
            if not width:continue
            slots=copy.deepcopy(sr['slots'][:width]);facts=[];steps=[];used=set()
            initial={i:{'slot':i} for i in range(width)}
            for a in sr['query']:
                if 'table' in a['call']:facts.append(dict(table=a['call']['table'],args=copy.deepcopy(a['args']),producer=-1))
            def append(r,env,deps):
                rule=rules[r['recipe_rule']];step=len(steps);mapping=dict(env)
                # Primitive results may be newly bound by the consumer's guard.
                for i,s in enumerate(rule['slots']):
                    if i not in mapping:
                        mapping[i]={'slot':len(slots)};slots.append(copy.deepcopy(s))
                def val(x):return copy.deepcopy(mapping[x['slot']]) if 'slot' in x else copy.deepcopy(x)
                outputs=[]
                for e in rule['effects']:
                    if e['kind'] in ('construct','set'):
                        args=list(map(val,e['args']))+[val({'slot':e['dst']}) if e['kind']=='construct' else val(e['value'])]
                        outputs.append(dict(table=e['call']['table'],args=args,producer=step))
                steps.append(dict(rule=r['id'],recipe_rule=r['recipe_rule'],mapping=[mapping[i] for i in range(len(rule['slots']))],requires=sorted(x for x in deps if x>=0)))
                # A set proposes a replacement for its exact symbolic key. Its
                # successor still verifies the *actual* post-merge value.
                for f in outputs:
                    facts[:]=[old for old in facts if not (old['table']==f['table'] and old['args'][:-1]==f['args'][:-1])]
                    facts.append(f)
                used.add(r['id'])
            append(seed,initial,set())
            progress=True
            while progress:
                progress=False
                for r in candidates:
                    if r['id'] in used or not eligible(rules[r['recipe_rule']],tables):continue
                    found=wire(rules[r['recipe_rule']],facts)
                    if not found or not any(d>=0 for d in found[1]):continue
                    env,deps=found
                    # Guards may compute Unit/i64 results, but every primitive
                    # input must already be bound by this correlated context.
                    known=set(env)
                    for a in rules[r['recipe_rule']]['query']:
                        if 'primitive' not in a['call']:continue
                        if any('slot' in v and v['slot'] not in known for v in a['args'][:-1]):break
                        if 'slot' in a['args'][-1]:known.add(a['args'][-1]['slot'])
                    else:
                        append(r,env,deps);progress=True
            if len(steps)>1:templates.append(dict(scope=scope_id,seed=seed['id'],input_width=width,slots=slots,steps=steps))
    payload=dict(schema='eggfuse-batch-program/v1',recipe_sha256=recipe['sha256'],scopes=scopes_,templates=templates,
        ordering='source seed binding ordinal, then recipe step and effect ordinal',
        correctness='exact seed query; every successor checks actual canonical rows/guards before effect publication',
        coverage='unique symbolic wiring only; ambiguous/cyclic/uncovered work keeps original matching')
    return dict(payload,sha256=digest(payload))
