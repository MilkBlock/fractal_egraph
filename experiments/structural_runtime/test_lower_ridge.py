import copy,unittest
import numpy as np
from lower_ridge import lower,reference

class LowerTests(unittest.TestCase):
    def fixture(self):
        rng=np.random.default_rng(10)
        return {'vocabulary':[{'hash':i+31,'token':['head',str(i)]} for i in range(9)],
                'models':[{'sort':'A','sort_hash':991,'weights':(rng.normal(size=256)*8).astype(np.float16).view(np.uint16).tolist(),'bias':.13,'excluded_tokens':list(range(i))} for i in [0,4,9]]}
    def test_same_model_and_masked_normalization(self):
        source=self.fixture();original=copy.deepcopy(source);new=lower(source);self.assertEqual(source,original);self.assertEqual(new['models'],source['models'])
        bank=new['resident_matrix'];a=np.array(bank['weights'],dtype=np.uint16).view(np.float16).astype(np.float64).reshape(10,3,3)
        for mask in range(512):
            features=[i for i in range(9) if mask>>i&1];z=np.zeros(10);z[features+[9]]=1;y=z@a.reshape(10,9);y=y.reshape(3,3)
            expected=reference(source,'A',features)
            for m in range(3):
                d=1
                while d*d<y[m,2]:d*=2
                self.assertAlmostEqual((y[m,0]+y[m,1])/d+.13,expected[m],delta=2e-5)
    def test_invalid_masks_and_dimension_are_rejected(self):
        model=self.fixture();model['models'][0]['excluded_tokens']=[20]
        with self.assertRaises(ValueError):lower(model)

if __name__=='__main__':unittest.main()
