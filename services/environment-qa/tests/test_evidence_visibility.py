import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
from environment_qa.core import Store
from environment_qa.runtime import trial
from environment_qa.executors import review_schema, review
from environment_qa.inference import validate_shape


class EvidenceVisibilityTests(unittest.TestCase):
    def test_failure_tail_is_in_review_packet_and_full_log_is_hashed(self):
        with tempfile.TemporaryDirectory() as temp:
            store = Store(Path(temp)/'store')
            run = {'id':'a'*32,'policy':{'pipeline':{'trial_timeout_seconds':10}}}
            gate = {'id':'repeat-verifier','mode':'oracle-repeat'}
            job = store.root/'trials'/run['id']/('qa-'+run['id'][:16]+'-repeat-verifier')/'task__fixture'/'verifier'
            job.mkdir(parents=True)
            log = job/'test-stdout.txt'
            log.write_text('dependency noise\n'*2000+'AssertionError: unexpected output artifact\n')
            with patch('environment_qa.runtime.probe',return_value={'findings':[],'limitations':[]}), patch('environment_qa.runtime.reconcile_cleanup',return_value=[{'clean':True}]):
                result = trial(store,run,gate,Path(temp))
            observed = result['observations'][str(log.relative_to(store.root))]
            self.assertIn('AssertionError: unexpected output artifact',observed)
            self.assertLessEqual(len(observed.encode()),8000)
            self.assertGreater(result['artifacts'][0]['bytes'],8000)

    def test_adjudication_cannot_emit_replacement_findings(self):
        raw = {'findings':[{'category':'x','severity':'warning','title':'x','path':'x','evidence':'x','mechanism':'x'}],
               'limitations':[],'dispositions':[]}
        with self.assertRaises(ValueError): validate_shape(raw,review_schema('attribution'))

    def test_adjudication_must_account_for_active_ledger(self):
        with tempfile.TemporaryDirectory() as temp:
            store = Store(Path(temp)/'store')
            run = {'id':'run','policy':{'charter':{},'task_goals':''},'findings':[{'id':'existing','disposition':'proposed'}]}
            gate = {'id':'attribution','attempt':'attempt','role':'attribution'}
            with patch('environment_qa.executors.inputs',return_value={}), patch('environment_qa.executors.request_json',return_value={'findings':[],'limitations':[],'dispositions':[]}):
                with self.assertRaisesRegex(ValueError,'each active finding'):
                    review(store,run,gate,Path(temp))

    def test_adjudication_uses_required_keys_and_bounded_duplicate_targets(self):
        with tempfile.TemporaryDirectory() as temp:
            store=Store(Path(temp)/'store')
            run={'id':'run','policy':{'charter':{},'task_goals':''},'findings':[{'id':'existing','disposition':'dismissed'}]}
            gate={'id':'critic','attempt':'attempt','role':'critic'}
            def respond(*args,**kwargs):
                schema=kwargs['response_schema']['properties']['disposition_by_id']
                self.assertEqual(schema['required'],['existing'])
                self.assertEqual(schema['properties']['existing']['properties']['duplicate_of']['type'],'string')
                return {'findings':[],'limitations':[],'assessment':{'verdict':'inconclusive','rationale':'conditional','unresolved':[]},
                        'disposition_by_id':{'existing':{'status':'unresolved','reason':'No counterevidence','duplicate_of':''}}}
            with patch('environment_qa.executors.inputs',return_value={}),patch('environment_qa.executors.request_json',side_effect=respond):
                result=review(store,run,gate,Path(temp))
            self.assertEqual(result['dispositions'][0]['finding_id'],'existing')
