import json
import os
from pathlib import Path
import tempfile
import unittest
from features import identity, stage_spans
from scale_state import DeltaState, Lineage, answers, changes, holdout_group
from study import Oracle


class ScaleTests(unittest.TestCase):
    def test_lineage_split_keeps_all_alias_versions_together(self):
        u=Lineage();a=('Expr','eq','a');b=('Expr','eq','b');c=('Expr','eq','c')
        u.apply([{'sort':'Expr','old':'a','now':'b'},{'sort':'Expr','old':'b','now':'c'}])
        self.assertEqual(u.find(a),c);self.assertEqual(holdout_group(u.find(a)),holdout_group(u.find(b)))

    def test_union_representative_change_is_not_a_new_payload(self):
        u=Lineage();k=('Expr','eq','root');a=('Type','eq','a');b=('Type','eq','b')
        u.apply([{'sort':'Type','old':'a','now':'b'}])
        gained,lost,*_=changes([k],{k:{(a,)}},{k:{(b,)}},u)
        self.assertEqual(gained,[False]);self.assertEqual(lost,[False])

    def test_function_payload_change_inside_same_class_is_observed(self):
        core={'tables':[{'name':'Pick','kind':'function','schema':[{'name':'Expr'},{'name':'Pair','kind':'eq'}]},
                        {'name':'Candidate','kind':'constructor','schema':[{'name':'i64'},{'name':'Pair'}]}]}
        e={'sort':'Expr','eq':'e'};p={'sort':'Pair','eq':'p'}
        row=lambda t,vs:{'table':t,'values':vs,'subsumed':False}
        before={'rows':[row('Pick',[e,p]),row('Candidate',[{'sort':'i64','value':9},p])]}
        after={'rows':before['rows']+[row('Candidate',[{'sort':'i64','value':4},p])]}
        a=answers(before,core,'Pick');b=answers(after,core,'Pick')
        self.assertTrue(changes([identity(e)],a,b,Lineage())[0][0])

    def test_delta_rejects_unknown_removal(self):
        s=DeltaState()
        with self.assertRaises(ValueError):s.apply({'tables':['T'],'values':[],'remove':[[0,0,0]],'insert':[],'rows':0,'aliases':[]})

    @unittest.skipUnless(os.environ.get('POLYSPINE_NATIVE_FRONTEND'),'native container lineage audit')
    def test_new_container_inner_value_lineage(self):
        with tempfile.TemporaryDirectory() as d:
            d=Path(d);source=d/'input.egg'
            source.write_text('(datatype E (N i64))\n(sort ES (Set E))\n(constructor Bag (ES) E)\n(ruleset r)\n(let a (N 1))\n(let b (N 2))\n(Bag (set-of a b))\n(run-schedule (run r))\n')
            native=Oracle(Path(os.environ['POLYSPINE_NATIVE_FRONTEND']),source,d);s=DeltaState()
            try:
                s.apply(native.ask({'op':'observe_tables','tables':['Bag']}))
                native.ask({'op':'edit_actions','source':'(let later (N 3)) (Bag (set-of later))'})
                s.apply(native.ask({'op':'observe_delta'}))
                v=next(r['values'][0]['elements'][0] for r in s.snapshot()['rows'] if len(r['values'][0]['elements'])==1)
                old=identity(v)
                native.ask({'op':'edit_actions','source':'(union later a)'})
                s.apply(native.ask({'op':'observe_delta'}))
                self.assertNotEqual(s.lineage.find(old),old)
            finally:native.close()

    def test_all_saturation_visits_are_retained(self):
        schedule=[{'repeat':2,'body':{'saturate':{'run':'is-resolved'}}}]
        # First saturate changes, so outer Repeat enters it again.
        trace=[{'set':'is-resolved','updated':True},{'set':'is-resolved','updated':False},{'set':'is-resolved','updated':False}]
        a,_=stage_spans(schedule,trace,False);b,_=stage_spans(schedule,trace,True)
        self.assertEqual(len(a),2);self.assertEqual(len(b),1)

    @unittest.skipUnless(os.environ.get('POLYSPINE_NATIVE_FRONTEND'),'native observer audit')
    def test_native_compact_matches_full_snapshot_after_union_set_subsume(self):
        with tempfile.TemporaryDirectory() as d:
            native=Oracle(Path(os.environ['POLYSPINE_NATIVE_FRONTEND']),Path(__file__).parents[1]/'matrix_closure/dynamic_fixture.egg',Path(d))
            names=['Leaf','Base','Blocked','Forward','Pair','Ready','Selected'];state=DeltaState()
            try:
                state.apply(native.ask({'op':'observe_tables','tables':names}))
                for action in [None,'(union blocked (Blocked 3))','(set (Selected key) (Blocked 40))','(subsume (Leaf 99))','(let newer (Forward a))']:
                    if action:native.ask({'op':'edit_actions','source':action})
                    native.ask({'op':'run','set':'closure'})
                    state.apply(native.ask({'op':'observe_delta'}))
                    allrows=native.ask({'op':'snapshot'})['rows']
                    normalize=lambda rows:sorted(json.dumps(r,sort_keys=True) for r in rows)
                    self.assertEqual(normalize(state.snapshot()['rows']),normalize([r for r in allrows if r['table'] in names]))
                self.assertTrue(state.lineage.parent)
            finally:native.close()


if __name__=='__main__':unittest.main()
