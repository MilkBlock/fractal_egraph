"""Do not mistake window discovery or marker retirement for Cell merging."""
import unittest
from .spine_bench import merge_rules
from tools.cell_spine.takeover import call,var


def marker(f,x):return call(f'Summary-{f}!', [var(x)])
def rule(reads,head):return dict(name='r',body=[dict(kind='fact',expr=r) for r in reads],head=head)


class MergeEvidence(unittest.TestCase):
    def test_two_instances_of_same_family_still_count_as_two_inputs(self):
        r=rule([marker(0,'a'),marker(0,'b')],[dict(kind='expr',expr=marker(1,'out'))])
        result=merge_rules([r],{0})
        self.assertEqual(result[0]['source_markers'],2)
        self.assertEqual(result[0]['source_families'],[0,0])
        self.assertTrue(result[0]['uses_window_only_family'])

    def test_duplicate_atom_and_retirement_are_not_merge_evidence(self):
        r=rule([marker(0,'a'),marker(0,'a')],[dict(kind='expr',expr=marker(1,'out'))])
        self.assertEqual(merge_rules([r],{0}),[])
        r=rule([marker(0,'a'),marker(1,'b')],[dict(kind='change',change='subsume',head='Summary-0!',args=[var('a')])])
        self.assertEqual(merge_rules([r],{0}),[])

    def test_window_destination_does_not_claim_a_window_source(self):
        r=rule([marker(0,'a'),marker(1,'b')],[dict(kind='expr',expr=marker(2,'out'))])
        result=merge_rules([r],{2})
        self.assertFalse(result[0]['uses_window_only_family'])


if __name__=='__main__':unittest.main()
