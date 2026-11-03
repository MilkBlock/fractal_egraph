import copy,json,tempfile,unittest,sys
from pathlib import Path
import numpy as np
ROOT=Path(__file__).resolve().parents[2];sys.path.insert(0,str(ROOT))
from experiments.resident_ridge.compiler import compile_library,fit,active_features,guard,frame_for,train
from experiments.resident_ridge.run import Resident,decode,fixture,reference
BUILD=ROOT/'out/resident-ridge-build'

class ResidentTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.lib=json.loads((BUILD/'library.json').read_text());cls.program=fit(compile_library(cls.lib))
    def test_reject_incomplete_proof_and_branch_selection(self):
        bad=copy.deepcopy(self.lib);bad['steps'][0]['bake']['native_complete']=False
        with self.assertRaises(ValueError):compile_library(bad)
        bad=copy.deepcopy(self.lib);bad['steps'].append(copy.deepcopy(bad['steps'][1]))
        with self.assertRaisesRegex(ValueError,'branch'):compile_library(bad)
    def test_inherited_unions_need_a_richer_identity_abi(self):
        bad=copy.deepcopy(self.lib);bad['steps'][0]['input_to_target']=[0,0]
        with self.assertRaisesRegex(ValueError,'inherited equality'):compile_library(bad)
    def test_guards_do_not_accept_new_alias_specialization(self):
        p=self.program;params=[1,2,1,4];state=1;frame=frame_for(p,state,params)
        self.assertEqual(guard(p,p['transitions'][1],state,15,params,frame),2)
        self.assertEqual(guard(p,p['transitions'][1],state,3,params,frame),0)
    def test_orthogonal_permutation_preserves_prediction(self):
        p=self.program;cases,_=fixture();code=np.array(p['codes']);w=np.array(p['ridge']['weights'],np.uint16).view(np.float16).astype(np.float32)
        rng=np.random.default_rng(17);perm=rng.permutation(256);sign=rng.choice([-1,1],256)
        for state in range(len(p['sizes'])):
            for c in cases:
                a=active_features(p,state,c['ready'],c['parameters']);x=np.array(a)@code/train.divisor(sum(a))
                self.assertTrue(np.allclose(x@w,(x[perm]*sign)@(w[perm]*sign[:,None]),atol=1e-5))
        self.assertTrue(np.array_equal(np.sort(np.abs(w).ravel()),np.sort(np.abs(w[perm]*sign[:,None]).ravel())))
    def test_explicit_macro_step_and_late_ready_resume(self):
        p=self.program
        with tempfile.TemporaryDirectory() as d:
            path=Path(d)/'p.json';path.write_text(json.dumps(p));r=Resident(BUILD/'worker',path,'learned')
            try:
                r.send([1,1,11,12,13,14,3]);r.send([2,1]);a=decode(p,r.send([4]))[0]
                self.assertEqual((a['state'],a['events']),(1,[0]))
                r.send([2,3]);b=decode(p,r.send([4]))[0];self.assertEqual(b['events'],[0])
                r.send([3,1,0,12]);r.send([2,1]);a=decode(p,r.send([4]))[0];self.assertEqual(a['state'],2)
                r.send([2,1]);a=decode(p,r.send([4]))[0];self.assertEqual((a['state'],a['events']),(3,[0,1,2]))
                r.send([2,3]);b=decode(p,r.send([4]))[0];self.assertEqual(b['events'],a['events'])
            finally:r.close()

if __name__=='__main__':unittest.main()
