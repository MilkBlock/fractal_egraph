"""Backend contracts: incremental cuts, fixed bounds, failure and unrestricted size."""
import importlib.util
from pathlib import Path
import tempfile
import unittest
from . import solver


class SolverContract(unittest.TestCase):
    def tearDown(self):solver.select('scip')

    def exercise(self,backend):
        solver.select(backend)
        with tempfile.TemporaryDirectory() as d,solver.Model('incremental') as m:
            x=m.addVar(vtype=solver.BINARY,name='x');t=m.addVar(name='cost')
            m.setObjective(t+3*(1-x));m.optimize()
            self.assertEqual(m.Status,solver.OPTIMAL);self.assertAlmostEqual(m.ObjVal,0)
            m.addConstr(t>=5*x)
            with self.assertRaises(RuntimeError):m.value(x)
            self.assertFalse(hasattr(m,'ObjVal'))
            m.optimize();self.assertEqual(m.Status,solver.OPTIMAL);self.assertAlmostEqual(m.ObjVal,3)
            self.assertAlmostEqual(m.value(x),0)
            m.fix(x,1);m.optimize();self.assertEqual(m.Status,solver.OPTIMAL)
            self.assertAlmostEqual(m.ObjVal,5);self.assertAlmostEqual(m.ObjBound,5)
            m.write(Path(d)/'model.lp');m.write(Path(d)/'model.sol')
            self.assertTrue((Path(d)/'model.lp').stat().st_size)
            self.assertTrue((Path(d)/'model.sol').stat().st_size)
        with self.assertRaises(RuntimeError):m.value(x)
        r=solver.statistics()['models']['incremental']
        self.assertEqual(r['solve_calls'],3);self.assertEqual(r['peak_variables'],2)
        self.assertEqual(r['nonoptimal_solves'],0)
        self.assertGreater(r['solve_seconds'],0);self.assertGreater(r['api_build_seconds'],0)

    def test_scip_incremental_contract(self):self.exercise('scip')

    @unittest.skipUnless(importlib.util.find_spec('gurobipy'),'optional Gurobi backend not installed')
    def test_optional_gurobi_same_contract(self):self.exercise('gurobi')

    def test_scip_exceeds_both_old_license_limits(self):
        solver.select('scip')
        with solver.Model('large') as m:
            xs=[m.addVar(vtype=solver.BINARY,name=f'x{i}') for i in range(2005)]
            for x in xs:m.addConstr(x>=0)
            m.addConstr(solver.quicksum(xs)>=1)
            m.setObjective(solver.quicksum((i+1)*x for i,x in enumerate(xs)))
            m.optimize();self.assertEqual(m.Status,solver.OPTIMAL)
            self.assertEqual(m.NumVars,2005);self.assertEqual(m.NumConstrs,2006)
            self.assertAlmostEqual(m.ObjVal,1);self.assertAlmostEqual(m.value(xs[0]),1)

    def test_infeasibility_is_not_an_optimal_or_fallback_result(self):
        solver.select('scip')
        with solver.Model('infeasible') as m:
            x=m.addVar(vtype=solver.BINARY);m.addConstr(x>=2);m.setObjective(x);m.optimize()
            self.assertEqual(m.Status,'infeasible')
            with self.assertRaises(RuntimeError):m.value(x)
        self.assertEqual(solver.statistics()['models']['infeasible']['nonoptimal_solves'],1)


if __name__=='__main__':unittest.main()
