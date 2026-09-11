import unittest
from deep_combs import History

def history(parents, names=None):
    names = names or {}
    name = lambda i: names.get(i, f"R{i}")
    witnesses, classes = [], {}
    for consumer, producers in parents.items():
        roles = {}
        entries, support = [], []
        for slot, producer in enumerate(producers):
            role = roles.setdefault(producer, f"p{len(roles)}")
            entries.append(f"{name(producer)}@{role}:T#{slot}{{head/0/expr/1=>body/0/expr/1}}")
            support.append({"producer": producer, "table": "T", "slot": slot, "producer_sites": ["head/0/expr/1"],
                            "consumer_sites": ["body/0/expr/1"], "kind": "row", "union_dependencies": []})
        motif = "[" + " + ".join(entries) + "] -> " + name(consumer)
        witnesses.append({"scope": 0, "consumer": consumer, "motif": motif, "support": support, "boundary_read_count": 0})
        classes[motif] = {"shape": motif, "normalized": {"sort": "BindingMap", "op": "binding-map", "args": []},
                          "producer_port_labels": [], "consumer_port_labels": [], "boundary_consumer_ports": []}
    return History({"witnesses": witnesses}, {"classes": list(classes.values())})

class Windows(unittest.TestCase):
    def test_diamond_retains_one_shared_ancestor(self):
        h = history({1: [0], 2: [0], 3: [1, 2]})
        w = h.window((0, 3), 2)
        self.assertEqual(w["stats"]["nodes"], 4)
        self.assertEqual(w["stats"]["edges"], 4)
        self.assertEqual(w["stats"]["shared_ancestors"], 1)
        self.assertEqual(sum(n["match"] == 0 for n in w["source_nodes"]), 1)

    def test_windows_overlap_without_ownership_restriction(self):
        h = history({1: [0], 2: [0], 3: [1, 2]})
        a = {n["match"] for n in h.window((0, 1), 1)["source_nodes"]}
        b = {n["match"] for n in h.window((0, 2), 1)["source_nodes"]}
        self.assertEqual(a & b, {0})

    def test_shorter_path_expands_shared_node(self):
        h = history({1: [0], 2: [1], 3: [1], 4: [3], 5: [4, 2]})
        w = h.window((0, 5), 3)
        distances = {n["match"]: n["distance"] for n in w["source_nodes"]}
        self.assertEqual(distances[1], 2)
        self.assertEqual(distances[0], 3)

    def test_same_rule_different_events_are_not_collapsed(self):
        h = history({1: [0], 2: [0], 3: [1, 2]}, {1: "A", 2: "A"})
        w = h.window((0, 3), 2)
        self.assertEqual(sum(n["rule"] == "A" for n in w["source_nodes"]), 2)

    def test_event_cycle_rejected(self):
        with self.assertRaisesRegex(AssertionError, "event dependency cycle"):
            history({1: [2], 2: [1]})

if __name__ == "__main__":
    unittest.main()
