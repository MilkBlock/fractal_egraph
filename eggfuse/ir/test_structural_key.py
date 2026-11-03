"""Shared key transport and source-derived merge permissions."""
import copy
import json
import unittest

from .structural_key import ABI, arena_key, decode_key_literal, key_token, merge_contracts, validate_core


class StructuralKeyTests(unittest.TestCase):
    def test_replay_literals_keep_the_same_metal_decode_program(self):
        from eggfuse.compiler.targets.metal.structural_keys import layout
        wire=dict(nodes=[dict(anchor=False,label='leaf',children=[]),
                         dict(anchor=False,label='Pair',children=[0,0])],root=1)
        raw=json.dumps(wire)
        legacy=json.dumps([False,'Pair',[[False,'leaf',[]],[False,'leaf',[]]]])
        encoded='dag-v1:'+raw.encode().hex()
        strings=[encoded,raw,legacy,'not a key']
        core=dict(tables=[dict(schema=[dict(kind='structural_key')])],
                  structural_key=dict(nodes=[],string_labels=[dict(value=s,label=repr(s)) for s in strings]))
        decoded=layout(core,1,strings)['decoded']
        for value in decoded[:3]:self.assertEqual(key_token(value),key_token(wire))
        self.assertIsNone(decoded[3])
        for bad in ['dag-v1:0','dag-v1:gg','dag-v1:7b7d',json.dumps(dict(root=0,nodes=[dict(anchor=False,label='cycle',children=[0])]))]:
            with self.assertRaises((ValueError,KeyError)):decode_key_literal(bad)

    def test_wire_sharing_does_not_change_identity(self):
        leaf = dict(anchor=False, label='i64:7', children=[])
        shared = dict(nodes=[leaf, dict(anchor=False, label='Pair', children=[0, 0])], root=1)
        copied = dict(nodes=[leaf, leaf, dict(anchor=False, label='Pair', children=[0, 1])], root=2)
        self.assertEqual(key_token(shared), key_token(copied))
        bad = copy.deepcopy(shared)
        bad['nodes'][1]['children'][0] = 1
        with self.assertRaises(ValueError):
            key_token(bad)

    def test_gpu_height_is_checked(self):
        # Header followed by a one-byte label and no children.
        arena = [14, 0, 0, 0, 0, 0, 0, 0, 6, 0, 0, 1, 0, ord('A')]
        self.assertEqual(arena_key(arena, 8)['nodes'][0]['label'], 'A')
        arena[10] = 2
        with self.assertRaises(ValueError):
            arena_key(arena, 8)

    def test_abi_and_primitive_types_are_not_optional(self):
        core = dict(tables=[dict(schema=[dict(kind='structural_key')])],
                    structural_key=dict(abi=ABI),
                    op=dict(primitive='cell-key-base-i64', inputs=['i64'], output='CellStructuralKey'))
        validate_core(core)
        wrong = copy.deepcopy(core)
        wrong['op']['inputs'] = ['f64']
        with self.assertRaises(ValueError):
            validate_core(wrong)
        del wrong['structural_key']
        with self.assertRaises(ValueError):
            validate_core(wrong)

    def test_function_reads_do_not_become_constructor_effects(self):
        read = dict(call=dict(table='Score'), args=[dict(var='old')])
        expr = dict(call=dict(table='Pair'), args=[read, dict(var='new')])
        core = dict(tables=[dict(name='Score', kind='function'),
                            dict(name='Pair', kind='constructor'),
                            dict(name='Winner', kind='function', merge_expr=expr)])
        c = merge_contracts(core)['tables'][-1]
        self.assertEqual(c['reads'], ['Score'])
        self.assertEqual(c['constructs'], ['Pair'])
        self.assertEqual(c['kind'], 'ordered')
        self.assertEqual(c['equal_inputs'], 'return_old')
        # A primitive root still evaluates nested reads when old == new.
        core['tables'][-1]['merge_expr']['call'] = dict(primitive='+')
        c = merge_contracts(core)['tables'][-1]
        self.assertEqual(c['equal_inputs'], 'evaluate')
        self.assertEqual(c['reads'], ['Score'])
        self.assertEqual(c['constructs'], [])


if __name__ == '__main__':
    unittest.main()
