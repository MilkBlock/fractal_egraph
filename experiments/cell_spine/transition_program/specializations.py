"""Observed fact guards -> certified parameterized whole-Cell transitions.

The guard is exact (including distinct equality groups), not a statistical rule.
Its transfer proof uses constructor-only equivariance plus a symbolic native
whole-closure comparison. No unobserved alias partitions are enumerated.
"""
from collections import Counter
import hashlib

from .runtime import closure_key,partition
from experiments.cell_spine.factorized_boundary.demo import sexpr


def validate_environment(ast):
    """The renaming proof requires a fixed, name-insensitive rule environment."""
    if ast.get('rules') or ast.get('actions') or any(c['kind'] not in ('declaration','rule') for c in ast['commands']):
        raise ValueError('local Cell specialization requires an input-free constructor-rewrite environment')
    def expression(t):
        if t['kind']=='var':return
        sig=ast['signatures'].get(t.get('head'),{})
        if t['kind']!='call' or sig.get('kind')!='constructor' or sig.get('output')!='Math' or any(s!='Math' for s in sig.get('input',[])):
            raise ValueError('literal/primitive/analysis-data erasure needs a separate Cell contract')
        for child in t['args']:expression(child)
    for rule in ast['rewrites']:
        if rule['conditions'] or rule['subsume'] or rule['bidirectional'] or rule['ruleset']:
            raise ValueError('guarded/effectful/nondefault rulesets need a separate Cell contract')
        expression(rule['lhs']);expression(rule['rhs'])


class FactSpecializations:
    def __init__(self,library,cells,context,ast):
        validate_environment(ast)
        self.library,self.cells,self.context=library,cells,context
        self.source=library.source
        self.environment=hashlib.sha256(self.source.encode()).hexdigest()
        self.recipes={};self.entries=[];self.stats=Counter()

    def describe(self,op,left,right):
        if self.library.source!=self.source:
            raise ValueError('rule environment changed; rebuild the specialization library')
        a,b=self.cells.get(left),self.cells.get(right)
        values=tuple(a['binding'])+tuple(b['binding'])
        equality,_=partition(values)
        key=(op,a['template'],b['template'],equality)
        return key,values,len(a['binding'])

    def instantiate(self,recipe,values):
        binding=tuple(values[i] for i in recipe['output_sources'])
        return self.cells.intern(recipe['target_template'],binding)

    def apply(self,op,left,right):
        key,values,_=self.describe(op,left,right)
        recipe=self.recipes.get(key)
        if recipe is None:
            self.stats['misses']+=1;return None
        target=self.instantiate(recipe,values)
        recipe['replays']+=1;self.stats['replays']+=1
        self.stats['input_slots_classified']+=len(values)
        self.stats['output_slots_copied']+=len(recipe['output_sources'])
        return target

    def observe(self,op,left,right,result):
        """Called only after this supplied constructor fact has a valid Cell."""
        key,values,left_width=self.describe(op,left,right)
        if key in self.recipes:
            recipe=self.recipes[key]
            assert self.instantiate(recipe,values)==self.cells.resolve(result)
            recipe['observations']+=1;self.stats['repeat_observations']+=1
            return
        seed,context_values=self.context(op,left,right)
        before=self.library.stats['native_bakes']
        fact_template=self.library.add(seed)
        self.stats['certificate_native_bakes']+=self.library.stats['native_bakes']-before
        t=self.library.templates[fact_template]
        assert len(context_values)==t['arity']
        context_index={v:i for i,v in enumerate(context_values)}
        observed=self.cells.get(result)
        # No new opaque identity may be fabricated by this positive constructor
        # domain. All target inputs must come from the supplied parent facts.
        if not set(observed['binding'])<=set(context_values):
            raise ValueError('target input absent from parent context')
        target_labels=[context_index[v] for v in observed['binding']]
        checked=closure_key(self.library.graphs[fact_template],t['root'],range(t['arity']))
        actual=closure_key(self.library.graphs[observed['template']],
                           self.library.templates[observed['template']]['root'],target_labels)
        if checked!=actual:
            raise AssertionError('published Cell differs from the full native fact-context closure')
        first={}
        for i,v in enumerate(values):first.setdefault(v,i)
        projection=[first[v] for v in context_values]
        groups=[]
        for group in range(max(key[3])+1):
            groups.append([('L' if i<left_width else 'R')+str(i if i<left_width else i-left_width)
                           for i,g in enumerate(key[3]) if g==group])
        recipe=dict(id=len(self.entries),operator=op,source_templates=[key[1],key[2]],
            source_arities=[left_width,len(values)-left_width],equality_partition=list(key[3]),
            equal_groups=groups,groups_must_remain_distinct=True,
            target_template=fact_template,output_sources=projection,
            output_ports=[('L' if i<left_width else 'R')+str(i if i<left_width else i-left_width) for i in projection],
            fact_seed=sexpr(seed),environment_sha256=self.environment,
            proof=dict(native_rooted_whole_closure_equal=True,
                       transport='bijection of opaque inputs in a fixed positive constructor-rewrite environment'),
            observations=1,replays=0)
        self.recipes[key]=recipe;self.entries.append(recipe);self.stats['certificates']+=1

    def report(self):
        return dict(environment_sha256=self.environment,stats=dict(self.stats),recipes=self.entries)
