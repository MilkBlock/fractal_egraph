import unittest
from bench_dynamic import groups,bank_sizes

class TimingContract(unittest.TestCase):
    def test_batching_respects_visibility_and_quiet_boundaries(self):
        def run(i,updated=True,epoch=1):return {'op':'run','native_run':i,'updated':updated,'epoch':epoch}
        ops=[run(1),run(2,False),run(3),run(5),{'op':'patch'},run(6,epoch=2),run(7,epoch=2)]
        actual=list(groups(ops,True))
        self.assertEqual([([x['native_run'] for x in g] if isinstance(g,list) else 'patch') for g in actual],
                         [[1,2],[3],[5],'patch',[6,7]])
        self.assertEqual(len(list(groups(ops,False))),len(ops))

    def test_layout_accounting_tracks_growth_and_retirement(self):
        banks={}
        bank_sizes([1,0,1,2,2,0,0,1,0,0,1,0,1,1,0,1,1,0],banks)
        self.assertEqual(banks,{1:1})
        bank_sizes([1,1,2,2,0,0,0,1,1,1,0,0],banks)
        self.assertEqual(banks,{})

if __name__=='__main__':unittest.main()
