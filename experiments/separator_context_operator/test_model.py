import copy
import unittest
import numpy as np
from data import fixture
from model import *
from run import evolving_fixture,loop_check


class ProjectionTests(unittest.TestCase):
    def test_multi_ports_and_updates(self):
        for name,data in evolving_fixture():
            report,f,answer=loop_check(data)
            self.assertTrue(report['verified_against_indexed_query'])
        self.assertEqual(len(loop_factors(fixture())['ctx_ids']),4)

    def test_signed_join_and_alias_update(self):
        rng=np.random.default_rng(602)
        for keys in range(1,18):
            left=rng.integers(0,4,size=(9,keys));right=rng.integers(0,3,size=(7,keys))
            got,x,y,_=signed_join(left,right)
            np.testing.assert_array_equal(got,left@right.T)
            # Merge identity 1 into 0 in the SAME signed basis, no binding scan.
            if keys>1:
                xx=signed_remap(x,{1:0});yy=signed_remap(y,{1:0})
                left[:,0]+=left[:,1];left[:,1]=0;right[:,0]+=right[:,1];right[:,1]=0
                np.testing.assert_array_equal(xx@yy.T//xx.shape[1],left@right.T)

    def test_linear_insert_and_remove_without_rematching(self):
        h=hadamard(4);l=np.zeros((2,4),dtype=np.int64);r=np.zeros((3,4),dtype=np.int64)
        x=l@h;y=r@h
        # State changes are rank-one row/code updates, not full joins.
        for side,row,key,weight in [('l',0,1,1),('r',2,1,1),('l',0,3,1),('r',1,3,1),('l',0,1,-1)]:
            if side=='l':l[row,key]+=weight;x[row]+=weight*h[key]
            else:r[row,key]+=weight;y[row]+=weight*h[key]
            np.testing.assert_array_equal(x@y.T//4,l@r.T)

    def test_if_joint_separator_cannot_be_replaced_by_marginals(self):
        data={'ifs':[[100,0,10,11,12]],'args':[[20,1,0],[21,2,1],[22,2,2],[23,1,3]],
              'gets':[[11,1000003,70],[12,1000003,71],[20,4,70],[21,9,70],[22,4,71],[23,9,71]],
              'state_values':[70]}
        got,_,_=if_projection(data)
        self.assertEqual(got,set());self.assertEqual(got,if_reference(data))
        self.assertEqual(got,if_masked_projection(data)[0])
        data['args'].append([24,1,4]);data['gets'].append([24,4,71])
        got,_,_=if_projection(data)
        self.assertEqual(got,{(100,10,1000003,4)});self.assertEqual(got,if_reference(data))
        self.assertEqual(got,if_masked_projection(data)[0])
        # Union of output values changes signatures without any numeric law.
        data['gets']=[[p,i,70 if v==71 else v] for p,i,v in data['gets']]
        self.assertEqual(if_projection(data)[0],if_reference(data))
        self.assertEqual(if_masked_projection(data)[0],if_reference(data))

    def test_multi_cost_union_targets_and_equality_holes(self):
        data={'pairs':[[1,10,1],[1,11,9],[2,20,5],[3,30,9]],'smaller':[[100,1,2],[101,1,3],[102,3,2]]}
        combined,sep,_=comparator_projection(data);ref=comparator_reference(data)
        self.assertEqual(sep,ref);self.assertEqual(combined,set().union(*ref.values()))
        self.assertNotIn((100,1),sep['eq']) # overlapping intervals do not prove equal costs
        self.assertIn((100,1),combined);self.assertIn((100,2),combined)
        self.assertEqual(fused_comparator_matrix(data)[0],combined)
        data['pairs']=[[1,10,-2**63],[2,20,2**63-1],[3,30,2**63-1]]
        self.assertEqual(fused_comparator_matrix(data)[0],set().union(*comparator_reference(data).values()))

    def test_incremental_factor_ownership(self):
        data=fixture();rng=np.random.default_rng(92)
        for _ in range(100):
            fresh={k:{i for i in range(len(data[k])) if rng.integers(2)}
                   for k in ('loops','contexts','types','constants')}
            full,terms=delta_terms(data,fresh)
            self.assertEqual(delta_effects(data,full,terms),delta_reference(data,fresh))
        # A new witness can yield an already-known RHS constructor key. Taking
        # full_boolean_support - old_boolean_support would wrongly lose it.
        fresh={'constants':{2}}
        full,terms=delta_terms(data,fresh)
        emitted=delta_effects(data,full,terms)
        self.assertTrue(emitted)
        old=copy.deepcopy(data);old['constants']=old['constants'][:2]+old['constants'][3:]
        old_keys=new_const_keys(loop_factors(old))
        self.assertTrue({(c,t,x) for root,x,t,c,prev in emitted}&old_keys)

    def test_whole_rule_const_feedback_without_binding_expansion(self):
        data=fixture();machine=LoopClosure(data)
        keys={(c,t,x) for cls,c,t,x in data['constants']}
        contexts={};types={}
        for e,c in data['contexts']:contexts.setdefault(e,set()).add(c)
        for e,t in data['types']:types.setdefault(e,set()).add(t)
        for _ in range(5):
            old=keys.copy();effects=set()
            for root,inp,body in data['loops']:
                for c,t,ctx in old:
                    if ctx not in contexts.get(body,set()):continue
                    for ic in contexts.get(inp,set()):
                        for it in types.get(inp,set()):
                            keys.add((c,it,ic));effects.add((root,c,it,ic,c,t,ctx))
            machine.step()
            self.assertEqual(machine.keys(),keys)
            self.assertEqual(machine.deferred_effects(),effects)


if __name__=='__main__':unittest.main()
