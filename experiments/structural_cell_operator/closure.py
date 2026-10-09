"""Compile constructor/union closure to exact boundary-condition polynomials.

The arena holds typed ground terms, NOT numerical encodings of e-class IDs.
Presence and equality have separate polynomial coordinates. Source rewrites,
congruence, and transitivity update those coordinates to their least fixed point.
This finite experiment rejects functions, guards, deletion and non-rewrite rules.
"""
from collections import defaultdict
from dataclasses import dataclass

from polynomial import Poly

ZERO = Poly.const(0)
ONE = Poly.const(1)


@dataclass(frozen=True)
class Term:
    sort: str
    op: str
    args: tuple = ()
    literal: str | None = None


class Closure:
    def __init__(self, signatures, rules):
        self.signatures = signatures
        self.rules = rules
        self.terms = []
        self.index = {}
        self.by_op = defaultdict(list)
        self.present = []
        self.equal = {}
        self.changed = False
        self.rule_effects = defaultdict(set)
        for rule in rules:
            body = rule['body']
            if len(body) != 1 or body[0]['kind'] != 'eq':
                raise ValueError('only source constructor rewrites are supported')
            a, b = body[0]['args']
            if a['kind'] != 'var' or b['kind'] != 'call':
                raise ValueError('expected (= root (Constructor ...))')
            self.check_pattern(b)
            if any(action['kind'] != 'union' for action in rule['head']):
                raise ValueError('only constructor-building union effects are supported')
            for action in rule['head']:
                for expr in action['args']: self.check_pattern(expr)

    def check_pattern(self, expr):
        if expr['kind'] in {'var', 'literal'}: return
        sig = self.signatures.get(expr['head'])
        if not sig or sig['kind'] != 'constructor':
            raise ValueError(f'non-constructor operation: {expr["head"]}')
        if len(sig['input']) != len(expr['args']): raise ValueError('constructor arity')
        for child in expr['args']: self.check_pattern(child)

    def intern(self, term):
        if term in self.index: return self.index[term]
        key = len(self.terms)
        self.index[term] = key
        self.terms.append(term)
        self.present.append(ONE if term.literal is not None else ZERO)
        self.by_op[term.op].append(key)
        self.changed = True
        return key

    def insert(self, key, condition):
        update = self.present[key] | condition
        if update != self.present[key]:
            self.present[key] = update
            self.changed = True

    def equality(self, a, b):
        if self.terms[a].sort != self.terms[b].sort: return ZERO
        if a == b: return self.present[a]
        if self.terms[a].literal is not None or self.terms[b].literal is not None:
            return ZERO
        return self.equal.get(tuple(sorted((a, b))), ZERO)

    def union(self, a, b, condition):
        if self.terms[a].sort != self.terms[b].sort: raise ValueError('cross-sort union')
        if self.terms[a].literal is not None or self.terms[b].literal is not None:
            raise ValueError('primitive union is not supported')
        if a == b or condition.is_zero(): return
        pair = tuple(sorted((a, b)))
        old = self.equal.get(pair, ZERO)
        update = old | condition
        if update != old:
            self.equal[pair] = update
            self.changed = True

    def construct(self, expr, env, condition):
        if expr['kind'] == 'var':
            key = env[expr['name']]
            if not (condition * (ONE - self.present[key])).is_zero():
                raise ValueError('RHS refers to an unavailable variable')
            return key
        if expr['kind'] == 'literal':
            return self.intern(Term(expr['sort'], 'literal', literal=expr['text']))
        sig = self.signatures[expr['head']]
        args = tuple(self.construct(e, env, condition) for e in expr['args'])
        if [self.terms[a].sort for a in args] != sig['input']: raise ValueError('argument sort')
        key = self.intern(Term(sig['output'], expr['head'], args))
        self.insert(key, condition)
        return key

    def action(self, action, condition=ONE, env=None):
        if condition.is_zero(): return
        env = env or {}
        if action['kind'] == 'expr':
            self.construct(action['expr'], env, condition)
        elif action['kind'] == 'union':
            a, b = [self.construct(e, env, condition) for e in action['args']]
            self.union(a, b, condition)
        else:
            raise ValueError(f'unsupported external action: {action["kind"]}')

    def match(self, pattern, target, env, condition):
        if condition.is_zero(): return []
        if pattern['kind'] == 'var':
            name = pattern['name']
            if name in env:
                condition = condition * self.equality(env[name], target)
                return [] if condition.is_zero() else [(env, condition)]
            return [({**env, name: target}, condition)]
        if pattern['kind'] == 'literal':
            term = self.terms[target]
            return [(env, condition)] if (term.sort, term.literal) == (pattern['sort'], pattern['text']) else []
        answers = []
        for node in list(self.by_op[pattern['head']]):
            candidates = [(env, condition * self.equality(target, node))]
            for child, arg in zip(pattern['args'], self.terms[node].args):
                candidates = [found for binding, guard in candidates
                              for found in self.match(child, arg, binding, guard)]
            answers.extend(candidates)
        # Distinct proofs of the same named binding are OR, not witness counts.
        grouped = {}
        for binding, guard in answers:
            if not guard.is_zero():
                key = tuple(sorted(binding.items()))
                grouped[key] = grouped.get(key, ZERO) | guard
        return [(dict(binding), guard) for binding, guard in grouped.items()]

    def rebuild(self):
        while True:
            previous = self.changed
            self.changed = False
            # A term spelling can become available by a congruent existing row;
            # no physical duplicate row is implied by this symbolic coordinate.
            for nodes in list(self.by_op.values()):
                for a in nodes:
                    if self.terms[a].literal is not None: continue
                    for b in nodes:
                        if a == b: continue
                        guard = self.present[b]
                        for x, y in zip(self.terms[a].args, self.terms[b].args):
                            guard = guard * self.equality(x, y)
                        if not guard.is_zero():
                            self.insert(a, guard)
                            self.union(a, b, guard)
            # Transitivity is part of the effect, including paths crossing Cells.
            for mid in range(len(self.terms)):
                neighbors = [n for n in range(len(self.terms)) if n != mid and not self.equality(mid, n).is_zero()]
                for pos, a in enumerate(neighbors):
                    for b in neighbors[pos + 1:]:
                        self.union(a, b, self.equality(a, mid) * self.equality(mid, b))
            current = self.changed
            self.changed = previous or current
            if not current: return

    def saturate(self, on_sweep=None):
        sweeps = 0
        while True:
            self.changed = False
            self.rebuild()
            for rule in self.rules:
                root, pattern = rule['body'][0]['args']
                for node in list(self.by_op[pattern['head']]):
                    # Bind root to the actual witness. Equality propagation makes
                    # aliases observable without enumerating root aliases again.
                    for env, condition in self.match(pattern, node, {root['name']: node}, self.present[node]):
                        for action in rule['head']:
                            old = len(self.terms)
                            self.action(action, condition, env)
                            self.rule_effects[rule['name']].update(range(old, len(self.terms)))
            sweeps += 1
            if on_sweep: on_sweep(sweeps, len(self.terms), len(self.equal))
            if not self.changed: return sweeps

    def render(self, key):
        term = self.terms[key]
        if term.literal is not None: return term.literal
        return '(' + ' '.join([term.op] + [self.render(a) for a in term.args]) + ')'

    def world(self, assignment, response=None):
        def present(n):
            return self.present[n].evaluate(assignment) if response is None else response[f'row:{self.render(n)}']
        def equal(a,b):
            if response is None: return self.equality(a,b).evaluate(assignment)
            if a==b:return present(a)
            a,b=sorted((a,b))
            return response.get(f'eq:{self.render(a)}={self.render(b)}',0)
        nodes = [i for i, term in enumerate(self.terms) if term.literal is None and present(i)]
        classes = []
        for node in nodes:
            group = next((c for c in classes if equal(node,c[0])), None)
            if group is None: classes.append([node])
            else: group.append(node)
        return nodes, classes

    def compiled_actions(self, assignment, response=None):
        nodes, classes = self.world(assignment,response)
        # Emit the quotient itself: one operation per canonical constructor row,
        # with explicit class slots. Symbolic term aliases are not physical rows.
        owner={n:i for i,c in enumerate(classes) for n in c}
        rows=defaultdict(dict);dependencies=defaultdict(set)
        for n in nodes:
            t=self.terms[n];args=[]
            for child in t.args:
                if self.terms[child].literal is not None:args.append(('literal',self.render(child)))
                else:
                    args.append(('class',owner[child]));dependencies[owner[n]].add(owner[child])
            rows[owner[n]][(t.op,tuple(args))]=n
        order=[];visiting=set();done=set()
        def visit(c):
            if c in done:return
            if c in visiting:raise ValueError('cyclic class graph needs a cyclic effect emitter')
            visiting.add(c)
            for child in sorted(dependencies[c]):visit(child)
            visiting.remove(c);done.add(c);order.append(c)
        for c in range(len(classes)):visit(c)
        name=lambda c:f'structural_class_{c}'
        def expression(row):
            op,args=row
            return '('+' '.join([op]+[text if kind=='literal' else name(text) for kind,text in args])+')'
        actions=[]
        for c in order:
            constructors=sorted(rows[c],key=lambda row:(len(self.render(rows[c][row])),str(row)))
            actions.append(f'(let {name(c)} {expression(constructors[0])})')
            actions.extend(f'(union {name(c)} {expression(row)})' for row in constructors[1:])
        return actions

    def observations(self):
        results = {f'row:{self.render(n)}': self.present[n] for n, t in enumerate(self.terms) if t.literal is None}
        results.update({f'eq:{self.render(a)}={self.render(b)}': p for (a, b), p in self.equal.items()})
        return results

    def delta_actions(self, before, after):
        """Emit only new row spellings and a spanning forest of new equalities."""
        actions=[]
        parent=list(range(len(self.terms)))
        def find(x):
            while parent[x]!=x:
                parent[x]=parent[parent[x]];x=parent[x]
            return x
        for (a,b) in self.equal:
            name=f'eq:{self.render(a)}={self.render(b)}'
            if before.get(name,0):parent[find(a)]=find(b)
        for n,t in enumerate(self.terms):
            if t.literal is not None:continue
            name=f'row:{self.render(n)}'
            if before[name]>after[name]:raise ValueError('nonmonotone structural transition')
            if after[name] and not before[name]:actions.append(self.render(n))
        for (a,b) in self.equal:
            name=f'eq:{self.render(a)}={self.render(b)}'
            if before.get(name,0)>after[name]:raise ValueError('nonmonotone equality transition')
            if after[name] and find(a)!=find(b):
                actions.append(f'(union {self.render(a)} {self.render(b)})')
                parent[find(a)]=find(b)
        return actions
