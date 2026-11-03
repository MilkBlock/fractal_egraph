import unittest
from features import Graph
from context import footprints


def v(n,s='E'):return ('Eq',s,str(n))


class ContextTests(unittest.TestCase):
    def test_cycle_converges_and_keeps_all_alternatives(self):
        ops={'A':(['E'],'E'),'B':(['E'],'E'),'Leaf':([],'E')}
        g=Graph([('A',(v(2),),v(1)),('B',(v(1),),v(2)),('Leaf',(),v(2))],ops)
        _,r,work=footprints(g)
        self.assertIn(('head','Leaf'),r[v(1)]);self.assertIn(('head','A'),r[v(2)])
        self.assertLess(work['successful_unions'],20)

    def test_cross_sort_scope_backlinks_are_not_followed(self):
        ops={'EWrap':(['S'],'E'),'Scope':(['E'],'S'),'Leaf':([],'E')}
        g=Graph([('Leaf',(),v(1)),('Scope',(v(1),),v(2,'S')),('EWrap',(v(2,'S'),),v(3))],ops)
        _,r,_=footprints(g)
        self.assertNotIn(('head','Leaf'),r[v(3)])

    def test_child_metadata_enum_is_local_input_not_analysis_label(self):
        ops={'Add':([],'Op'),'Bop':(['Op','E'],'E'),'Leaf':([],'E')}
        rows=[('Add',(),v(1,'Op')),('Leaf',(),v(2)),('Bop',(v(1,'Op'),v(2)),v(3))]
        local,r,_=footprints(Graph(rows,ops))
        self.assertIn(('enum-argument','Add'),local[v(3)])
        self.assertNotIn(('head','Leaf'),local[v(3)])
        self.assertIn(('head','Leaf'),r[v(3)])


if __name__=='__main__':unittest.main()
