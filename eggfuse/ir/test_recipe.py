import copy,json,unittest,tempfile
from pathlib import Path
from .recipe import build,validate,instantiate,digest
from eggfuse.analyzer.recipes import adapt,decode

E={'name':'E','kind':'eq'}
I={'name':'i64','kind':'i64'}
def var(n,s=E):return dict(var=n,sort=s)
def call(t,*args):return dict(call={'table':t},args=list(args))
def fixture():
 return dict(schema='egglog-device-core/v1',tables=[dict(name='Node',kind='constructor',schema=[I,E],merge=None),dict(name='Pair',kind='constructor',schema=[E,E,E],merge=None)],rulesets=[dict(name='step',rules=[dict(name='grow',seminaive=True,body=[call('Node',var('n',I),var('x'))],head=[dict(op='copy',dst='y',sort=E,arg=var('x')),dict(op='let',dst='p',sort=E,call={'table':'Pair'},args=[var('y'),var('x')]),dict(op='set',call={'table':'Pair'},args=[var('x'),var('x')],value=var('p')),dict(op='union',args=[var('p'),var('x')])])])],schedule=[{'repeat':2,'body':{'run':'step'}}],assertion_tables=[],initial=[])

class RecipeTests(unittest.TestCase):
 def test_roundtrip_effects_and_finite_control(self):
  core=fixture();r=build(core);self.assertEqual(instantiate(r,core['initial']),core)
  self.assertEqual(r['rules'][0]['footprint']['union_sorts'],['E'])
  self.assertEqual(r['capabilities']['operations'],['construct','copy','set','union'])
 def test_tampered_wiring_and_contract_rejected_even_after_resigning(self):
  r=build(fixture())
  for mutation in [lambda x:x['rules'][0]['interface'].update(guaranteed_atoms=[0],residual_atoms=[]),lambda x:x['rules'][0]['effects'][1]['args'][0].update(slot=999),lambda x:x['authorization'].update(live_substitution_authorized=True),lambda x:x['rules'][0]['footprint'].update(union_sorts=[])]:
   bad=copy.deepcopy(r);mutation(bad);bad['sha256']=digest({k:v for k,v in bad.items() if k!='sha256'})
   with self.assertRaises((ValueError,KeyError)):validate(bad)
 def test_valid_other_program_cannot_replace_expected_source(self):
  core=fixture();other=copy.deepcopy(core);other['schedule'][0]['repeat']=3
  with self.assertRaises(ValueError):validate(build(other),core)
 def test_literal_and_slot_sorts_not_interchangeable(self):
  c=fixture();c['rulesets'][0]['rules'][0]['head'][-1]['args'][0]=var('n',I)
  with self.assertRaises(ValueError):build(c)
 def test_event_routes_are_shared_and_require_actual_merge_result(self):
  c=fixture();c['rulesets'][0]['rules'][0]['body'].append(call('Pair',var('x'),var('x'),var('p0')))
  r=build(c);routes=[e for e in r['event_routes'] if e['table']=='Pair'];self.assertEqual(len(routes),1)
  self.assertEqual(len(routes[0]['publishers']),2);self.assertIn('post-merge',routes[0]['payload'])
  self.assertFalse(routes[0]['runtime_subscription_installed'])
 def test_analyzer_adapter_does_not_grant_live_substitution(self):
  c=fixture();initial=c.pop('initial');old=dict(schema='analyzer-device-cells/v1',programs=[c],instances=[dict(unit=0,program=0,initial=initial)],source_program=dict(status='ready',core={**c,'initial':initial}))
  r=adapt(old);self.assertEqual(decode(r),old)
  with self.assertRaises(ValueError):validate(r['programs'][0],scope='original_program')
  old['instances'][0]['live_substitution_authorized']=True
  with self.assertRaises(ValueError):adapt(old)
 def test_cross_binding_aliases_remain_correlated(self):
  c=fixture();c['rulesets'][0]['rules'][0]['body']=[call('Pair',var('x'),var('x'),var('root'))]
  c['rulesets'][0]['rules'][0]['head']=[dict(op='union',args=[var('root'),var('x')])]
  r=build(c);columns=r['rules'][0]['interface']['rows'][0]['columns'];self.assertEqual(columns[0],columns[1]);self.assertNotEqual(columns[0],columns[2])

 def test_engine_loader_checks_the_recipe_against_actual_source(self):
  from eggfuse.engine.artifacts import validate_recipe_artifact
  c=fixture();r=build(c)
  with tempfile.TemporaryDirectory() as d:
   folder=Path(d);file=folder/'recipe-program.json';file.write_text(json.dumps(r))
   manifest={'files':{'recipe-program.json':'checked by outer loader'}}
   program={'recipe_ir':dict(schema=r['schema'],sha256=r['sha256'],scope=r['scope'])}
   validate_recipe_artifact(folder,manifest,program,c)
   other=copy.deepcopy(c);other['schedule'][0]['repeat']=3
   wrong=build(other);file.write_text(json.dumps(wrong));program['recipe_ir']['sha256']=wrong['sha256']
   with self.assertRaises(ValueError):validate_recipe_artifact(folder,manifest,program,c)

 def test_ambiguous_call_and_operand_are_rejected(self):
  c=fixture();c['rulesets'][0]['rules'][0]['body'][0]['call']['primitive']='+'
  with self.assertRaises(ValueError):build(c)
  c=fixture();c['rulesets'][0]['rules'][0]['body'][0]['args'][0]['literal']=3
  with self.assertRaises(ValueError):build(c)

if __name__=='__main__':unittest.main()
