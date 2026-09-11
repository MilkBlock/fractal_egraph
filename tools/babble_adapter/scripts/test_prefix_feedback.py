import unittest
from deep_combs import term
from prefix_feedback import FrozenLibrary, encode

def dump(op, *args):
    return {"op": op, "args": list(args)}

class Scoring(unittest.TestCase):
    def model(self):
        op = encode(term("T", "pair", [term("T", "a"), term("T", "a")]))[0]
        definition = dump("Lambda", dump(op, dump("Var(0)"), dump("Var(0)")))
        return FrozenLibrary({"libraries": [{"id": 1, "definition": definition}]})

    def test_repeated_binding_is_required(self):
        m = self.model()
        different = encode(term("T", "pair", [term("T", "a"), term("T", "b")]))
        self.assertEqual(m.rewrite(different), different)
        x = term("T", "wrap", [term("T", "wrap", [term("T", "a")])])
        repeated = encode(term("T", "pair", [x, x]))
        packed = m.rewrite(repeated)
        self.assertEqual(packed[0], "Apply")
        self.assertEqual(m.expand(packed), repeated)

    def test_merely_installing_a_library_is_not_a_compression_gain(self):
        c = self.model().cost(term("T", "unmatched"))
        self.assertEqual(c["raw_bytes"], c["coded_bytes"])

if __name__ == "__main__":
    unittest.main()
