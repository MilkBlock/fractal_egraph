"""Source-declared key uses and backward type liveness; no application names.

Order-sensitive reference selectors are suggestions, not authority to change a
program. Only cell-choice/v1 declarations activate structural ordering.
"""
from collections import defaultdict, deque
import json
from .terms import text, walk

CHOICE = '; cell-choice/v1 '
BASES = {'i64', 'f64', 'String', 'bool', 'Unit'}


def declarations(source):
    result = {}
    for line in source.splitlines():
        if not line.startswith(CHOICE):
            continue
        value = json.loads(line[len(CHOICE):])
        if not isinstance(value, dict) or set(value) - {'function', 'score', 'order', 'tie_break'}:
            raise ValueError('invalid cell-choice/v1 fields')
        if not isinstance(value.get('function'), str) or value.get('tie_break') != 'structural':
            raise ValueError('cell-choice requires a function and structural tie_break')
        if 'score' in value and (not isinstance(value['score'], str) or not value['score']):
            raise ValueError('choice score must name a unary i64 function')
        if value.get('order', 'max') not in ('min', 'max'):
            raise ValueError('choice order must be min/max')
        if value['function'] in result:
            raise ValueError('duplicate cell-choice declaration')
        result[value['function']] = dict(value, order=value.get('order', 'max'))
    return result


def active_sets(data):
    active = set()
    def scan(s):
        if s['kind']=='repeat' and s['count']==0:return
        if s['kind'] == 'run': active.add(s['ruleset'])
        for c in s.get('children', []): scan(c)
        if 'child' in s: scan(s['child'])
    for c in data['commands']:
        if c['kind'] == 'schedule': scan(c['schedule'])
    todo = list(active)
    while todo:
        for child in data['combined_rulesets'].get(todo.pop(), []):
            if child not in active: active.add(child); todo.append(child)
    return active


def sets_function(action, name):
    return action['kind'] == 'set' and action['head'] == name


def ranked_guards(rule, action, signatures):
    """Suggest score(v) = best(keys), where best has a primitive min/max merge.

    This is evidence of a ranked-choice shape, not a proof that overwriting or
    its temporal behavior may be replaced. The source declaration is required.
    """
    found = []
    for fact in rule['body']:
        if fact['kind'] != 'eq': continue
        for rank, best in [fact['args'], list(reversed(fact['args']))]:
            if rank['kind'] != 'call' or best['kind'] != 'call': continue
            if len(rank['args']) != 1 or text(rank['args'][0]) != text(action['value']): continue
            if list(map(text, best['args'])) != list(map(text, action['args'])): continue
            r, b = signatures.get(rank['head'], {}), signatures.get(best['head'], {})
            m = b.get('merge') or {}
            if r.get('output') != 'i64' or b.get('output') != 'i64': continue
            if m.get('kind') != 'call' or m.get('head') not in ('min', 'max') or m['head'] in signatures: continue
            if sorted(text(x) for x in m['args']) != ['new', 'old']: continue
            found.append(dict(score=rank['head'], order=m['head'], best=best['head']))
    return found


