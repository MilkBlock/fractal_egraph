import unittest
import numpy as np
from train import fit,heldout_split


class TrainingTests(unittest.TestCase):
    def test_held_out_labels_cannot_change_the_model(self):
        c={'x':np.array([[1,0],[0,1],[1,1]],dtype=np.float16),
           'domain':np.arange(3),'query':np.arange(3)}
        g={'payload':np.array([[1,0],[0,1],[1,1]],dtype=np.uint8),'probes':np.arange(3)}
        held=np.array([False,False,True]);a=fit(g,c,held)
        g['payload'][2]=0;b=fit(g,c,held)
        np.testing.assert_array_equal(a[2]['w'],b[2]['w'])
        np.testing.assert_array_equal(a[2]['bias'],b[2]['bias'])
        np.testing.assert_array_equal(a[0],b[0])

    def test_conflicting_signature_labels_are_frequency_weighted(self):
        c={'x':np.array([[1,1]],dtype=np.float16),'domain':np.array([0,0,0]),'query':np.array([0])}
        g={'payload':np.array([[1],[1],[0]],dtype=np.uint8),'probes':np.array([1])}
        score,_,m=fit(g,c,np.array([False,False,False]))
        self.assertAlmostEqual(float(score[0,0]),2/3,places=6)
        self.assertEqual(m['contradictory_profile_count'],1)

    def test_structure_split_keeps_entire_profiles_together(self):
        g={'keys':np.arange(1,7,dtype=np.uint64),'probes':np.arange(1,7,dtype=np.uint64)}
        c={'signature_hash':np.array([31,99],dtype=np.uint64),'domain':np.array([0,1,0,1,0,1])}
        held,_=heldout_split(g,c,'new_structure')
        self.assertEqual(len(set(held[::2])),1);self.assertEqual(len(set(held[1::2])),1)


if __name__=='__main__':unittest.main()
