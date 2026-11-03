import unittest
import numpy as np
from memory import Memory,codes,fingerprint,interpolation_parts


class MemoryTests(unittest.TestCase):
    def test_encoding_independent_of_dictionary(self):
        keys=np.array([fingerprint([('Eq','Expr','9')]),fingerprint([('Eq','Expr','10')])],dtype=np.uint64)
        np.testing.assert_array_equal(codes(keys,256)[0],codes(keys[:1],256)[0])
        self.assertTrue(np.all(np.abs(codes(keys,256))==1))

    def test_single_key_exact_self_response(self):
        m=Memory(np.array([123],dtype=np.uint64),np.array([[1,0,1]],dtype=np.uint8),center=False)
        scores,_=m.predict([123]);np.testing.assert_array_equal(scores,[[1,0,1]])

    def test_coefficient_update_equals_rebuild_with_fixed_prior(self):
        keys=np.array([5,7,11,13],dtype=np.uint64);y=np.array([[1,0],[0,1],[1,0],[0,1]],dtype=np.uint8)
        m=Memory(keys,y,width=128,parts=1,center=False)
        m.update_existing_key(7,[1,-1]);y[1]=[1,0]
        rebuilt=Memory(keys,y,width=128,parts=1,center=False)
        np.testing.assert_array_equal(m.weights,rebuilt.weights)

    def test_center_is_not_a_certificate_of_absence(self):
        m=Memory(np.array([123],dtype=np.uint64),np.array([[1]],dtype=np.uint8),parts=1,center=True)
        np.testing.assert_array_equal(m.weights,0)
        # A constant positive prior deliberately predicts unseen keys too.
        score,_=m.predict([999]);self.assertEqual(float(score[0,0]),1.)

    def test_baked_decoder_corrects_cross_talk_and_rejects_unsafe_update(self):
        rng=np.random.default_rng(9);keys=rng.integers(0,2**63,size=375,dtype=np.uint64)
        y=rng.integers(0,2,size=(375,19),dtype=np.uint8);parts=interpolation_parts(keys,128)
        m=Memory(keys,y,128,parts,fit=True);scores,_=m.predict(keys)
        np.testing.assert_array_equal(scores>=.5,y.astype(bool))
        self.assertLess(m.training_max_error,.02)
        with self.assertRaises(ValueError):m.update_existing_key(int(keys[0]),np.zeros(19))

    def test_conflicting_encoded_keys_are_not_certified(self):
        with self.assertRaises(ValueError):
            Memory(np.array([5,5],dtype=np.uint64),np.array([[0],[1]],dtype=np.uint8),128,1,fit=True)


if __name__=='__main__':unittest.main()
