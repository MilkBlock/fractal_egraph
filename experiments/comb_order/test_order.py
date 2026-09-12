import unittest
from order import partition, fractions, key

def candidate(i,members,score):
    return {'id':i,'members':members,'lhs_num':1,'rhs_num':score}

class OrderTests(unittest.TestCase):
    def test_highest_individual_score_is_not_always_a_good_merge(self):
        cs=[candidate(0,['a'],3),candidate(1,['b'],3),candidate(2,['a','b'],4)]
        blocks,r=partition(cs,['a','b'],[('a','b')])
        self.assertEqual(len(blocks),2)
        self.assertEqual(r['partition_objective'],6)
    def test_overlap_is_not_counted_twice(self):
        cs=[candidate(0,['a'],1),candidate(1,['b'],1),candidate(2,['c'],1),candidate(3,['a','b'],5),candidate(4,['b','c'],4)]
        blocks,r=partition(cs,['a','b','c'],[('a','b'),('b','c')])
        self.assertEqual(sorted(n for b in blocks for n in b['members']),['a','b','c'])
        self.assertGreaterEqual(r['partition_objective'],r['singleton_objective'])
    def test_convex_blocks_can_still_form_a_cycle_and_must_be_rejected(self):
        ns=['a1','a2','a3','b1','b2','b3']
        cs=[candidate(i,[n],0) for i,n in enumerate(ns)]
        cs.extend([candidate(6,ns[:3],0),candidate(7,ns[3:],0)])
        edges=[('a1','a3'),('a2','a3'),('b1','b3'),('b2','b3'),('a1','b1'),('b2','a2')]
        _,r=partition(cs,ns,edges)
        self.assertEqual(r['cycle_rejected_merges'],1)
        self.assertTrue(r['quotient_acyclic_verified'])
    def test_constructor_key_not_output_id_defines_enode_count(self):
        r={'scope':0,'op':'Mul','sorts':['Math','Math','Math'],'values':['a','b','c']}
        self.assertEqual(key(r),key(dict(r,values=['a','b','d'])))
        self.assertEqual(fractions({'lhs_num':0,'rhs_num':0}),0)

if __name__=='__main__':unittest.main()
