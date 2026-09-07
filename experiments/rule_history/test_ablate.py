import unittest
import ablate


def sample(nodes, root='E-r'):
    children = {c for n in nodes for c in n['children']}
    return dict(root=root, nodes=nodes, child_ops={c:['Var'] for c in children}, history=[], round=0)


class StructureTests(unittest.TestCase):
    def test_global_sharing_must_not_disappear(self):
        shared = sample([dict(op='Add', children=['E-a','E-b']), dict(op='Mul', children=['E-a','E-b'])])
        separate = sample([dict(op='Add', children=['E-a','E-b']), dict(op='Mul', children=['E-c','E-d'])])
        a, b = ablate.prepare(shared), ablate.prepare(separate)
        self.assertEqual(a['strong'], b['strong'])  # Genuine hard negative for cheap features.
        self.assertNotEqual(a['truth'], b['truth'])

    def test_renaming_and_node_order_preserve_truth(self):
        a = sample([dict(op='Add', children=['E-a','E-b']), dict(op='Mul', children=['E-b','E-a'])])
        b = sample([dict(op='Mul', children=['E-z','E-q']), dict(op='Add', children=['E-q','E-z'])])
        self.assertEqual(ablate.prepare(a)['truth'], ablate.prepare(b)['truth'])

    def test_self_reference_is_fixed(self):
        a = sample([dict(op='Add', children=['E-r','E-a'])])
        b = sample([dict(op='Add', children=['E-b','E-a'])])
        self.assertNotEqual(ablate.prepare(a)['truth'], ablate.prepare(b)['truth'])

    def test_sort_is_preserved(self):
        a = sample([dict(op='Var', children=['i64-a'])])
        b = sample([dict(op='Var', children=['E-a'])])
        self.assertNotEqual(ablate.prepare(a)['truth'], ablate.prepare(b)['truth'])

    def test_past_and_noop_controls(self):
        x = sample([dict(op='Add', children=['E-a','E-b'])])
        x['round'] = 1
        x['history'] = [dict(round=0, rule='add-comm', bindings={'x':'E-a','y':'E-b','root':'E-r'}),dict(round=1, rule='noop-add', bindings={'x':'E-a','y':'E-b','root':'E-r'})]
        f = ablate.prepare(x)['features']
        self.assertEqual(len(f['roles']),2)
        self.assertEqual(len(f['past']),1)
        self.assertEqual(len(f['no_noop']),1)
        self.assertEqual(len(f['bindings']),1)

    def test_partition_runs_do_not_overlap(self):
        xs=[dict(replica=r,shape=s,schedule=c,round=t) for r in range(4) for s in range(16) for c in range(6) for t in range(4)]
        for mode in ['renamed','unseen_shape','unseen_schedule']:
            parts = [{(x['replica'],x['shape'],x['schedule']) for x in group} for group in ablate.split(xs,mode)]
            for a,b in [(0,1),(0,2),(1,2)]: self.assertFalse(parts[a]&parts[b])


if __name__ == '__main__': unittest.main()
