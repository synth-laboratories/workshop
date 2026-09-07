import io
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import MagicMock, patch
from environment_qa.core import Store, Conflict
from environment_qa.bundles import export_bundle
from environment_qa.policy import full_policy
from environment_qa.dag import claim
from environment_qa.inference import request_json, validate_shape
from environment_qa.executors import object_schema


class InferenceTests(unittest.TestCase):
    def test_provider_credit_failure_is_actionable_and_redacted(self):
        import urllib.error
        opener=MagicMock()
        opener.open.side_effect=urllib.error.HTTPError('https://example.invalid',402,'Payment Required',{},io.BytesIO(b'{"error":{"message":"Insufficient credits for fake"}}'))
        with patch('environment_qa.inference.ai_request',return_value=('https://example.invalid','fake',{'model':'openai/gpt-5.6-luna'},0,(.2,1.2),{})),patch('urllib.request.build_opener',return_value=opener):
            with self.assertRaisesRegex(ValueError,'HTTP 402'):
                request_json(self.store,self.run['id'],self.gate['id'],[],attempt_token=self.token,response_schema=self.schema)
        call=next(iter(self.store.get(self.run['id'])['budget']['calls'].values()))
        self.assertEqual(call['provider_error']['http_status'],402)
        self.assertNotIn('fake',call['provider_error']['message'])
        self.assertIsNone(call['actual_usd'])
        opener.open.assert_called_once()

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        root = Path(self.temp.name)
        task = root/'task'; task.mkdir()
        (task/'instruction.md').write_text('Fixture')
        (task/'task.toml').write_text('version="1.0"')
        self.store = Store(root/'store')
        self.run = self.store.create(export_bundle(task,self.store.root,[task]),reviewer='ai',budget_usd=1,pipeline=full_policy())
        self.gate,self.token = claim(self.store,self.run['id'])
        self.schema = object_schema({'answer':{'type':'string'}})

    def tearDown(self): self.temp.cleanup()

    def invoke(self, responses, token=None, cost=None, reported_model=None,tool_name='submit_qa_result'):
        opener = MagicMock()
        self.last_opener=opener
        opener.open.side_effect = [io.BytesIO(json.dumps({'model':reported_model,'choices':[{'message':{'content':None,'tool_calls':[{'function':{'name':tool_name,'arguments':content}}]}}], 'usage':dict({'prompt_tokens':100,'completion_tokens':20},**({'cost':cost} if cost is not None else {}))}).encode()) for content in responses]
        with patch('environment_qa.inference.ai_request',return_value=('https://example.invalid','fake',{'model':'openai/gpt-5.6-luna'},0,(.2,1.2),{})), patch('urllib.request.build_opener',return_value=opener):
            return request_json(self.store,self.run['id'],self.gate['id'],[],attempt_token=token or self.token,response_schema=self.schema)

    def test_invalid_response_repaired_and_both_calls_accounted(self):
        self.assertEqual(self.invoke(['{}','{"answer":"ok"}']),{'answer':'ok'})
        budget = self.store.get(self.run['id'])['budget']
        self.assertEqual(len(budget['calls']),2)
        self.assertAlmostEqual(budget['actual_usd'],.000088)
        self.assertEqual(len(list((self.store.root/'responses'/self.run['id']).glob('*.json'))),2)

    def test_execution_step_has_execution_tool_not_final_report_tool(self):
        def change(run):
            next(g for g in run['gates'] if g['id']==self.gate['id'])['executor']='targeted_trial'
        self.store.mutate(self.run['id'],'test.command-mode',change)
        self.assertEqual(self.invoke(['{"answer":"ok"}'],tool_name='execute_qa_step'),{'answer':'ok'})
        body=json.loads(self.last_opener.open.call_args.args[0].data)
        self.assertEqual(body['tool_choice']['function']['name'],'execute_qa_step')
        self.assertIn('runs command immediately',body['tools'][0]['function']['description'])

    def test_repair_is_bounded(self):
        with self.assertRaises(ValueError): self.invoke(['{}','{}'])
        self.assertEqual(len(self.store.get(self.run['id'])['budget']['calls']),2)

    def test_fractional_service_allocations_do_not_accumulate_float_error(self):
        self.store.authorize_service('fractional',16)
        for _ in range(20):self.store.create(self.run['bundle'],reviewer='ai',budget_usd=.8,pipeline=full_policy(),allowance_id='fractional')
        with self.assertRaisesRegex(ValueError,'budget exhausted'):
            self.store.create(self.run['bundle'],reviewer='ai',budget_usd=.000001,pipeline=full_policy(),allowance_id='fractional')

    def test_only_settled_reported_charge_releases_reservation(self):
        self.store.mutate(self.run['id'],'test.budget',lambda r:r['budget'].update(limit_usd=.01))
        for _ in range(3):self.invoke(['{"answer":"ok"}'],cost=.001)
        budget=self.store.get(self.run['id'])['budget']
        self.assertGreater(budget['reserved_usd'],budget['limit_usd'])
        self.assertAlmostEqual(budget['committed_usd'],.003)
        self.assertAlmostEqual(budget['actual_usd'],.003)

    def test_unreported_charge_keeps_conservative_reserve(self):
        self.store.mutate(self.run['id'],'test.budget',lambda r:r['budget'].update(limit_usd=.01))
        self.invoke(['{"answer":"ok"}'])
        with self.assertRaisesRegex(ValueError,'budget exhausted'):self.invoke(['{"answer":"ok"}'])

    def test_wrong_reported_model_is_retained_not_retried(self):
        with self.assertRaisesRegex(ValueError,'different model'):self.invoke(['{"answer":"ok"}'],reported_model='different/provider')
        self.assertEqual(len(self.store.get(self.run['id'])['budget']['calls']),1)

    def test_reasoning_allowance_is_not_doubled_on_repair(self):
        self.store.mutate(self.run['id'],'test.reasoning',lambda r:r['policy']['pipeline'].update(reasoning_effort='high'))
        self.invoke(['{}','{"answer":"ok"}'])
        for call in self.last_opener.open.call_args_list:
            body=json.loads(call.args[0].data)
            self.assertEqual(body['reasoning_effort'],'high')
            self.assertEqual(body['max_completion_tokens'],8192)

    def test_stale_dispatch_spends_nothing(self):
        with self.assertRaises(Conflict): self.invoke([],token='stale')
        self.assertEqual(self.store.get(self.run['id'])['budget']['reserved_usd'],0)

    def test_schema_rejects_extra_and_wrong_types(self):
        for value in ({'answer':False},{'answer':'ok','extra':1},[]):
            with self.assertRaises(ValueError): validate_shape(value,self.schema)

    def test_runtime_projection_preserves_measurements_and_declares_omissions(self):
        from environment_qa.executors import inputs
        run = self.run
        run['evidence'] = [{'id':'original','gate':'oracle-1','result':{
            'artifacts':[{'path':'result.json','sha256':'digest'}],
            'observations':{'result.json':'redundant','qa-trajectory.json':'observed action'},
            'trials':[{'rewards':{'reward':1}}]}}]
        with patch('environment_qa.executors.read_source_files',return_value={'instruction.md':'Fixture'}):
            packet = json.loads(inputs(run,Path('.'))['evidence/oracle-1.json'])
        self.assertEqual(packet['trials'],run['evidence'][0]['result']['trials'])
        self.assertEqual(packet['observations'],{'qa-trajectory.json':{'path':'runtime/oracle-1/qa-trajectory.json'}})
        self.assertEqual(packet['evidence_projection']['omitted_preview_paths'],['result.json'])
        self.assertIn('artifacts',run['evidence'][0]['result'])
