"""Physical representation equality must not discard history obligations."""
import unittest
from .compiler import representation_choices,transition_options
from .takeover import call
from .terms import text


def candidate(identifier,slot):
    literal=dict(kind='literal',sort='i64',text=str(slot))
    parameter=call('RipenInput',[literal]);root=call('A',[parameter]);other=call('B',[parameter])
    return dict(id=identifier,family=0,acts=[dict(kind='expr',expr=root)],roots=[root],
        params={text(parameter):'p0'},param_sorts={text(parameter):'E'},
        signatures={op:dict(input=inputs,output='E',kind='constructor') for op,inputs in [('A',['E']),('B',['E']),('RipenInput',['i64'])]},
        snapshot=dict(values=[dict(sort='i64',literal=str(slot)),dict(sort='E'),dict(sort='E'),dict(sort='E')],
            rows=[dict(op='RipenInput',args=[0],result=1),dict(op='A',args=[1],result=2),dict(op='B',args=[1],result=3)]),
        value_map=[0,1,2,3],origin=dict(symbolic_values=[],composition=dict(parts=[])))


class RepresentationChoices(unittest.TestCase):
    def test_certified_slot_renaming_shares_choice_and_keeps_both_entries(self):
        paths=[candidate(0,7),candidate(1,91)]
        reps,aliases=representation_choices(paths)
        self.assertEqual([r['id'] for r in reps],[0]);self.assertEqual(aliases,{0:0,1:0})
        full=transition_options(paths);reduced=transition_options(paths,reps)
        self.assertEqual(len(full),len(reduced));self.assertEqual(len(reduced),2)
        self.assertEqual([e['id'] for e in reduced],[0,1])
        for edge in reduced:
            self.assertEqual(len(edge['options']),1)
            self.assertEqual(edge['options'][0]['requires'],[0])

    def test_guards_root_order_types_and_root_identity_are_not_erased(self):
        a=candidate(0,7)
        changes=[]
        b=candidate(1,91);b['acts'].append(dict(kind='union',args=[b['roots'][0],call('B',b['roots'][0]['args'])]));changes.append(b)
        b=candidate(1,91);b['roots']=[call('B',b['roots'][0]['args']),b['roots'][0]];changes.append(b)
        b=candidate(1,91);b['param_sorts']={k:'Other' for k in b['params']};changes.append(b)
        b=candidate(1,91);b['value_map'][2]=3;changes.append(b)
        for b in changes:
            reps,_=representation_choices([a,b]);self.assertEqual(len(reps),2)

    def test_noninterface_literals_are_not_renamed_as_slots(self):
        paths=[candidate(0,7),candidate(1,91)]
        for p,value in zip(paths,[5,6]):
            p['roots'][0]['args'].append(dict(kind='literal',sort='i64',text=str(value)))
            p['signatures']['A']['input'].append('i64')
            p['snapshot']['values'].append(dict(sort='i64',literal=str(value)))
            p['snapshot']['rows'][1]['args'].append(4)
            p['value_map'].append(4)
        self.assertEqual(len(representation_choices(paths)[0]),2)


if __name__=='__main__':unittest.main()
