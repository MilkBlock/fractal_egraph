"""Representation contracts must not confuse semantic closure with stored rows."""
import unittest
from . import markers
from .representation import raw_contracts,extend_layouts
from .takeover import var,call
from .terms import text


def path(f,root):
    c=dict(id=f,family=f,params={'x':'p0'},roots=[root],acts=[])
    c['marker_layouts']=markers.layouts(c)
    return c


def rule(name,body,target):
    return dict(name=name,ruleset='Spine-Run',body=body,head=[dict(kind='expr',expr=target)])


def fact(e):return dict(kind='fact',expr=e)

def summary(f,root,x):return call(f'Spine-Summary-{f}',[root,x])


class RepresentationProofs(unittest.TestCase):
    def test_every_producer_must_supply_a_borrowed_row(self):
        x=var('x');a=call('Spine-A',[x]);b=call('Spine-B',[x])
        cs={0:path(0,call('A',[x])),1:path(1,call('B',[x]))}
        ingress=rule('input',[fact(a)],summary(0,a,x))
        existing=rule('existing',[fact(b)],summary(1,b,x))
        generated=rule('new',[fact(summary(0,var('r'),x))],summary(1,b,x))
        proof=raw_contracts(cs,[ingress,existing,generated])
        self.assertEqual(proof[0],{'(Spine-A p0)'})
        self.assertEqual(proof[1],set())
        extended=extend_layouts(dict(chosen=cs,consumers=[ingress,existing,generated]))
        self.assertFalse(any(l.get('representation') for l in extended['chosen'][1]['marker_layouts']))

    def test_self_transition_preserves_but_does_not_invent_raw_contract(self):
        x=var('x');a=call('Spine-A',[x]);cs={0:path(0,call('A',[x]))}
        preserve=rule('self',[fact(summary(0,var('r'),x))],summary(0,var('r'),x))
        self.assertEqual(raw_contracts(cs,[rule('input',[fact(a)],summary(0,a,x)),preserve])[0],{'(Spine-A p0)'})
        hidden=rule('hidden',[fact(call('Spine-B',[x]))],summary(0,a,x))
        self.assertEqual(raw_contracts(cs,[hidden,preserve])[0],set())
        self.assertEqual(raw_contracts(cs,[])[0],set())

    def test_equal_results_do_not_prove_raw_row_arguments_equal(self):
        x,y=var('x'),var('y');a=call('Spine-A',[x]);b=call('Spine-B',[y])
        cs={0:path(0,call('A',[x])),1:path(1,call('B',[x]))}
        rules=[rule('input',[fact(a)],summary(0,a,x)),
            rule('output-union',[fact(summary(0,var('r'),x)),dict(kind='eq',args=[var('r'),b])],
                 summary(1,call('Spine-B',[x]),x))]
        self.assertFalse(raw_contracts(cs,rules)[1])

    def test_borrowed_source_is_not_evidence_of_a_canonical_table_row(self):
        x=var('x');cs={0:path(0,call('A',[x])),1:path(1,call('A',[x]))}
        ingress=rule('input',[fact(call('Spine-A',[x]))],summary(0,call('Spine-A',[x]),x))
        graft=rule('copy',[fact(summary(0,var('r'),x))],summary(1,var('r'),x))
        bundle=extend_layouts(dict(chosen=cs,consumers=[ingress,graft]))
        selected={0:next(l for l in bundle['chosen'][0]['marker_layouts'] if l.get('representation')=='borrowed'),
                  1:cs[1]['marker_layouts'][0]}
        sig={'A':dict(input=['T'],output='T')}
        emitted=markers.render(markers.lower_ast(graft,selected,sig))
        self.assertIn('(Spine-Cano-A',emitted)
        self.assertIn('(union',emitted)

    def test_borrowing_preserves_new_union_between_existing_roots(self):
        x=var('x');a=call('Spine-A',[x]);b=call('Spine-B',[x])
        cs={0:path(0,call('A',[x]))}
        r=rule('alias',[dict(kind='eq',args=[var('a'),a]),dict(kind='eq',args=[var('b'),b])],summary(0,var('b'),x))
        bundle=extend_layouts(dict(chosen=cs,consumers=[r]))
        selected={0:next(l for l in bundle['chosen'][0]['marker_layouts'] if l.get('representation')=='borrowed')}
        sig={k:dict(input=['T'],output='T') for k in ('A','B')}
        emitted=markers.lower_ast(r,selected,sig)
        self.assertTrue(any(a['kind']=='union' and a['args'][0]!=a['args'][1] for a in emitted['head']))
        # Missing this union leaves the marker rooted at B while its decoder
        # expects A. Querying the seed cannot silently replace the effect.

    def test_hidden_shared_binding_is_not_erased_in_multi_owner_join(self):
        x=var('x');cs={0:path(0,call('A',[x])),1:path(1,call('B',[x]))}
        r=rule('join',[fact(summary(0,var('a'),x)),fact(summary(1,var('b'),x))],call('Spine-Out',[]))
        sig={k:dict(input=['T'],output='T') for k in ['A','B','Out']}
        emitted=markers.render(markers.lower_ast(r,{f:c['marker_layouts'][0] for f,c in cs.items()},sig))
        self.assertIn('(Spine-Cano-A x)',emitted)
        self.assertIn('(Spine-Cano-B x)',emitted)


if __name__=='__main__':unittest.main()
