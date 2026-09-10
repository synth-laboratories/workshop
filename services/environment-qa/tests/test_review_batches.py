import unittest
import json
import tempfile
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch
from environment_qa.review_batches import combine

class BatchTests(unittest.TestCase):
    def test_large_ledger_is_exhaustively_dispatched_with_unique_contexts(self):
        from environment_qa.executors import review
        findings=[{'id':str(i),'title':'claim','causal_claim':'x'*80000,'disposition':'proposed','path':'source.py','evidence':'source'} for i in range(4)]
        run={'id':'r','policy':{'charter':'quality','task_goals':[]},'findings':findings,'evidence':[]}
        gate={'id':'critic','role':'critic','attempt':'a'}
        dispatched=[]
        def inputs(scoped,path):return {'source.py':'source','evidence/findings.json':json.dumps(scoped['findings'])}
        def request(*args,**kwargs):
            ids=kwargs['response_schema']['properties']['disposition_by_id']['required'];dispatched.extend(ids)
            return {'findings':[],'limitations':[],'assessment':{'verdict':'inconclusive','rationale':'unmeasured','unresolved':ids},
                    'disposition_by_id':{id:{'status':'unresolved','reason':'unmeasured','duplicate_of':''} for id in ids}}
        with tempfile.TemporaryDirectory() as temp,patch('environment_qa.executors.inputs',side_effect=inputs),patch('environment_qa.executors.request_json',side_effect=request) as dispatch:
            result=review(SimpleNamespace(root=Path(temp)),run,gate,None)
            self.assertEqual(dispatch.call_count,2)
            self.assertEqual(set(dispatched),{'0','1','2','3'})
            self.assertEqual(len(dispatched),4)
            self.assertEqual(len(list((Path(temp)/'contexts/r').glob('*.json'))),3)
            self.assertEqual(result['assessment']['verdict'],'inconclusive')
            self.assertEqual(len(result['dispositions']),4)
    def test_exhaustive_union_is_conservative(self):
        rows=[{'dispositions':[{'finding_id':'a','status':'unresolved'}],
               'assessment':{'verdict':'pass','rationale':'A checked','unresolved':[]},'context_ref':{'path':'a'}},
              {'dispositions':[{'finding_id':'b','status':'confirmed'}],
               'assessment':{'verdict':'inconclusive','rationale':'B needs evidence','unresolved':['b']},'context_ref':{'path':'b'}}]
        result=combine(rows,'critic',{'path':'all'},'digest',240001)
        self.assertEqual([d['finding_id'] for d in result['dispositions']],['a','b'])
        self.assertEqual(result['assessment']['verdict'],'inconclusive')
        self.assertEqual(result['assessment']['unresolved'],['b'])
        self.assertTrue(result['batch_review']['all_batches_completed'])
        rows[1]['assessment']['verdict']='fail'
        self.assertEqual(combine(rows,'critic',{},'d',1)['assessment']['verdict'],'fail')

    def test_duplicate_batch_members_are_rejected(self):
        with self.assertRaisesRegex(ValueError,'Duplicate finding'):
            combine([{'dispositions':[{'finding_id':'a'}]},{'dispositions':[{'finding_id':'a'}]}],'critic',{},'d',1)
