import copy
import random
import unittest
import numpy as np
from model import (Graph, MatrixState, ScalarState, CoordinateState, CoordinateOperator,
                   OccurrenceState, Operator, InconsistentCoordinates, equality_pairs)
from run import fixture, scale_graph


class RepresentationTests(unittest.TestCase):
    def assert_states(self, states):
        reference=states[0]
        for s in states[1:]:
            self.assertEqual(reference.snapshot(),s.snapshot())
            self.assertEqual(
                equality_pairs(reference.snapshot()[2],reference.unions,{0,1}),
                equality_pairs(s.snapshot()[2],s.unions,{0,1}))

    def test_late_length_and_new_spine_do_not_replay_history(self):
        g=fixture();g.known_length.remove(3) # also blocks BOTH root->child edges
        states=[ScalarState(g,[(5,0)]),MatrixState(g,[(5,0)]),CoordinateState(g,[(5,0)])]
        for r in range(18):
            if r in (3,6,9):
                if r==3:g.known_length.add(3)
                if r==6:g.known_length.add(2)
                if r==9:g.concat[5].append((0,4))
                states[0].graph=copy.deepcopy(g)
                for s in states[1:]:s.op.update(g)
            for s in states:s.step()
            self.assert_states(states)

    def test_random_missing_lengths_and_input_arrivals(self):
        # Fixed seed, exact integer oracle, 384 Run boundaries. No sampling of
        # oracle facts: every A/ALL/Get/equality in each fixture is compared.
        rng=random.Random(4381)
        for leaves in range(2,10):
            g=scale_graph(leaves)
            missing=set(rng.sample(range(g.n),g.n//2));g.known_length-=missing
            roots=[g.n-1];states=[ScalarState(g,[(roots[0],0)]),CoordinateState(g,[(roots[0],0)])]
            for r in range(48):
                if r==7:
                    late=g.n//2
                    states[0].gets.add((late,0));states[1].gets[late,0]=True
                if missing and r%3==0:
                    g.known_length.add(missing.pop());states[0].graph=copy.deepcopy(g);states[1].op.update(g)
                for s in states:s.step()
                self.assertEqual(states[0].snapshot(),states[1].snapshot())
                self.assertEqual(equality_pairs(states[0].gets,states[0].unions,set(g.single.values())),
                                 equality_pairs(states[1].snapshot()[2],states[1].unions,set(g.single.values())))

    def test_repeated_occurrences_are_split_not_unioned(self):
        g=Graph([1,2,4,8],{0:0},set(),{1:[(0,0)],2:[(1,1)],3:[(2,2)]},{0,1,2,3})
        with self.assertRaises(InconsistentCoordinates):CoordinateOperator(g)
        states=[ScalarState(g,[(3,0)]),MatrixState(g,[(3,0)]),OccurrenceState(g,[(3,0)])]
        for _ in range(10):
            for s in states:s.step()
            self.assert_states(states)
        self.assertEqual({o for r,n,o in states[-1].snapshot()[0] if r==3 and n==0},set(range(8)))
        self.assertLess(len(states[-1].op.slots),g.n*g.width)

    def test_rank_update_equals_rebake_and_boolean_closure(self):
        g=fixture();p=Operator(g);c=CoordinateOperator(g)
        for edit in ('length','graft'):
            if edit=='length':g.known_length.add(3)
            else:g.concat[5].append((0,4))
            p.update(g);c.update(g)
            np.testing.assert_allclose(p.k,Operator(g).k,rtol=1e-12,atol=1e-10)
            np.testing.assert_array_equal(c.k,CoordinateOperator(g).k)

    def test_reject_cycles_capacity_wrap_and_deletion(self):
        with self.assertRaises(ValueError):Graph([1,2],{0:0},set(),{1:[(1,0)]},{0,1}).validate()
        g=fixture();p=Operator(g);seed=np.zeros((g.n,g.n,g.width),dtype=bool);seed[0,5,1]=True
        with self.assertRaises(ValueError):p.closure(seed)
        c=CoordinateOperator(g);g.known_length.remove(0)
        with self.assertRaises(ValueError):c.update(g)

    def test_saturating_macro_is_not_one_run(self):
        g=scale_graph(5);s=CoordinateState(g,[(g.n-1,0)]);s.step()
        self.assertEqual(int(s.a[g.n-1].sum()),1)
        self.assertGreater(int(s.op.k[g.n-1].sum()),1)

    def test_float32_boolean_dot_bound(self):
        # The iterative quotient never counts proof paths. Each dot product
        # adds at most n exact 0/1 values, then clamps to 0/1.
        g=scale_graph(12);c=CoordinateOperator(g)
        self.assertTrue(np.isin(c.k,[0,1]).all())
        self.assertTrue(np.isin(c.t,[0,1]).all())
        np.testing.assert_array_equal(np.minimum(1,c.k@c.k),c.k)


if __name__=='__main__':unittest.main()
