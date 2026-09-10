import unittest
from environment_qa.assertion_inventory import assertions

class AssertionInventoryTests(unittest.TestCase):
    def test_all_verifier_assertions_get_stable_source_targets(self):
        files={'tests/test_result.py':'def test_output():\n    assert output.exists()\n    assert len(items) == 3, "shape"\n','solution/main.py':'assert secret\n'}
        result=assertions(files)
        self.assertEqual(len(result['assertions']),2)
        self.assertEqual(result['assertions']['assertion-002']['line'],3)
        self.assertEqual(result['assertions']['assertion-001']['evidence'],'assert output.exists()')
        self.assertEqual(assertions(files,1)['omitted'],1)
