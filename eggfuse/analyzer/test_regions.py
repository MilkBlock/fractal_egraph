import copy
import unittest
from eggfuse.analyzer.regions import analyze,verify
from eggfuse.ir.recipe import build, digest
from eggfuse.ir.regions import validate_plan
from eggfuse.ir.test_recipe import fixture as core, call, var


class RegionTests(unittest.TestCase):
    def test_typed_program_and_order_are_verified(self):
        recipe=build(core());plan=analyze(recipe)
        self.assertEqual(plan['control'],recipe['control'])
        self.assertEqual(sum(len(s['rules']) for s in plan['rulesets']),sum(len(s['rule_ids']) for s in recipe['rulesets']))
        for field in ('control','routes','rules'):
            bad=copy.deepcopy(plan);bad[field]=[]
            with self.assertRaises(ValueError):verify(bad,recipe)

    def test_shared_spines_preserve_alias_wiring(self):
        c=core();r=c['rulesets'][0]['rules'][0]
        r['body'].append(call('Pair',var('x'),var('x'),var('p0')))
        other=copy.deepcopy(r);other['name']='another-consumer'
        def rename(x):
            if isinstance(x,dict):return {k:('renamed-'+v if k in ('var','dst') else rename(v)) for k,v in x.items()}
            if isinstance(x,list):return list(map(rename,x))
            return x
        c['rulesets'].append(dict(name='other-stage',rules=[rename(other)]))
        plan=analyze(build(c))
        self.assertEqual(plan['rules'][0]['variants'][0]['node'],plan['rules'][1]['variants'][0]['node'])
        self.assertIsNotNone(plan['rules'][0]['variants'][0]['node'])
        validate_plan(plan,build(c))
        for mutate in [lambda p:p['nodes'][-1].update(reads=[]),
                       lambda p:p['rules'][0]['variants'][0]['slots'].update(x=99),
                       lambda p:p['rules'][0]['variants'][0].update(table_atoms=[1,0])]:
            bad=copy.deepcopy(plan);mutate(bad);bad['sha256']=digest({k:v for k,v in bad.items() if k!='sha256'})
            with self.assertRaises(ValueError):validate_plan(bad,build(c))
        # Removing the repeated-slot condition creates a fanout. It must not
        # inherit the old keyed Spine guarantee.
        c['rulesets'][1]['rules'][0]['body'][1]['args'][1]=var('independent')
        changed=analyze(build(c))
        self.assertIsNone(changed['rules'][1]['variants'][0]['node'])

    def test_region_partition_keeps_every_rule(self):
        plan=analyze(build(core()))
        members=[r for region in plan['regions'] for r in region['rules']]
        self.assertEqual(sorted(members),list(range(len(plan['rules']))))
        for rule in plan['rules']:
            for variant in rule['variants']:
                if variant['node'] is not None:
                    node=plan['nodes'][variant['node']]
                    self.assertEqual(node['tables'],len(variant['prefix']))
                    self.assertEqual(len(node['sorts']),len(variant['slots']))


if __name__=='__main__':unittest.main()
