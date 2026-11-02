"""The pinned experiment keeps mutable If visibility in the real consumer."""
import copy,unittest
from .marker_shape import transform,OLD,NEW
from tools.cell_spine import takeover as t
from tools.cell_spine.terms import text

def fixture(reverse=False,related=True):
    v=t.var
    fields=['s0','s1','s2','s3','s4']
    eq=lambda a,b:dict(kind='eq',args=[a,b])
    guarantee=[eq(v('s0'),t.call('Get',[v('s1'),v('s2')])),eq(v('s3'),t.call('Get',[v('s4'),v('s2')]))]
    args=[v('u'),v('x'),v('i'),v('z'),v('y')]
    if reverse:args=[v('z'),v('y'),v('i'),v('u'),v('x')]
    parent=eq(v('f'),t.call('If',[v('p'),v('inputs'),v('x'),v('y' if related else 'other')]))
    rule=dict(command_ordinal=2,name='consumer',ruleset='work',body=[parent,dict(kind='fact',expr=t.call(OLD,args))],
              head=[dict(kind='expr',expr=t.call('Out',[v('u'),v('z')])),dict(kind='set',head=OLD,args=args,value=dict(kind='literal',sort='Unit',text='()'))])
    complete=dict(command_ordinal=1,name='complete-'+OLD,ruleset='cell-spine-complete',body=guarantee,head=[])
    data=dict(rules=[complete,rule],commands=[dict(ordinal=0,kind='declaration',text=f'(function {OLD} (Expr Expr i64 Expr Expr) Unit :merge old)'),
            dict(ordinal=1,kind='rule',text='old-completion'),dict(ordinal=2,kind='rule',text='old-consumer')])
    marker=dict(name=OLD,body=guarantee,parameters=fields,sorts=['Expr','Expr','i64','Expr','Expr'],completion_body=guarantee)
    return data,dict(markers=[marker])

class MarkerShapeTests(unittest.TestCase):
    def test_if_guard_and_full_binding_are_retained_in_both_orientations(self):
        for reverse in [False,True]:
            data,report=fixture(reverse)
            code,out=transform(data,report,'','if')
            self.assertIn('(= f (If p inputs x y))',code)
            self.assertIn(f'({NEW} u x i z y)',code)
            self.assertIn('(Out u z)',code)
            self.assertNotIn(OLD,code)
            self.assertTrue(out['experimental_shape']['conditional_view_is_not_a_certified_marker'])
            self.assertEqual(out['markers'],[])

    def test_no_unrelated_if_is_used_to_filter_the_original_query(self):
        data,report=fixture(related=False)
        code,out=transform(data,report,'','if')
        consumer=code.split(':name "experiment-if-get-complete")')[-1]
        self.assertNotIn(NEW,consumer)
        self.assertIn('(Get x i)',consumer);self.assertIn('(Get y i)',consumer)
        self.assertEqual(out['experimental_shape']['changed_rules'][0]['replacements'],['expanded'])

    def test_removal_expands_dependent_prefixes_and_keeps_their_guarantee(self):
        data,report=fixture()
        longer=copy.deepcopy(report['markers'][0]);longer.update(name='Longer',parent=OLD,
            completion_body=[dict(kind='fact',expr=t.call(OLD,[t.var(s) for s in longer['parameters']]))])
        report['markers'].append(longer)
        _,out=transform(data,report,'','remove')
        self.assertEqual(out['markers'][0]['body'],longer['body'])
        self.assertEqual(out['markers'][0]['completion_body'],longer['body'])

    def test_a_different_marker_layout_is_rejected(self):
        data,report=fixture();report['markers'][0]['body'][1]['args'][1]['args'][1]=t.var('s1')
        with self.assertRaises(AssertionError):transform(data,report,'','if')

if __name__=='__main__':unittest.main()
