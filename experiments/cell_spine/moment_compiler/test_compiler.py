import json
from itertools import combinations_with_replacement
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from . import demo
from .compiler import Circuit, bit_code, lift2, layout, execute, reconstruct, certify_commutative
from experiments.cell_spine.factorized_boundary.demo import ground_key


class Algebra(unittest.TestCase):
    def circuit(self,bits=3,slots=4):
        return Circuit(bits,slots,dict(head='Add'))

    def test_pair_exact_with_aliases_exhaustively(self):
        proof=demo.exhaustive()
        self.assertEqual(proof['pair_exhaustive'][-1]['unordered_pairs_including_aliases'],2080)

    def test_bit_width_does_not_allocate_the_domain(self):
        c=self.circuit(bits=32,slots=2)
        shape,n=c.compile(('Add',('Input',0),('Input',1)))
        self.assertEqual(c.nodes[n]['width'],528)
        x=bit_code((1<<32)-1,32);y=bit_code(0,32)
        a,_=c.evaluate([x,y]);b,_=c.evaluate([y,x]);same,_=c.evaluate([x,x])
        self.assertEqual(a[n],b[n]);self.assertNotEqual(a[n],same[n])
        with self.assertRaises(ValueError):bit_code(1<<32,32)

    def test_recursive_moments_preserve_tree_not_just_bag(self):
        c=self.circuit(bits=2)
        tree=('Add',('Add',('Input',0),('Input',1)),('Add',('Input',2),('Input',3)))
        _,root=c.compile(tree)
        pair_choices=list(combinations_with_replacement(range(4),2))
        values={}
        for left,right in combinations_with_replacement(pair_choices,2):
            tape,_=c.evaluate([bit_code(i,2) for i in left+right])
            signature=tape[root]
            self.assertNotIn(signature,values)
            values[signature]=(left,right)
        self.assertEqual(len(values),55)

    def test_norm_distance_cannot_compensate_failed_guard(self):
        c=self.circuit(bits=3)
        _,root=c.compile(('Add',('Add',('Input',0),('Input',1)),('Input',2)))
        rows=[]
        for ids in [(0,1,2,3),(1,0,2,3),(0,2,1,3),(0,1,3,2)]:
            tape,_=c.evaluate([bit_code(i,3) for i in ids]);rows.append(tape[root])
        dist=lambda a,b:sum((x-y)**2 for x,y in zip(a,b))
        self.assertEqual(dist(rows[0],rows[1]),0)
        self.assertGreater(dist(rows[0],rows[2]),0)
        self.assertGreater(dist(rows[0],rows[3]),0)

    def test_unreported_update_rejected(self):
        c=self.circuit();c.compile(('Add',('Input',1),('Input',2)))
        p=[bit_code(i,3) for i in range(4)]
        old,_=c.evaluate(p);p[1]=p[2]
        with self.assertRaisesRegex(ValueError,'unreported'):c.evaluate(p,old,{0})
        fresh,_=c.evaluate(p);new,_=c.evaluate(p,old,{1})
        self.assertEqual(fresh,new)

    def test_no_uncertified_operator(self):
        c=self.circuit()
        with self.assertRaisesRegex(ValueError,'certified'):c.compile(('Mul',('Input',0),('Input',1)))
        with self.assertRaisesRegex(ValueError,'certificate'):certify_commutative({'rewrites':[]},'Add')


