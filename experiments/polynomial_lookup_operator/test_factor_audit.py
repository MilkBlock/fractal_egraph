import unittest
from factor_audit import compile_factors,project


def atom(name,*slots):
    return {'table':name,'sorts':['E']*len(slots),'columns':[{'Slot':s} for s in slots]}


class FactorTests(unittest.TestCase):
    def test_renaming_shares_projection_but_different_wiring_does_not(self):
        a,_=compile_factors({'atoms':[atom('P',0,1),atom('Q',1,2)]})
        b,_=compile_factors({'atoms':[atom('P',8,9),atom('Q',9,10)]})
        c,_=compile_factors({'atoms':[atom('P',8,9),atom('Q',8,10)]})
        self.assertEqual(a[0][0],b[0][0]);self.assertNotEqual(a[0][0],c[0][0])

    def test_cycle_not_mislabelled_path(self):
        factors,forest=compile_factors({'atoms':[atom('P',0,1),atom('Q',1,2),atom('R',2,0)]})
        self.assertFalse(forest);self.assertTrue(all(len(s)==2 for _,s in factors))

    def test_payloads_and_alias_constraints_retained(self):
        fs,_=compile_factors({'atoms':[atom('P',0,0,1),atom('Q',0,2)]})
        v=lambda n:('Eq','E',str(n))
        rows=[(v(1),v(1),v(3)),(v(1),v(2),v(4)),(v(1),v(1),v(5))]
        self.assertEqual(dict(project(rows,fs[0][0])),{(v(1),):[0,2]})

    def test_joint_coordinates_not_independent_marginals(self):
        fs,_=compile_factors({'atoms':[atom('P',0,1),atom('Q',0,1)]})
        v=lambda n:('Eq','E',str(n))
        out=project([(v(1),v(2)),(v(3),v(4))],fs[0][0])
        self.assertNotIn((v(1),v(4)),out)


if __name__=='__main__':unittest.main()
