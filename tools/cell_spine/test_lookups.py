"""Constructor query CSE: functionality, not injectivity or RHS assumptions."""
import unittest
from .markers import normalize_lookups
from .takeover import call,var


def eq(a,b):return dict(kind='eq',args=[a,b])

def union(a,b):return dict(kind='union',args=[a,b])


class LookupCSE(unittest.TestCase):
    signatures={n:dict(kind='constructor') for n in ('F','G')}

    def test_aliases_rewrite_every_use_and_remove_only_self_union(self):
        a,b,x,y=map(var,('a','b','x','y'))
        body=[eq(a,call('F!',[x])),eq(b,call('F!',[y])),eq(x,y)]
        head=[union(a,b),dict(kind='expr',expr=call('Summary-0!',[b])),
              dict(kind='change',change='Subsume',head='Summary-1!',args=[b]),
              dict(kind='set',head='Output',args=[b],value=b)]
        body,head=normalize_lookups(body,head,self.signatures)
        self.assertEqual(body,[eq(a,call('F!',[x]))])
        self.assertEqual(head,[dict(kind='expr',expr=call('Summary-0!',[a])),
              dict(kind='change',change='Subsume',head='Summary-1!',args=[a]),
              dict(kind='set',head='Output',args=[a],value=a)])
        self.assertEqual(normalize_lookups(body,head,self.signatures),(body,head))

    def test_child_alias_exposes_parent_duplicates_until_fixed_point(self):
        a,b,c,d,x=map(var,('a','b','c','d','x'))
        body=[eq(c,call('G!',[a])),eq(call('G!',[b]),d),
              eq(a,call('F!',[x])),eq(call('F!',[x]),b)]
        body,head=normalize_lookups(body,[union(c,d)],self.signatures)
        self.assertEqual(body,[eq(c,call('G!',[a])),eq(a,call('F!',[x]))])
        self.assertEqual(head,[])

    def test_equal_outputs_do_not_imply_equal_arguments(self):
        root,x,y=map(var,('root','x','y'))
        body=[eq(root,call('F!',[x])),eq(root,call('F!',[y]))]
        head=[union(x,y)]
        self.assertEqual(normalize_lookups(body,head,self.signatures),(body,head))

    def test_table_argument_order_and_literals_remain_distinct(self):
        x,y=var('x'),var('y')
        lit=lambda n:dict(kind='literal',sort='i64',text=str(n))
        terms=[call('F!',[x,y]),call('F!',[y,x]),call('G!',[x,y]),call('F!!',[x,y]),
               call('F!',[x,lit(1)]),call('F!',[x,lit(2)])]
        body=[eq(var(f'v{i}'),e) for i,e in enumerate(terms)]
        self.assertEqual(normalize_lookups(body,[],self.signatures),(body,[]))

    def test_unknown_primitive_and_nonconstructor_functions_not_rewritten(self):
        a,b,x=var('a'),var('b'),var('x')
        for op,sig in [('F!',{'F':dict(kind='function')}),('F!',{}),('random',self.signatures)]:
            body=[eq(a,call(op,[x])),eq(b,call(op,[x]))]
            self.assertEqual(normalize_lookups(body,[union(a,b)],sig),(body,[union(a,b)]))

    def test_keep_constructor_existence_and_constructor_materializing_union(self):
        a,b,x,y=map(var,('a','b','x','y'))
        f=call('F!',[x]);g=call('G!!',[y])
        body,head=normalize_lookups([eq(a,f),eq(b,f)],[union(g,g),union(x,y)],self.signatures)
        self.assertEqual(body,[eq(a,f)])
        self.assertEqual(head,[union(g,g),union(x,y)])

    def test_rhs_union_does_not_make_distinct_lhs_arguments_equal(self):
        a,b,x,y=map(var,('a','b','x','y'))
        body=[eq(a,call('F!',[x])),eq(b,call('F!',[y]))]
        head=[union(x,y),union(a,b)]
        self.assertEqual(normalize_lookups(body,head,self.signatures),(body,head))


if __name__=='__main__':unittest.main()
