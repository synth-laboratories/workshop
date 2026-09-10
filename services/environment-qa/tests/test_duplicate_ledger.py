import unittest
from environment_qa.matching import prediction_ledger

class DuplicateLedgerTests(unittest.TestCase):
    def test_specific_facet_survives_canonicalization(self):
        canonical=dict(id='a',title='Timing sensitivity',mechanism='timing',path='test.py',evidence='assert ratio>1.2',severity='warning',disposition='unresolved')
        alias=dict(canonical,id='b',title='Library baseline changes ratio',causal_claim='A faster baseline reduces the ratio for the same implementation',disposition='dismissed',assessments=[{'duplicate_of':'a'}])
        ledger=prediction_ledger([canonical,alias])
        self.assertEqual(len(ledger),1)
        self.assertEqual(ledger[0]['id'],'a')
        self.assertEqual(ledger[0]['duplicate_allegations'][0]['title'],alias['title'])
        self.assertEqual(ledger[0]['duplicate_allegations'][0]['causal_claim'],alias['causal_claim'])
    def test_disproven_finding_is_not_resurrected(self):
        finding=dict(id='a',title='wrong',mechanism='x',path='test.py',evidence='x',severity='warning',disposition='dismissed',assessments=[{'duplicate_of':''}])
        self.assertEqual(prediction_ledger([finding]),[])
