import unittest
from eggfuse.analyzer.congruence import build

E = {'name': 'E', 'kind': 'eq'}
def recipe(ops):
    return {'tables': [{'name': t, 'kind': 'constructor'} for t in ('F', 'G')],
            'rules': [{'slots': [{'sort': E} for _ in range(2 + len(ops))], 'effects': ops}]}
def make(dst, src, table='F'):
    return {'op': 'let', 'call': {'table': table}, 'dst': dst, 'args': [{'slot': src}]}

class CongruenceTests(unittest.TestCase):
    def test_nested_conclusions_share_one_boundary_condition(self):
        p = build(recipe([make(2,0), make(3,1), make(4,2,'G'), make(5,3,'G')]))
        groups = p['templates'][0]['groups']
        self.assertEqual(len(groups), 1)
        self.assertEqual(groups[0]['guards'], [[{'slot':0}, {'slot':1}]])
        self.assertEqual(groups[0]['merges'], [
            {'left':0, 'right':1, 'requires':[0,1]},
            {'left':2, 'right':3, 'requires':[0,1,2,3]}])

    def test_single_chain_has_no_pairwise_alias_enumeration(self):
        p = build(recipe([make(i+2, 0 if i==0 else i+1) for i in range(100)]))
        self.assertEqual(p['templates'], [])

    def test_constants_and_constructor_types_are_preserved(self):
        a, b = make(2,0), make(3,1)
        a['args'] = [{'literal': 1, 'sort': {'name':'i64','kind':'i64'}}]
        b['args'] = [{'literal': 2, 'sort': {'name':'i64','kind':'i64'}}]
        self.assertEqual(build(recipe([a,b]))['templates'], [])
        self.assertEqual(build(recipe([make(2,0),make(3,1,'G')]))['templates'], [])

    def test_copy_wiring_is_normalized(self):
        p = build(recipe([{'op':'copy','dst':2,'arg':{'slot':0}},make(3,2),make(4,1)]))
        self.assertEqual(p['templates'][0]['groups'][0]['guards'], [[{'slot':0}, {'slot':1}]])

if __name__ == '__main__': unittest.main()
