"""Short identifiers preserve roles and never rewrite payload strings."""
import copy
import unittest
from . import names
from .compiler import compatible_program,fork_definitions,ast,ROOT
from .terms import text


class ShortNames(unittest.TestCase):
    def test_raw_canonical_and_marker_roles_are_distinct(self):
        self.assertEqual(names.raw('Add'),'Add!')
        self.assertEqual(names.canonical('Add'),'Add!!')
        self.assertEqual(names.summary(3),'Summary-3!')
        self.assertTrue(names.is_raw('Add!',{'Add':{}}))
        self.assertFalse(names.is_raw('Add!!',{'Add':{}}))
        self.assertFalse(names.is_raw('Summary-3!'))
        self.assertFalse(names.is_raw('Reified-Term!'))
        self.assertEqual(names.family('Summary-3!'),3)
        self.assertEqual(names.family('Spine-Summary-3'),3)
        self.assertFalse(names.is_summary('Summary-3!!'))
        self.assertFalse(names.is_summary('Summary-x!'))

    def test_payload_and_variable_names_are_not_text_replaced(self):
        e=dict(kind='call',head='Leaf',args=[dict(kind='literal',sort='String',text='"Spine-Add Add!"')])
        before=copy.deepcopy(e)
        self.assertEqual(text(e,True,{'Leaf':dict(kind='constructor')}),'(Leaf! "Spine-Add Add!")')
        self.assertEqual(e,before)
        self.assertEqual(text(dict(kind='var',name='Spine-root'),True,{}),'Spine-root')

    def test_only_equivalence_sorts_get_forked(self):
        sig={'Leaf':dict(input=['String'],output='Term'),'Add':dict(input=['Term','Term'],output='Term')}
        source='\n'.join(fork_definitions(sig))
        self.assertIn('(constructor Leaf! (String) Term!)',source)
        self.assertIn('(constructor Add! (Term! Term!) Term!)',source)
        self.assertNotIn('String!',source)
        self.assertNotIn('Spine-',source)

    def test_reserved_generated_names_are_rejected_before_analysis(self):
        import tempfile
        from pathlib import Path
        with tempfile.TemporaryDirectory(dir=ROOT/'out',prefix='name-collision-') as tmp:
            for op in ('A!','Summary-0','Reified-Term','Spine-A'):
                source=Path(tmp)/'input.egg'
                source.write_text(f'(datatype Term ({op}) (B)) (ruleset r) '
                    f'(rewrite ({op}) (B) :ruleset r) ({op}) (run-schedule (saturate r))')
                data=ast(source,Path(tmp)/'input.json')
                self.assertIn('reserved Cell namespace',compatible_program(data))


if __name__=='__main__':unittest.main()
