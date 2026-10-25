import unittest
import numpy as np
from context_study import separate_blocks,preserve_local_columns


class BlockTests(unittest.TestCase):
    def test_equal_descendants_do_not_erase_distinct_local_shapes(self):
        left={('E',):{'x':np.array([[1,0],[0,1]],dtype=np.float16),'domain':np.array([0,1]),
            'query':np.array([1,0]),'hashes':[np.array([11]),np.array([22])],
            'signature_hash':np.array([111,222],dtype=np.uint64)}}
        right={('E',):{'x':np.array([[1,1]],dtype=np.float16),'domain':np.array([0,0]),
            'query':np.array([0,0]),'hashes':[np.array([33])],
            'signature_hash':np.array([333],dtype=np.uint64)}}
        c=separate_blocks(left,right)[('E',)]
        np.testing.assert_array_equal(c['x'],[[1,0,1,1],[0,1,1,1]])
        np.testing.assert_array_equal(c['query'],[1,0])

    def test_local_column_mask_preserves_scaling_and_other_outputs(self):
        coords={'x':np.array([[1,0,10,20]],dtype=np.float16),'query':np.array([0])}
        model={'w':np.ones((4,2),dtype=np.float16),'bias':np.zeros(2,dtype=np.float32)}
        local={'w':np.array([[2,4],[6,8]],dtype=np.float16),'bias':np.array([1,2],dtype=np.float32)}
        score=preserve_local_columns(coords,model,local,[0])
        self.assertEqual(float(score[0,0]),2.)
        self.assertEqual(float(score[0,1]),7.75)
        np.testing.assert_array_equal(model['w'][2:,0],0)


if __name__=='__main__':unittest.main()
