"""Compile cross-rule local continuations from the lossless typed Recipe IR.

No observed endpoint or name-specific shortcut is assumed. A route projects
producer-local facts into a consumer binding; the runtime must establish that
each projected read is exhaustive in the current visible graph. Ambiguous
alias alternatives remain residual queries, never selected representatives.
"""
from collections import defaultdict
import json
from eggfuse.ir.recipe import digest

SCHEMA = 'eggfuse-multistage/v1'

def symbol(arg):
    return ('slot',arg['slot']) if 'slot' in arg else ('literal',json.dumps(arg,sort_keys=True,separators=(',',':')))

def sources(rule, tables):
    copies = {e['dst']: e['arg'] for e in rule['effects'] if e['op'] == 'copy'}
    def normalized(arg):
        while 'slot' in arg and arg['slot'] in copies: arg = copies[arg['slot']]
        return arg
    rows = []
    for i, atom in enumerate(rule['query']):
        if 'table' in atom['call']:
            rows.append(dict(kind='query', index=i, table=atom['call']['table'], args=[normalized(a) for a in atom['args']]))
    for i, effect in enumerate(rule['effects']):
        table = effect.get('call', {}).get('table')
        if not table: continue
        t = tables[table]
        virtual = t['kind'] == 'constructor' or (t['schema'][-1]['kind'] == 'Unit' and t.get('merge_expr') is None)
        if effect['op'] == 'let' and virtual:
            args = effect['args'] + [dict(slot=effect['dst'])]
        elif effect['op'] == 'set':
            args = effect['args'] + [effect['value']]
        else: continue
        rows.append(dict(kind='effect', index=i, table=table, args=[normalized(a) for a in args]))
    return rows

def extend(atom, source, mapping):
    if atom['call'].get('table') != source['table']: return None
    result = dict(mapping)
    for a, b in zip(atom['args'], source['args']):
        if 'slot' in a:
            if a['slot'] in result and result[a['slot']] != b: return None
            result[a['slot']] = b
        elif a != b: return None
    return result

def build(recipe):
    rules = recipe['rules']; tables = {t['name']: t for t in recipe['tables']}
    readers = defaultdict(list)
    for rid, rule in enumerate(rules):
        for atom, a in enumerate(rule['query']):
            if 'table' in a['call']: readers[a['call']['table']].append((rid, atom))
    routes = []
    loaders=[]
    for producer, rule in enumerate(rules):
        if not any('table' in a['call'] for a in rule['query']):
            loaders.append(producer);continue
        local = sources(rule, tables)
        by_table = defaultdict(list)
        postings=defaultdict(list)
        for source in local:
            by_table[source['table']].append(source)
            for col,arg in enumerate(source['args']):postings[source['table'],col,symbol(arg)].append(source)
        for anchor in local:
            if anchor['kind'] != 'effect': continue
            # A function anchor is its committed result port, never the proposal.
            # The runtime keeps the current producer frame and rechecks reads.
            for consumer, atom in readers[anchor['table']]:
                target = rules[consumer]
                mapping = extend(target['query'][atom], anchor, {})
                if mapping is None: continue
                steps = []; remaining = set(range(len(target['query']))) - {atom}
                while True:
                    progress = False
                    for ai in sorted(remaining):
                        a = target['query'][ai]
                        # Only use a single structural projection. Multiple possible
                        # provider rows remain explicit; no heuristic representative.
                        table=a['call'].get('table')
                        constraints=[postings.get((table,col,symbol(mapping[v['slot']] if 'slot' in v else v)),[]) for col,v in enumerate(a['args']) if 'slot' not in v or v['slot'] in mapping]
                        # Disconnected query components belong to the residual
                        # domain, not to this producer's local context.
                        if not constraints:continue
                        possible=min(constraints,key=len)
                        options = [(s, m) for s in possible if (m := extend(a, s, mapping)) is not None]
                        unique = {}
                        for s, m in options: unique.setdefault(tuple(map(symbol,s['args'])), (s, m))
                        if len(unique) != 1: continue
                        s, mapping = next(iter(unique.values()))
                        steps.append(dict(atom=ai, source_kind=s['kind'], source_index=s['index']))
                        remaining.remove(ai); progress = True
                    if not progress: break
                # A multi-input consumer with no local projection gets exactly
                # the ordinary delta path. Do not build a redundant context.
                if not steps and sum('table' in a['call'] for a in target['query'])!=1: continue
                routes.append(dict(producer=producer, effect=anchor['index'], consumer=consumer, anchor=atom,
                    steps=steps, residual=sorted(remaining)))
    def points(rule, known, remaining):
        order=[]
        while remaining:
            def priority(i):
                a=rule['query'][i];full=all('slot' not in v or v['slot'] in known for v in a['args'][:-1])
                if full:return 0 if 'primitive' in a['call'] else 1
                table=a['call'].get('table');out=a['args'][-1]
                if table and tables[table]['schema'][-1]['kind']=='eq' and 'slot' in out and out['slot'] in known:return 2
                return None
            ready=[i for i in remaining if priority(i) is not None]
            if not ready: break
            i=min(ready,key=lambda i:(priority(i),i))
            order.append(i);remaining.remove(i)
            known.update(a['slot'] for a in rule['query'][i]['args'] if 'slot' in a)
        return order
    entries=[];finishes=[]
    for rid,rule in enumerate(rules):
        table_atoms=[i for i,a in enumerate(rule['query']) if 'table' in a['call']]
        for anchor in table_atoms:
            known={a['slot'] for a in rule['query'][anchor]['args'] if 'slot' in a}
            order=points(rule,known,set(range(len(rule['query'])))-{anchor})
            if order:entries.append(dict(rule=rid,anchor=anchor,atoms=order))
        known={a['slot'] for i in table_atoms for a in rule['query'][i]['args'] if 'slot' in a}
        finishes.append(points(rule,known,set(range(len(rule['query'])))-set(table_atoms)))
    # Rule nodes and these edges form a finite shared recipe graph, including SCCs.
    # Repeat/Saturate stay as control instructions, never copied into unbounded paths.
    return dict(schema=SCHEMA, recipe_sha256=recipe['sha256'], control=recipe['control'],
        rule_sets=[dict(name=s['name'],rule_ids=s['rule_ids']) for s in recipe['rulesets']], routes=routes, entries=entries, finishes=finishes, data_loaders=loaders,
        contract=dict(activation='committed producer fact; current visible exhaustive projection',
            publication='original Run occurrence, after all entry bindings are gathered',
            late_input='ordinary delta route remains authoritative',
            alias_alternatives='residual matching whenever a projection is not exhaustive'))
