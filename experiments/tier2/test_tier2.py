import unittest
from affine import fit_trigger,infer,invariant,affine,source_certificate,path_samples
from mine import canonical,paths
class Tier2Tests(unittest.TestCase):
    def test_learns_coefficients_not_rule_names(self):
        for dx,dy in [(-1,1),(-2,3),(2,-7),(0,4),(-4,0)]:
            samples=[((10+dx*k,dy*k),(10+dx*(k+1),dy*(k+1))) for k in range(5)]
            self.assertEqual(infer(samples),(dx,dy));a,b=invariant((dx,dy));self.assertEqual(a*dx+b*dy,0)
    def test_trigger_is_selected_from_training_only(self):
        samples=[((10,0),(9,7)),((9,7),(8,14)),((8,14),(7,15)),((7,15),(6,16)),((6,16),(5,17))]
        self.assertEqual(fit_trigger(samples),(2,(-1,1)))
    def test_noise_rejected(self):
        self.assertIsNone(infer([((3,0),(2,1)),((2,1),(1,3))]))
    def test_source_nonlinearity_rejected(self):
        x={'var':'n'}
        with self.assertRaises(ValueError):affine({'op':'*','args':[x,x]},'n')
    def test_alpha_normalization_preserves_aliases(self):
        def e(a,b):return {'op':'Add','args':[{'var':a},{'var':b}],'span':'arbitrary'}
        self.assertEqual(canonical(e('x','y')),canonical(e('a','b')))
        self.assertNotEqual(canonical(e('x','x')),canonical(e('x','y')))
    def test_paths_keep_equal_operator_occurrences_distinct(self):
        r={'head':[{'union':[{'op':'Add','span':'a','args':[]},{'op':'Add','span':'b','args':[]}]}]}
        self.assertNotEqual(paths(r)['a'],paths(r)['b'])
    def test_ambiguous_or_cyclic_edge_not_a_chain(self):
        with self.assertRaises(ValueError):path_samples([(3,2,1),(3,1,1)],3)
        with self.assertRaises(ValueError):path_samples([(3,2,1),(2,3,1)],3)
    def test_zero_motion_is_not_fractal_evidence(self):
        self.assertIsNone(invariant((0,0)))
if __name__=='__main__':unittest.main()
