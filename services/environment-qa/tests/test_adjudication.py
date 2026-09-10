import unittest
from environment_qa.adjudication import normalize_dispositions

class AdjudicationTests(unittest.TestCase):
    def test_invalid_duplicate_is_not_lost(self):
        ds,notes=normalize_dispositions([{'finding_id':'a','status':'dismissed','duplicate_of':'missing'}],[{'id':'a'}])
        self.assertEqual(ds[0]['status'],'unresolved');self.assertTrue(notes)
    def test_chain_resolves_to_live_canonical(self):
        ds,_=normalize_dispositions([{'finding_id':'a','status':'dismissed','duplicate_of':'b'},
                                    {'finding_id':'b','status':'dismissed','duplicate_of':'c'},
                                    {'finding_id':'c','status':'confirmed','duplicate_of':''}], [{'id':x} for x in 'abc'])
        self.assertEqual(ds[0]['duplicate_of'],'c')
    def test_cycle_cannot_delete_all_findings(self):
        ds,_=normalize_dispositions([{'finding_id':'a','status':'dismissed','duplicate_of':'b'},
                                    {'finding_id':'b','status':'dismissed','duplicate_of':'a'}],[{'id':x} for x in 'ab'])
        self.assertTrue(any(d['status']=='unresolved' for d in ds))
