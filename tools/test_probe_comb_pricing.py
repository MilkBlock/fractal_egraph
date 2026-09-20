import json
from pathlib import Path
import tempfile
import unittest
from probe_comb_pricing import audit


def fixture():
    step = dict(schema=0, wiring=[{'Input': 0}], internal_parents=[], requires=[], effects=[])
    templates = [dict(pattern=dict(root=1, steps=[step, step]),
                      atoms=[{'Apply': 0}, {'Apply': 1}], observed=1, matches=0),
                 dict(pattern=dict(root=2, steps=[step, step, step]),
                      atoms=[{'Use': dict(template=0, members=[0, 1])}, {'Apply': 2}],
                      observed=1, matches=0)]
    return dict(layers=dict(reuse=dict(
        templates=templates, schemas=[dict(output_roles=['var:x'])],
        uses=[dict(template=0, members=[0, 1], parts=[{'Residual': 0}, {'Residual': 1}]),
              dict(template=1, members=[4, 5, 6], parts=[{'Residual': i} for i in [4, 5, 6]])],
        roots=[{'Use': 0}, {'Use': 1}],
        residuals=[dict(event=2, binding=[{'ResidualPort': dict(event=0, output=0)}] * 2),
                   dict(event=3, binding=[{'ResidualPort': dict(event=1, output=0)}])],
        stats=dict(selected_wiring_units=30, candidate_index_units=35))))


class AuditTests(unittest.TestCase):
    def run_audit(self, doc):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'analysis.json'
            path.write_text(json.dumps(doc))
            return audit(path)

    def test_read_slots_are_not_deduplicated_ports(self):
        a = self.run_audit(fixture())['observability']
        self.assertEqual(a['interior_read_slots'], 2)
        self.assertEqual(a['cross_use_read_slots'], 3)
        self.assertEqual(a['distinct_use_interior_ports'], 1)
        self.assertAlmostEqual(a['interior_read_fraction'], 2 / 3)
        self.assertEqual(a['closed_templates_including_singletons'], [1])
        self.assertEqual(a['closed_templates_at_least_two_uses'], [])

    def test_dependency_definition_is_not_free(self):
        doc = fixture()
        doc['layers']['reuse']['roots'] = [{'Use': 1}]
        a = self.run_audit(doc)['fixed_cover_pricing']
        self.assertEqual(a['additional_dependency_templates'], 1)
        self.assertEqual(a['flat_active_dictionary'], 21)
        self.assertEqual(a['optimistic_recipe_dictionary_with_dependencies'], 24)


if __name__ == '__main__':
    unittest.main()
