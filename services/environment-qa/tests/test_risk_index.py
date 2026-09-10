import unittest
from environment_qa.risk_index import risk_index

class RiskIndexTests(unittest.TestCase):
    def test_generic_environment_candidates(self):
        rows=risk_index({'solution/solve.sh':'ulimit -c unlimited\nx=malloc(1000);\nspawn telnet localhost 1', 'tests/test.py':'os.rename("x", "/tmp/x")'})
        self.assertEqual({r['kind'] for r in rows},{'hard_resource_limit','allocation_budget','session_lifecycle','filesystem_boundary'})
        self.assertTrue(all('question' in r and 'source' in r for r in rows))

    def test_never_indexes_previous_review_evidence(self):
        self.assertEqual(risk_index({'evidence/peer.json':'os.rename("a","b")'}),[])
