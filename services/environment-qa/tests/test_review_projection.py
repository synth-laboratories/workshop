import json
import unittest
from unittest.mock import patch
from environment_qa.executors import inputs, ledger_projection


class ReviewProjectionTests(unittest.TestCase):
    def test_assessment_provenance_dedup_keeps_all_judgments(self):
        original={'id':'f','evidence_id':'hash','gate_id':'source','title':'claim',
                  'assessments':[{'finding_id':'f','evidence_id':'otherhash','actor':'ai','gate_id':'critic','status':'unresolved','reason':'condition untested','duplicate_of':''}]}
        projected=ledger_projection([original],{})[0]
        self.assertNotIn('evidence_id',projected)
        self.assertEqual(projected['assessments'][0],{'actor':'ai','gate_id':'critic','status':'unresolved','reason':'condition untested','duplicate_of':''})
        self.assertEqual(original['assessments'][0]['evidence_id'],'otherhash')
    def test_drop_only_redundant_specialist_coverage_not_contract_observations(self):
        run={'policy':{},'bundle':{},'findings':[{'id':'f','title':'specific defect','evidence':'source','path':'source.py','assessments':[]}],
             'evidence':[{'id':'one','gate':'verifier','result':{'role':'verifier','coverage':['repeated prose']*100,'findings':[],'limitations':['uncertainty retained']}},
                         {'id':'two','gate':'contract-analysis','result':{'coverage':{'0':{'status':'not_checked','reason':'probe failed'}}}}]}
        with patch('environment_qa.executors.read_source_files',return_value={'source.py':'source'}):
            files=inputs(run,None)
        source=json.loads(files['evidence/verifier.json'])
        self.assertNotIn('coverage',source)
        self.assertEqual(source['coverage_projection']['entries'],100)
        self.assertEqual(source['limitations'],['uncertainty retained'])
        self.assertIn('specific defect',files['evidence/findings.json'])
        self.assertEqual(json.loads(files['evidence/contract-analysis.json'])['coverage']['0']['status'],'not_checked')
        self.assertEqual(len(run['evidence'][0]['result']['coverage']),100)
