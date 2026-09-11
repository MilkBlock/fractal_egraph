import copy
import unittest
from deep_combs import term, decode_graph
from test_deep_combs import history
from relative_bindings import convert, restore, site, unsite, compose_address, relative_suffix
from bridge_plans import plan, Reservation

class RelativeAndPlanning(unittest.TestCase):
    def test_relative_addresses_ignore_ids_and_keep_shared_ancestor(self):
        p = history({1:[0],2:[0],3:[1,2]}).window((0,3),2)["program"]
        mapping = {0:20,1:30,2:10,3:50}
        def rename(p):
            return term(p["sort"], mapping[int(p["op"])] if p["sort"]=="EventRef" else p["op"], [rename(a) for a in p["args"]])
        q = rename(p)
        a, witness = convert(p); b, other = convert(q)
        self.assertEqual(a,b)
        self.assertEqual(restore(a,witness),p)
        self.assertEqual(restore(b,other),q)
        self.assertEqual(len(witness["addresses"]),4)

    def test_rule_anchor_and_argument_route_round_trip(self):
        p = term("Position","path",[term("PositionPart",x) for x in "head/2/expr/1/args/0/args/1".split("/")])
        self.assertEqual(unsite(site(p)),p)

    def test_address_composition_and_cross_branch_reference(self):
        root = term("BindingAddress","root")
        a = term("BindingAddress","via",[root,term("ReadInterface","a")])
        b = term("BindingAddress","via",[root,term("ReadInterface","b")])
        c = term("BindingAddress","via",[root,term("ReadInterface","c")])
        self.assertEqual(compose_address(compose_address(a,b),c),compose_address(a,compose_address(b,c)))
        self.assertEqual(relative_suffix(a,compose_address(a,b)),b)
        self.assertIsNone(relative_suffix(a,b))
        typed = term("BindingAddress","via",[root,term("ReadInterface","use:A->X:T")])
        wrong = term("BindingAddress","via",[root,term("ReadInterface","use:B->C:T")])
        with self.assertRaisesRegex(AssertionError,"incompatible rule interfaces"):
            compose_address(typed,wrong)

    def test_two_dirty_steps_can_lead_to_profitable_entry(self):
        gains = {"root":0,"a":-5,"b":-8,"c":20}
        def score(x):
            return {"raw_bytes":100,"coded_bytes":100-gains[x],"library_calls":{"M":1} if x=="c" else {}}
        successors={"a":["b"],"b":["c"]}
        short,_=plan("root","a",successors,score,set(),1)
        long,_=plan("root","a",successors,score,set(),3)
        self.assertEqual(short.net_gain,-10)
        self.assertEqual(long.path,("a","b","c")); self.assertEqual(long.net_gain,12)
        self.assertEqual(long.startup_bytes,8)
        self.assertTrue(long.library_entry)

    def test_reservation_cancels_on_model_graph_or_availability_change(self):
        for available,epoch,version in [({2},9,0),({2},1,1),(set(),1,0)]:
            r=Reservation(1,0,[2]);self.assertIsNone(r.take(available,epoch,version));self.assertFalse(r.pending)

if __name__ == "__main__":
    unittest.main()
