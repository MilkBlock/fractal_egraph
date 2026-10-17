"""Extract guarded parameter programs from egglog's parsed rewrite AST.

This is a restricted Cell-family analysis, not unrestricted closure inference.
Numeric-root rewrites are mandatory; unsupported ones fail closed. Other rewrites
are retained as native residual work. Literal values are never identity-erased.
"""
import hashlib
import json

ARITH = ('+', '-', '*')
CMP = ('>', '>=', '<', '<=', '==', '!=')
OPS = ('slot', 'lit') + ARITH + CMP
MIN, MAX = -(1 << 31), (1 << 31) - 1


def source_expr(x):
    if x['kind'] == 'var': return x['name']
    if x['kind'] == 'literal': return x['text']
    return '(' + x['head'] + (' ' if x['args'] else '') + ' '.join(map(source_expr, x['args'])) + ')'


def source_fact(x):
    if x['kind'] == 'eq': return '(= ' + ' '.join(map(source_expr, x['args'])) + ')'
    return source_expr(x['expr'])


def scalar(x, env):
    if x['kind'] == 'var':
        if x['name'] not in env: raise ValueError('unbound parameter: ' + x['name'])
        return env[x['name']]
    if x['kind'] == 'literal' and x['sort'] == 'i64':
        if not MIN <= x['value'] <= MAX: raise ValueError('literal outside exact int32 device domain')
        return ('lit', x['value'])
    if x['kind'] == 'call' and x['head'] in ARITH and len(x['args']) == 2:
        return (x['head'], *(scalar(a, env) for a in x['args']))
    raise ValueError('unsupported scalar expression: ' + source_expr(x))


def conditions(facts, env):
    """Resolve local definitions without discarding equality predicates."""
    pending = list(facts); guards = []
    while pending:
        later = []
        for f in pending:
            try:
                if f['kind'] == 'eq':
                    a, b = f['args']
                    if a['kind'] == 'var' and a['name'] not in env:
                        env[a['name']] = scalar(b, env)
                    elif b['kind'] == 'var' and b['name'] not in env:
                        env[b['name']] = scalar(a, env)
                    else: guards.append(('==', scalar(a, env), scalar(b, env)))
                else:
                    x = f['expr']
                    if x.get('head') not in CMP or len(x['args']) != 2:
                        raise ValueError('unsupported condition: ' + source_fact(f))
                    guards.append((x['head'], *(scalar(a, env) for a in x['args'])))
            except ValueError as e:
                if 'unbound parameter:' not in str(e): raise
                later.append(f)
        if len(later) == len(pending): raise ValueError('unresolved or cyclic guard definitions')
        pending = later
    return guards


def affine(e, width):
    """Optional mathematical view; execution keeps original overflow order."""
    op = e[0]
    if op == 'slot': return [int(i == e[1]) for i in range(width)] + [0]
    if op == 'lit': return [0] * width + [e[1]]
    if op not in ARITH: return None
    a, b = affine(e[1], width), affine(e[2], width)
    if a is None or b is None: return None
    if op == '+': return [x+y for x,y in zip(a,b)]
    if op == '-': return [x-y for x,y in zip(a,b)]
    if not any(a[:-1]): return [a[-1]*x for x in b]
    if not any(b[:-1]): return [b[-1]*x for x in a]
    return None


def substitute(e, parameters):
    if e[0] == 'slot': return parameters[e[1]]
    if e[0] == 'lit': return e
    return (e[0], *(substitute(x, parameters) for x in e[1:]))


class Tape:
    def __init__(self): self.ops = []; self.ids = {}

    def add(self, e):
        if e not in self.ids:
            op = e[0]
            args = [e[1], 0] if op in ('slot', 'lit') else [self.add(x) for x in e[1:]]
            self.ids[e] = len(self.ops)
            self.ops.append([OPS.index(op), *args])
        return self.ids[e]


