import copy,unittest
from eggfuse.ir.recipe import build,digest
from eggfuse.ir.batches import validate_batch
from eggfuse.analyzer.regions import from_core
from eggfuse.ir.test_recipe import E,I,var,call


def fixture():
    tables=[dict(name=n,kind='constructor',schema=[E,I,E],merge=None) for n in ['Seed','Middle','Done']]
    a=dict(name='producer',seminaive=True,body=[call('Seed',var('r'),var('n',I),var('f'))],head=[
        dict(op='let',dst='next',sort=I,call=dict(primitive='+',inputs=['i64','i64'],output='i64'),args=[var('n',I),dict(literal=2,sort=I)]),
        dict(op='let',dst='mid',sort=E,call={'table':'Middle'},args=[var('r'),var('next',I)])])
    b=dict(name='consumer',seminaive=True,body=[call('Middle',var('r'),var('n',I),var('f'))],head=[dict(op='let',dst='done',sort=E,call={'table':'Done'},args=[var('r'),var('n',I)])])
    return dict(schema='egglog-device-core/v1',tables=tables,rulesets=[dict(name='first',rules=[a]),dict(name='second',rules=[b])],
                schedule=[dict(saturate=dict(sequence=[dict(run='first'),dict(run='second')]))],initial=[])


class BatchTests(unittest.TestCase):
    def test_source_derived_multistage_and_finite_boundary(self):
        c=fixture();r=from_core(c);b=r['batch_program'];validate_batch(b,build(c),r)
        self.assertEqual(len(b['templates']),1);self.assertEqual(len(b['templates'][0]['steps']),2)
        c['schedule']=[dict(repeat=2,body=dict(sequence=[dict(run='first'),dict(run='second')]))]
        self.assertFalse(from_core(c)['batch_program']['templates'])

    def test_resigned_ssa_and_scope_corruption_are_rejected(self):
        c=fixture();r=from_core(c);b=r['batch_program']
        for kind in ['scope','ssa','dependency']:
            bad=copy.deepcopy(b);t=bad['templates'][0]
            if kind=='scope':bad['scopes'][0]['control']={'saturate':{'run':'nonexistent'}}
            elif kind=='ssa':t['steps'][0]['mapping'][3]={'slot':1}
            else:t['steps'][1]['requires']=[]
            bad['sha256']=digest({k:v for k,v in bad.items() if k!='sha256'})
            with self.assertRaises(ValueError):validate_batch(bad,build(c),r)

    def test_constructor_set_and_union_keep_typed_receipts(self):
        c=fixture();a=c['rulesets'][0]['rules'][0]
        a['head'].append(dict(op='set',call={'table':'Middle'},args=[var('r'),var('n',I)],value=var('r')))
        a['head'].append(dict(op='union',args=[var('r'),var('f')]))
        r=from_core(c);b=r['batch_program'];validate_batch(b,build(c),r)
        t=next(t for t in b['templates'] if t['steps'][0]['receipts'])
        receipt=t['steps'][0]['receipts'][0]
        self.assertNotEqual(receipt['before'],receipt['actual'])
        bad=copy.deepcopy(b);t=next(t for t in bad['templates'] if t['steps'][0]['receipts'])
        t['steps'][0]['receipts'][0]['actual']={'slot':0}
        bad['sha256']=digest({k:v for k,v in bad.items() if k!='sha256'})
        with self.assertRaisesRegex(ValueError,'receipt overwrites'):validate_batch(bad,build(c),r)

    def test_mutable_custom_merge_never_receives_stable_completion(self):
        from eggfuse.ir.batches import certifiable
        c=fixture();a=c['rulesets'][0]['rules'][0];c['tables'].append(dict(name='Best',kind='function',schema=[E,E],merge='new'))
        a['head'].append(dict(op='set',call={'table':'Best'},args=[var('r')],value=var('f')))
        r=build(c)
        self.assertFalse(certifiable(r['rules'][0],r['tables']))

    def test_ambiguous_wiring_remains_residual(self):
        c=fixture();a=c['rulesets'][0]['rules'][0]
        a['head'].append(dict(op='let',dst='another',sort=E,call={'table':'Middle'},args=[var('r'),var('n',I)]))
        self.assertFalse(from_core(c)['batch_program']['templates'])


if __name__=='__main__':unittest.main()