class NativePipeline(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp=tempfile.TemporaryDirectory(prefix='moment-compiler-')
        cls.addClassCleanup(cls.tmp.cleanup)
        cls.out=Path(cls.tmp.name)/'run'
        cls.report=demo.run(cls.out,skip_build=True)
        cls.lib=json.loads((cls.out/'bake/library.json').read_text())
        cls.ast=json.loads((cls.out/'bake/rules.json').read_text())

    def test_native_all_bindings_and_actual_context(self):
        self.assertTrue(self.report['native_exact'])
        self.assertEqual((self.report['full_binding_matches'],self.report['global_bindings']),(462,110))
        self.assertEqual(self.report['existing_canonical_matches'],14)

    def test_union_requires_alias_specialization_not_only_vector_refresh(self):
        r=self.report['union']
        self.assertEqual(r['stale_plan_canonical_bindings'],512)
        self.assertEqual(r['missed_by_stale_plan'],106)
        self.assertEqual(r['specialized_plan_canonical_bindings'],618)
        self.assertEqual(r['native_canonical_bindings'],618)
        self.assertEqual(r['alias_specializations_baked'],3)
        self.assertFalse(r['same_compiled_plan'])
        self.assertTrue(r['old_guard_rejected'])
        self.assertTrue(r['native_equal'])

    def test_conditional_commutativity_does_not_grant_global_encoding(self):
        import copy
        ast=copy.deepcopy(self.ast)
        for r in ast['rewrites']:
            if r['lhs'].get('head')=='Add' and r['rhs'].get('head')=='Add':
                r['conditions']=[{'guard':'not globally true'}]
        with self.assertRaisesRegex(ValueError,'certificate'):certify_commutative(ast,'Add')

    def test_shared_program_and_fused_program_accounted(self):
        r=self.report
        self.assertEqual(r['plans']['root-fused']['matrix_macs'],640)
        self.assertEqual(r['plans']['all-fused']['matrix_macs'],4000)
        self.assertEqual(r['plans']['all-shared']['matrix_macs'],3776)
        self.assertEqual(r['plans']['all-shared']['blocks'],2)
        self.assertEqual(r['plans']['all-shared']['condition_pair_objects'],0)
        self.assertEqual(r['plans']['all-shared']['retained_channels'],64)
        self.assertTrue(r['update']['reused_pair_object'])

    def test_random_finite_bindings_with_aliased_coordinate_inputs(self):
        # Checks projected tree semantics, not completeness of Bake after alias.
        import random
        rng=random.Random(31)
        c=Circuit(4,4,certify_commutative(self.ast,'Add'))
        types=[i%2 for i in range(12)]
        plans=[layout(c,self.lib,types,shared=s) for s in (False,True)]
        bindings=[[rng.randrange(16) for _ in range(4)] for _ in types]
        tapes=[c.evaluate([bit_code(v,4) for v in row])[0] for row in bindings]
        matrices=[]
        def term(t,b):
            if t[0]=='Input':return ('Var',str(b[t[1]]))
            return (t[0],*(term(x,b) for x in t[1:]))
        for plan in plans:
            result=reconstruct(plan,execute(plan,tapes,validate=False))
            keys=[ground_key(term(self.lib['templates'][types[ch['instance']]]['channels'][ch['local']]['k'],bindings[ch['instance']])) for ch in plan['channels']]
            self.assertEqual(result['gate'],[[int(a==b) for b in keys] for a in keys])
            matrices.append(result['gate'])
        self.assertEqual(*matrices)

    def test_plan_does_not_read_bindings_or_expand_spines(self):
        from experiments.cell_spine.shared_matching.demo import Matcher
        from experiments.cell_spine.factorized_boundary.demo import Family,Graph
        from contextlib import ExitStack
        with ExitStack() as stack:
            for obj,name in [(Matcher,'evaluate'),(Family,'enumerate'),(Graph,'value'),
                             (demo.markers.MarkerPlan,'candidates'),(demo.layers,'candidates')]:
                stack.enter_context(patch.object(obj,name,side_effect=AssertionError('forbidden '+name)))
            c=Circuit(4,4,certify_commutative(self.ast,'Add'))
            plan=layout(c,self.lib,[0,1],shared=True)
            tapes=[c.evaluate([bit_code(i+j,4) for j in range(4)])[0] for i in (0,4)]
            answer=execute(plan,tapes)
            zeros=[[0]*len(row) for row in answer]
            self.assertEqual(execute(plan,tapes,zeros),zeros)

    def test_precision_guard(self):
        c=Circuit(4,4,certify_commutative(self.ast,'Add'))
        # Repeated homogeneous layers grow their integer coefficients as well
        # as width. Reject before dispatch rather than silently use FP epsilon.
        t=('Input',0)
        for _ in range(5):t=('Add',t,t)
        library={'templates':[dict(ports=[dict(location='root')],channels=[dict(k=t,port=0,recipe=0)])]}
        with self.assertRaisesRegex(ValueError,'FP32'):layout(c,library,[0])


if __name__=='__main__':unittest.main()
