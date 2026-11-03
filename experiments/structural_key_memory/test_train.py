import unittest
import numpy as np
from train import fit,heldout_split
from quadratic import lift


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

    def test_square_lift_keeps_routing_and_adds_no_external_data(self):
        c={'x':np.array([[1,-2,.5]],dtype=np.float16),'query':np.array([0])}
        lifted=lift(c)
        np.testing.assert_array_equal(lifted['x'],[[1,-2,.5,1,4,.25]])
        self.assertIs(lifted['query'],c['query'])
        self.assertTrue(lifted['quadratic'])

    def test_square_of_mixed_coordinates_can_express_xor(self):
        bits=np.array([[0,0],[0,1],[1,0],[1,1]],dtype=np.float32)
        x=(bits@np.array([[1,1],[1,-1]],dtype=np.float32)).astype(np.float16)
        c={'x':x,'domain':np.tile(np.arange(4),100),'query':np.arange(4)}
        wanted=np.array([[0],[1],[1],[0]],dtype=np.uint8)
        g={'payload':np.tile(wanted,(100,1)),'probes':np.arange(4)}
        held=np.zeros(400,dtype=bool)
        linear=fit(g,c,held)[0]>=.5;quadratic=fit(g,lift(c),held)[0]>=.5
        self.assertFalse(np.array_equal(linear,wanted))
        np.testing.assert_array_equal(quadratic,wanted)


if __name__=='__main__':unittest.main()