class FamilyProgram:
    def __init__(self, ast):
        if ast['rules'] or ast['actions'] or ast['schedules'] or ast['checks']:
            raise ValueError('expected declarations and rewrites only')
        if any(c['kind'] not in ('declaration', 'rule') for c in ast['commands']):
            raise ValueError('unsupported command')
        self.ast = ast; self.templates = []; self.residual = []
        self.environment = hashlib.sha256(json.dumps([c['text'] for c in ast['commands']]).encode()).hexdigest()
        self.declarations = '\n'.join(c['text'] for c in ast['commands'] if c['kind'] == 'declaration')
        self.families = set()
        for r in ast['rewrites']:
            head = r['lhs'].get('head'); sig = ast['signatures'].get(head, {})
            if sig.get('kind') == 'constructor' and sig.get('input') and all(s == 'i64' for s in sig['input']):
                self.families.add(head)
        for r in ast['rewrites']:
            if r['subsume'] or r['bidirectional'] or r['ruleset']:
                raise ValueError('effectful or nondefault rewrite requires another contract')
            if r['lhs'].get('head') not in self.families:
                def introduces_family(x):
                    return x.get('head') in self.families or any(introduces_family(a) for a in x.get('args',[]))
                if introduces_family(r['rhs']):
                    raise ValueError('native residual can introduce an untracked numeric family')
                self.residual.append(r['source']['text']); continue
            self.templates.append(self.compile(r))
        self.by_family = {f: [i for i,t in enumerate(self.templates) if t['family'] == f]
                          for f in sorted(self.families)}

    def compile(self, rule):
        lhs = rule['lhs']; sig = self.ast['signatures'][lhs['head']]
        if any(x['kind'] != 'var' for x in lhs['args']):
            raise ValueError('numeric family LHS requires parameter slots')
        names = [x['name'] for x in lhs['args']]
        if len(set(names)) != len(names): raise ValueError('repeated numeric LHS parameter needs an explicit guard')
        env = {name: ('slot',i) for i,name in enumerate(names)}
        guards = conditions(rule['conditions'], env)
        leaves = []; children = []

        def rhs(x, path=()):
            s = self.ast['signatures'].get(x.get('head'), {})
            if s.get('kind') != 'constructor' or s['output'] != sig['output']:
                raise ValueError('RHS must be a constructor tree of the source sort')
            if len(x['args']) != len(s['input']): raise ValueError('constructor arity')
            args = []; params = []
            for i,(a,sort) in enumerate(zip(x['args'], s['input'])):
                if sort == 'i64':
                    e = scalar(a,env)
                    if e not in leaves: leaves.append(e)
                    args.append(('value',leaves.index(e))); params.append(e)
                elif sort == sig['output']: args.append(rhs(a,path+(i,)))
                else: raise ValueError('unsupported constructor field sort: '+sort)
            if x['head'] in self.families:
                children.append(dict(family=x['head'], path=list(path), parameters=params))
            return ('constructor',x['head'],*args)

        tree = rhs(rule['rhs'])
        tape = Tape(); guard_ids = [tape.add(g) for g in guards]; guard_end = len(tape.ops)
        # These are full logical bindings, including locally defined variables.
        bindings = list(env.values()); outputs = bindings + leaves
        output_ids = [tape.add(e) for e in outputs]
        for c in children:
            c['outputs'] = [len(bindings)+leaves.index(e) for e in c['parameters']]
            c['affine_rows'] = [affine(e,len(names)) for e in c['parameters']]
        return dict(id=len(self.templates), name=rule.get('name') or f'rewrite-{rule["command_ordinal"]}',
                    family=lhs['head'], arity=len(names), sort=sig['output'], input_names=names,
                    binding_names=list(env), bindings=bindings, guards=guards, children=children,
                    rhs=tree, leaf_offset=len(bindings), leaves=leaves, outputs=outputs,
                    tape=tape.ops, guard_end=guard_end, guard_ids=guard_ids, output_ids=output_ids,
                    source=rule['source']['text'], raw=rule)

    def compose(self, first, child_index, second):
        a,b = self.templates[first],self.templates[second]; child = a['children'][child_index]
        if child['family'] != b['family']: raise ValueError('child/source family mismatch')
        params = child['parameters']
        siblings = [dict(family=c['family'],path=c['path'],parameters=c['parameters'],affine_rows=c['affine_rows'])
                    for i,c in enumerate(a['children']) if i!=child_index]
        return dict(source=a['family'], via=child['family'], child_path=child['path'],
                    rules=[a['name'],b['name']],
                    guards=a['guards']+[substitute(g,params) for g in b['guards']],
                    children=siblings+[dict(family=c['family'], path=child['path']+c['path'],
                                   parameters=[substitute(e,params) for e in c['parameters']],
                                   affine_rows=[affine(substitute(e,params),a['arity']) for e in c['parameters']])
                              for c in b['children']],
                    retained_effects='both original rewrites, including the intermediate child')

    def recursive_components(self):
        edges = {f:set() for f in self.families}
        for t in self.templates: edges[t['family']].update(c['family'] for c in t['children'])
        reach = {f:set(v) for f,v in edges.items()}
        for k in edges:
            for f in edges:
                if k in reach[f]: reach[f].update(reach[k])
        groups = {tuple(sorted(g for g in edges if g in reach[f] and f in reach[g])) for f in edges}
        return [list(g) for g in sorted(groups) if g]

    def report(self):
        return dict(environment=self.environment, recursive_components=self.recursive_components(),
                    templates=[{k:v for k,v in t.items() if k!='raw'} for t in self.templates],
                    residual_native_rewrites=self.residual,
                    proof_scope='one-step AST lowering; SCCs are recursion candidates, not saturation certificates')
