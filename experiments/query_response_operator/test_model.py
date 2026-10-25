import unittest
from collections import defaultdict
import numpy as np
from model import ResponseBank,reference,choose_anchor,fold_matrix,evaluate_folded_or_original


def atom(name,*slots):return {'table':name,'sorts':['E']*len(slots),'columns':[{'Slot':s} for s in slots]}
def query(*atoms,guards=None):return {'shape':{'atoms':list(atoms),'guards':guards or [],'native_residuals':False}}
def rows(*raw):return [tuple(('Eq','E',str(v)) for v in row) for row in raw]


class ResponseTests(unittest.TestCase):
    def test_different_payloads_share_decision_not_identity(self):
        qs=[query(atom('A',0,1),atom('P',0,2)),query(atom('A',0,1),atom('Q',1,3))]
        ts={'A':rows((1,2),(3,4),(5,6)),'P':rows((1,10),(3,11)),'Q':rows((2,20),(4,21))}
        b=ResponseBank(qs,ts).encode()
        self.assertEqual(b.answers(),reference(qs,ts,b.eligible))
        self.assertEqual(len(b.groups['A']['profiles']),2)
        self.assertNotEqual(b.binding_factors(0,0)[0],b.binding_factors(0,1)[0])

    def test_union_update_and_visibility_recompute(self):
        qs=[query(atom('A',0,1),atom('P',0,2),atom('Q',1,2))]
        # A does not bind shared variable 2: cannot split P/Q into marginals.
        self.assertIsNone(choose_anchor(qs[0]['shape']))
        qs=[query(atom('A',0,1,2),atom('P',0,2),atom('Q',1,2))]
        ts={'A':rows((1,2,3)),'P':rows((1,3)),'Q':rows((2,4))}
        self.assertFalse(ResponseBank(qs,ts).encode().answers())
        ts['Q']=rows((2,3));self.assertTrue(ResponseBank(qs,ts).encode().answers())
        ts['Q']=[];self.assertFalse(ResponseBank(qs,ts).encode().answers())

    def test_multivalued_independent_payloads_preserved(self):
        qs=[query(atom('A',0,1,2),atom('P',0,3),atom('Q',1,4))]
        ts={'A':rows((1,2,9)),'P':rows((1,10),(1,11)),'Q':rows((2,20),(2,21),(2,22))}
        b=ResponseBank(qs,ts).encode();_,payload=b.binding_factors(0,0)
        self.assertEqual([len(x[1]) for x in payload],[2,3])

    def test_new_witness_can_change_effect_bindings_without_changing_response(self):
        qs=[query(atom('A',0,1,2),atom('P',0,3))]
        ts={'A':rows((1,2,9)),'P':rows((1,10))}
        old=ResponseBank(qs,ts).encode();ts['P']+=rows((1,11))
        new=ResponseBank(qs,ts).encode()
        self.assertEqual(old.answers(),new.answers())
        self.assertEqual(len(old.binding_factors(0,0)[1][0][1]),1)
        self.assertEqual(len(new.binding_factors(0,0)[1][0][1]),2)

    def test_guards_not_silently_ignored(self):
        self.assertIsNone(choose_anchor(query(atom('P',0),guards=['guard'])['shape']))

    def test_repeated_variable_and_empty_disconnected_relation(self):
        qs=[query(atom('A',0,0,1),atom('P',2))]
        ts={'A':rows((1,1,2),(1,3,2)),'P':[]}
        self.assertFalse(ResponseBank(qs,ts).encode().answers())
        ts['P']=rows((4,));self.assertEqual(ResponseBank(qs,ts).encode().answers(),{(0,0)})

    def test_random_reference(self):
        rng=np.random.default_rng(18)
        qs=[query(atom('A',0,1,2),atom('P',0,3),atom('Q',1,2)),
            query(atom('A',0,1,2),atom('Q',2,1))]
        for _ in range(60):
            ts={'A':rows(*rng.integers(0,8,(25,3))),'P':rows(*rng.integers(0,8,(15,2))),
                'Q':rows(*rng.integers(0,8,(15,2)))}
            b=ResponseBank(qs,ts).encode();self.assertEqual(b.answers(),reference(qs,ts,b.eligible))

    def test_cofactor_has_non_boolean_weights_and_checked_assumptions(self):
        x=np.array([[1,0,0],[1,1,1]],dtype=np.uint8)
        w=np.ones((3,1),dtype=np.uint8);t=np.array([3],dtype=np.int32)
        f=fold_matrix(x,w,t)
        self.assertEqual(f['w'].tolist(),[[2]])
        answer,valid=evaluate_folded_or_original(x,w,t,f)
        self.assertTrue(valid);np.testing.assert_array_equal(answer,[[False],[True]])
        for new in ([[0,1,1]],[[1,1,0]]):
            new=np.array(new,dtype=np.uint8)
            answer,valid=evaluate_folded_or_original(new,w,t,f)
            self.assertFalse(valid);np.testing.assert_array_equal(answer,new.astype(np.int32)@w==t)


if __name__=='__main__':unittest.main()