def analyze(data, constructors, source):
    requested = declarations(source)
    sorts = {s['output'] for s in constructors.values()}
    active = active_sets(data)
    candidates = []; by_name = {}; writers = defaultdict(list)
    for rule in data['rules']:
        for action in rule['head']:
            if action['kind'] == 'set': writers[action['head']].append((rule, action))
    for name, sig in data['signatures'].items():
        merge = sig.get('merge') or {}
        if sig['kind'] != 'function' or sig['output'] not in sorts:
            continue
        if merge.get('kind') != 'var' or merge.get('name') not in ('old', 'new'):
            continue
        item = dict(function=name, output=sig['output'], merge=merge['name'],
            writers=sorted({r['ruleset'] for r, _ in writers[name]}),
            ranked_guards=[g for r, a in writers[name] for g in ranked_guards(r, a, data['signatures'])],
            authority='suggestion only; original merge preserved unless source opts in')
        candidates.append(item); by_name[name] = item
    positions={c['text'].split()[1]:c['ordinal'] for c in data['commands'] if c['text'].startswith('(function ')}
    selected = []; consumers = set(); write_rules = set(); top_writes = {}
    for name, choice in requested.items():
        if name not in by_name:
            raise ValueError('choice must target an old/new reference selector: ' + name)
        sig = data['signatures'][name]
        if choice.get('score'):
            for r in data['rules']:
                if r['ruleset'] in active and any(a['kind']=='change' and a['head']==choice['score'] for a in r['head']):
                    raise ValueError('selected score may be deleted/subsumed: '+choice['score'])
            if any(a['action']['kind']=='change' and a['action']['head']==choice['score'] for a in data['actions']):
                raise ValueError('selected score may be deleted/subsumed: '+choice['score'])
            if positions.get(choice['score'],len(data['commands'])) >= positions[name]:
                raise ValueError('choice score must be declared before its selector: '+choice['score'])
            score = data['signatures'].get(choice['score'], {})
            if score.get('input') != [sig['output']] or score.get('output') != 'i64':
                raise ValueError('choice score must have signature (' + sig['output'] + ') i64')
        live = [(r, a) for r, a in writers[name] if r['ruleset'] in active]
        inputs = [a for a in data['actions'] if sets_function(a['action'], name)]
        # Key availability precedes the original frozen Run. Creating a fresh
        # RHS term in that prepass could enable additional original matches.
        for r, a in live:
            bound = {v['name'] for f in r['body'] for e in (f['args'] if f['kind']=='eq' else [f['expr']])
                     for v in walk(e) if v['kind']=='var'}
            if a['value']['kind'] != 'var' or a['value']['name'] not in bound:
                raise ValueError('structural choice needs a candidate bound in its LHS; fresh RHS candidate in ' + r['name'])
            consumers.add(r['ruleset']); write_rules.add(r['command_ordinal'])
        for a in inputs: top_writes[a['command_ordinal']] = a['action']
        if live or inputs:
            selected.append(dict(choice, sort=sig['output'], original_merge=by_name[name]['merge']))
    if selected and any(r.get('naive') and r['ruleset'] in active for r in data['rules']):
        raise ValueError('key maintenance is not certified for active naive rules')
    if requested and any(c['text'].startswith('(fail ') and ('(run' in c['text'] or '(set ' in c['text']) for c in data['commands']):
        raise ValueError('source-declared key ordering does not support nested fail schedules/writes')
    # Type closure is a worklist over constructor-field edges. Recursive sorts
    # are visited once. Needed unsupported fields fail, never get fake keys.
    dependencies = defaultdict(set); edges=defaultdict(list)
    for constructor,sig in constructors.items():
        dependencies[sig['output']].update(sig['input'])
        for position,child in enumerate(sig['input']):
            edges[sig['output'],child].append(dict(constructor=constructor,position=position))
    roots = {c['sort'] for c in selected}; live_sorts = set(roots); queue = deque(sorted(roots))
    reasons = {s:dict(kind='choice', functions=[c['function'] for c in selected if c['sort']==s]) for s in roots}
    while queue:
        parent = queue.popleft()
        for child in sorted(dependencies[parent]):
            if child in BASES: continue
            if child not in sorts: raise ValueError(f'live structural key {parent} requires unsupported field {child}')
            if child not in live_sorts:
                live_sorts.add(child); queue.append(child)
                reasons[child] = dict(kind='constructor_field', parent=parent,fields=edges[parent,child])
    return dict(candidates=candidates, selected=selected, roots=sorted(roots), sorts=sorted(live_sorts),
        reasons=reasons, consumers=sorted(consumers), write_rules=sorted(write_rules), top_writes=top_writes,
        ignored_declarations=sorted(set(requested)-{c['function'] for c in selected}),
        guarantee='only source-declared ordering reads root the key dependency graph')
