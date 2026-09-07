import unittest
from environment_qa.executors import ledger_projection

class LedgerProjectionTests(unittest.TestCase):
    def test_duplicate_source_becomes_exact_reference_not_lost_claim(self):
        text='assert value # '+ 'long source statement '*8
        original={'id':'x','path':'test.py','evidence':text,'causal_claim':'Specific condition causes rejection','supporting_evidence':[{'path':'instruction.md','evidence':text}]}
        result=ledger_projection([original],{'test.py':'# header\n'+text+'\n','instruction.md':text})[0]
        self.assertEqual(result['causal_claim'],original['causal_claim'])
        self.assertEqual(result['evidence_ref']['line'],2)
        self.assertEqual(result['supporting_evidence'][0]['evidence_ref']['characters'],len(text))
        self.assertEqual(original['evidence'],text)
    def test_unavailable_source_quote_is_retained(self):
        original={'path':'prior-runtime','evidence':'error'}
        self.assertEqual(ledger_projection([original],{}),[original])
