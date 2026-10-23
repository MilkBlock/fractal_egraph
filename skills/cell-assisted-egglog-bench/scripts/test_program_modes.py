import copy
from pathlib import Path
import unittest
import program_modes as p


def base(x):return ['i64',{'base':str(x)}]
def eq(x):return ['E',{'eq':str(x)}]
def graph(a=1,b=2):
    return dict(tables={'Leaf':dict(input=['i64'],output='E',constructor=True),
                        'F':dict(input=['E'],output='i64',constructor=False)},
                rows=[['Leaf',[base(1)],eq(a),False],['Leaf',[base(2)],eq(b),False],['F',[eq(a)],base(7),False]])


class ProgramModes(unittest.TestCase):
    def test_ids_are_not_semantics(self):
        self.assertEqual(p.compare(graph(),graph(100,200))['status'],'equal')
    def test_function_updates_and_visibility_are_observable(self):
        a=graph();b=graph();b['rows'][-1][2]=base(9)
        self.assertEqual(p.compare(a,b)['reason'],'function/base value differs')
        b=graph();b['rows'][-1][-1]=True
        self.assertEqual(p.compare(a,b)['reason'],'subsumption differs')
    def test_union_partition_is_observable(self):
        self.assertEqual(p.compare(graph(),graph(1,1))['reason'],'equality partition differs')
    def test_only_additional_tables_can_be_projected_away(self):
        a=graph();b=graph();b['tables']['Marker']=dict(input=['E'],output='i64',constructor=False)
        b['rows'].append(['Marker',[eq(1)],base(3),False])
        self.assertEqual(p.compare(a,b)['status'],'different')
        proof=p.compare(a,b,set(a['tables']))
        self.assertEqual(proof['status'],'equal');self.assertEqual(proof['projected_out_right'],['Marker'])
        del b['tables']['F'];self.assertEqual(p.compare(a,b,set(a['tables']))['status'],'different')
    def test_new_original_sort_alternatives_are_not_silently_ignored(self):
        a=graph();b=graph();b['tables']['Shortcut']=dict(input=['E'],output='E',constructor=True)
        b['rows'].append(['Shortcut',[eq(1)],eq(1),False])
        self.assertEqual(p.compare(a,b,set(a['tables']))['status'],'inconclusive')
    def test_unknown_cycles_are_not_declared_equal(self):
        a=dict(tables={'Loop':dict(input=['E'],output='E',constructor=True)},rows=[['Loop',[eq(1)],eq(1),False]])
        self.assertEqual(p.compare(a,a)['status'],'inconclusive')
