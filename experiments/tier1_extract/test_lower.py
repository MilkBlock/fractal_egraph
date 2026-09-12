import unittest
from lower import Lower,Unsupported

def fixture():
    nodes={}
    def put(t):
        if isinstance(t,list):op=t[0];children=[put(x) for x in t[1:]];sort='T'
        else:
            import json
            op=json.dumps(t);children=[];sort='i64' if isinstance(t,int) else 'String'
        key=str(len(nodes));nodes[key]={'op':op,'children':children,'eclass':sort+'-'+key};return key
    root=['CoarseComb',['MoreParents',['Empty'],['NoParents']],['Rule','S'],
          ['PCons',['External',0,'Math'],['PCons',['External',1,'Math'],['PCons',['External',2,'Fact:Add'],['PNil']]]]]
    r=put(root);rc=nodes[r]['eclass']
    smooth=['SmoothComb',['MoreParents',root,['NoParents']],['Rule','S'],
            ['RCons',['ParentPort',0,1,'Math'],['RCons',['ParentPort',0,0,'Math'],['RCons',['ParentPort',0,5,'Fact:Add'],['RNil']]]]]
    sr=put(smooth);sc=nodes[sr]['eclass']
    def var(x):return {'var':x}
    def call(a,b,span):return {'op':'Add','args':[var(a),var(b)],'span':span}
    rule={'body':[{'eq':[var('root'),call('a','b','lhs')]}],'head':[{'union':[var('root'),call('b','a','rhs')]}]}
    inputs=[{'variable':'a'},{'variable':'b'},{'read_span':'lhs','op':'Add'}]
    outputs=['variable:a','variable:b','rhs:Add:0','rhs:Add:1','rhs:Add:2','produced-row:rhs']
    occurrences=[{'event':i,'template':sc if i==2 else rc,'parents':[0] if i==2 else [],'inputs':inputs,'outputs':outputs} for i in range(3)]
    return Lower({'native_egraph':{'nodes':nodes}},{'occurrences':occurrences},{'S':rule})

class LowerTests(unittest.TestCase):
    def test_two_swaps_preserve_first_swap_effect(self):
        e=fixture();r=e.lower(2,'swap_twice')
        self.assertEqual(r['status'],'lowered')
        self.assertEqual([x['rule'] for x in r['steps']],['S','S'])
        self.assertEqual(len(r['head']),2)
        self.assertIn('(union v2 v3)',r['egg'])
        self.assertIn('(= v2 (Add v0 v1))',r['egg'])
    def test_shared_template_is_not_shared_occurrence(self):
        e=fixture();e.reset();e.apply(0);e.apply(1)
        self.assertEqual(len(e.head),4)
        self.assertEqual(len(e.memo),2)
        self.assertNotEqual(e.memo[0][0],e.memo[1][0])
    def test_post_effect_equality_does_not_rewrite_entry(self):
        e=fixture();r=e.lower(0,'swap')
        self.assertEqual(r['body'],['(= v2 (Add v0 v1))'])
        self.assertNotIn('(union v2 v2)',r['egg'])
    def test_intermediate_join_is_not_assumed_equal(self):
        e=fixture();e.reset();a=e.fresh(True);e.bound.add(a);b=e.fresh()
        with self.assertRaises(Unsupported):e.constrain(a,b)
    def test_entry_equality_becomes_guard(self):
        e=fixture();e.reset();a=e.fresh(True);b=e.fresh(True);e.bound.update([a,b]);e.constrain(a,b)
        self.assertEqual(e.body,['(= v0 v1)'])
if __name__=='__main__':unittest.main()
