import random
import unittest
from probe_closed_storage import Flat, Shared, resolve, retained_bytes


class StorageTests(unittest.TestCase):
    def assert_equal(self, a, b):
        self.assertEqual(a.rows(), b.rows())
        for i in range(len(a.uf.parent)):
            self.assertEqual(a.select(i), b.select(i))
        self.assertEqual(a.join(), b.join())

    def test_overlap_congruence_and_external_join(self):
        template = [(0, 1, 2), (1, 0, 2)]
        bindings = [[0, 1, 2], [0, 1, 3], [3, 4, 5]]
        a, b = Flat(template, bindings, 6), Shared(template, bindings, 6)
        self.assert_equal(a, b)
        self.assertEqual(b.instances, 2)
        self.assertEqual(a.rows(), {(0,1,2),(1,0,2),(2,4,5),(4,2,5)})
        self.assertIn((0,1,2,4,5), b.join())
        for store in (a,b):
            store.insert((5,5,4))
            store.union(0,1)
        self.assert_equal(a,b)
        self.assertIn((5,5,4), b.rows())

    def test_random_overlaps_and_mutations(self):
        rng = random.Random(731)
        for _ in range(30):
            template = [(0,1,2),(2,3,4),(3,2,4)]
            bindings = [[rng.randrange(40) for _ in range(5)] for _ in range(12)]
            a,b = Flat(template,bindings,40),Shared(template,bindings,40)
            self.assert_equal(a,b)
            for _ in range(8):
                pair = rng.sample(range(40),2)
                a.union(*pair); b.union(*pair)
                row=tuple(rng.randrange(40) for _ in range(3))
                a.insert(row); b.insert(row)
                self.assert_equal(a,b)

    def test_retained_measure_includes_buffers(self):
        a=Shared([(0,1,2)],[[0,1,2]],3)
        before=retained_bytes(a)
        a.residual.extend([0,0,0]*100)
        self.assertGreaterEqual(retained_bytes(a)-before, 2400)

    def test_symbolic_resolution_is_exact(self):
        s={'values':[{'sort':'i64','literal':'0'}, {'sort':'Math','literal':None},
                     {'sort':'Math','literal':None}],
           'rows':[{'op':'Hole','args':[0],'result':1},
                   {'op':'Add','args':[1,1],'result':2}]}
        self.assertEqual(resolve('(Add (Hole 0) (Hole 0))',s),2)
        with self.assertRaises(ValueError): resolve('(Hole 1)',s)
        with self.assertRaises(ValueError): resolve('(Hole x)',s)
        with self.assertRaises(ValueError): resolve('(Hole 0) 0',s)


if __name__ == '__main__':
    unittest.main()
