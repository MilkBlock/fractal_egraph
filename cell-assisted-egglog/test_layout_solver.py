import importlib.util
import itertools
from pathlib import Path
import unittest

spec=importlib.util.spec_from_file_location('layout_solver',Path(__file__).with_name('layout_solver.py'))
layout=importlib.util.module_from_spec(spec);spec.loader.exec_module(layout)

class LayoutTest(unittest.TestCase):
    def test_optimum_matches_exhaustive_tiny_cover(self):
        request=dict(groups=[dict(width=32,edges=[[0,0,1],[1,0,2],[2,3,4]])])
        cs=layout.candidates(request['groups'],layout.DEFAULT_COSTS)
        best=float('inf')
        for bits in itertools.product([0,1],repeat=len(cs)):
            selected=[b for b,x in zip(cs,bits) if x]
            if all(sum(c in b['conditions'] for b in selected)==1 for c in [0,1,2]):
                best=min(best,sum(b['price'] for b in selected))
        for backend in ['scip','gurobi']:
            with self.subTest(backend=backend):
                layout.solver.select(backend)
                r=layout.solve(request,layout.DEFAULT_COSTS)
                self.assertAlmostEqual(r['report']['objective'],best)

if __name__=='__main__':unittest.main()
