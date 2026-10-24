import unittest
from known_evidence import live_masks


class LiveProjection(unittest.TestCase):
    def graph(self):
        rows=[(0,True,[2]),(0,True,[3]),(1,True,[]),(2,True,[1,4]),(3,True,[1,5]),
              (4,False,[6,8]),(5,False,[7]),(6,False,[9]),(7,False,[10]),
              (8,False,[]),(9,True,[1,8]),(10,True,[1,8]),(11,False,[9])]
        return [{'class':c,'effectful':e,'children':ch} for c,e,ch in rows]

    def test_follows_both_future_branches_and_indirect_pure_inputs(self):
        masks=live_masks(self.graph(),{1,8},0)
        self.assertEqual(masks[1],{4,5,6,7})
        self.assertNotIn(8,masks[1])  # Common initial data needs no per-state bit.
        self.assertNotIn(11,masks[1]) # Extractable later, but no future effect uses it.
        self.assertEqual(masks[0],set())

    def test_projection_does_not_delete_known_facts(self):
        known={1,8,9,6,11}
        result=known & live_masks(self.graph(),{1,8},0)[1]
        self.assertEqual(result,{6})
        self.assertIn(11,known)


if __name__=='__main__':unittest.main()
