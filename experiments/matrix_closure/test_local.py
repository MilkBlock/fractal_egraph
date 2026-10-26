"""Logical-time equations must preserve synchronous, phased Run semantics."""
import random
import unittest
from dynamic import DynamicContext
from test_dynamic import core,row

NEVER=10**9
def times(n,clauses,initial,period,offset):
    values=[0 if i in initial else NEVER for i in range(n)]
    while True:
        after=values[:]
        for phase,deps,target in clauses:
            ready=max([values[x] for x in deps],default=0)
            if ready==NEVER:continue
            candidate=ready+1+(phase-1-(offset+ready)%period)%period
            after[target]=min(after[target],candidate)
        if values==after:return values
        values=after

class LogicalTime(unittest.TestCase):
    def test_same_run_is_not_an_internal_saturation(self):
        c=[(1,[],0),(1,[0],1),(2,[0],2),(3,[1,2],3)]
        self.assertEqual(times(4,c,set(),3,0),[1,4,2,6])

    def test_phase_origin_moves_after_external_update(self):
        self.assertEqual(times(2,[(1,[],0),(2,[],1)],set(),3,1),[3,1])

    def test_all_visible_frontiers_match_scheduled_execution(self):
        rng=random.Random(23)
        for _ in range(100):
            n=7;period=rng.randrange(1,5);offset=rng.randrange(period)
            clauses=[(rng.randrange(1,period+1),rng.sample(range(n),rng.randrange(4)),rng.randrange(n)) for _ in range(16)]
            initial={i for i in range(n) if rng.randrange(5)==0}
            predicted=times(n,clauses,initial,period,offset);visible=initial.copy()
            for t in range(1,n*period+period+1):
                phase=(offset+t-1)%period+1
                visible=visible|{target for p,deps,target in clauses if p==phase and all(x in visible for x in deps)}
                self.assertEqual(visible,{i for i,v in enumerate(predicted) if v<=t})

    def test_identical_clauses_keep_their_ruleset_phase(self):
        c=core();r=c['rulesets'][0]['rules'][0]
        c['rulesets']=[{'name':'A','rules':[r]},{'name':'B','rules':[r]}]
        d=DynamicContext(c,['A','B']);p=d.apply(upserts=[(0,row('Leaf','a'))])
        self.assertEqual(d.library()['period'],2)
        self.assertEqual(d.library()['schema'],'matrix-scheduled/v1')
        self.assertEqual(set(d.banks),{0,1});self.assertEqual(p['stats']['live_contexts'],2)
        self.assertEqual(len(d.coordinates),1)

    def test_repeated_ruleset_has_distinct_legal_phases(self):
        d=DynamicContext(core(),['r','r']);d.apply(upserts=[(0,row('Leaf','a'))])
        self.assertEqual(set(d.banks),{0,1});self.assertEqual(len(d.coordinates),1)

    def test_empty_schedule_is_rejected(self):
        with self.assertRaises(ValueError):DynamicContext(core(),[])

if __name__=='__main__':unittest.main()
