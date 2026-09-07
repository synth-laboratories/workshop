import unittest
from environment_qa.api_assumptions import candidates

class AssumptionTests(unittest.TestCase):
    def test_stateful_library_protocol_is_not_reduced_to_importability(self):
        source='worker = api.Worker(config)\nworker.configure()\nworker._mode = 1\nworker.run()\n'
        result=candidates({'external/module.txt':source})
        self.assertEqual(result['0']['candidate_kind'],'stateful_library_protocol')
        self.assertEqual(result['0']['methods'],['configure','run'])
        self.assertIn('worker._mode = 1',result['0']['evidence'])
    def test_multiple_external_fields_are_indexed_without_claiming_bug(self):
        source="for row in adapter.records():\n    a=row['height']\n    b=row['width']\n"
        result=candidates({'external/source.txt':source,'instruction.md':"x['a']; x['b']"})
        self.assertEqual(len(result),1)
        self.assertEqual(result['0']['required_keys'],['height','width'])
        self.assertIn(result['0']['evidence'],source)
        self.assertIn('Candidate',result['0']['notice'])

    def test_task_endpoints_are_accounted_without_task_specific_rules(self):
        source='url = "https://example.org/records/{record}"\n# https://ignored.org\n'
        result=candidates({'solution/solve.sh':source})
        self.assertEqual(len(result),1)
        self.assertEqual(result['0']['candidate_kind'],'external_endpoint')
        self.assertIn(result['0']['evidence'],source)
        self.assertIn('selection/filtering',result['0']['notice'])
