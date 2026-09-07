import json,tempfile,unittest,hashlib
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch
from environment_qa.contract_analysis import analyze

class ContractAnalysisTests(unittest.TestCase):
    def test_raw_command_stream_restores_records_missing_from_trajectory_preview(self):
        contract=dict(package='records',source_path='consumer.py',evidence='consume()',contract='selected record is usable')
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);(root/'agent').mkdir()
            trajectory=json.dumps([{'step':0,'action':{'command':'measure all records'},'observation':{'stdout':'first\n[omitted]\nlast','return_code':0}}]).encode()
            stream=b'first\nselected record has incompatible value\nlast'
            (root/'agent/qa-trajectory.json').write_bytes(trajectory)
            (root/'agent/command-0.stdout.txt').write_bytes(stream)
            manifest=[{'path':p,'sha256':hashlib.sha256(b).hexdigest()} for p,b in [('agent/qa-trajectory.json',trajectory),('agent/command-0.stdout.txt',stream)]]
            run={'id':'r','findings':[],'evidence':[{'gate':'dependency-contracts','result':{'contracts':[contract]}},{'gate':'experiment-1','result':{'artifacts':manifest,'observations':{'agent/qa-trajectory.json':'{}'}}}]}
            def respond(*args,**kwargs):
                self.assertIn('selected record has incompatible value',json.dumps(args[3]))
                return {'coverage':{'0':{'status':'violated','reason':'selected value incompatible','evidence_ids':['o1']}},'limitations':[]}
            with patch('environment_qa.executors.inputs',return_value={'consumer.py':'consume()'}),patch('environment_qa.contract_analysis.request_json',side_effect=respond):
                result=analyze(SimpleNamespace(root=root),run,{'id':'contract-analysis','attempt':'one'},None)
                self.assertEqual(result['findings'][0]['evidence'],'selected record has incompatible value')
                (root/'agent/command-0.stdout.txt').write_text('tampered')
                with self.assertRaisesRegex(ValueError,'digest mismatch'):
                    analyze(SimpleNamespace(root=root),run,{'id':'contract-analysis','attempt':'two'},None)
    def test_line_id_materializes_exact_observation_without_requoting(self):
        contract=dict(package='adapter',source_path='consumer.py',evidence='required_key',contract='required_key present',consumer_requirement='required_key')
        run={'id':'r','findings':[],'evidence':[{'gate':'dependency-contracts','result':{'contracts':[contract]}},{'gate':'experiment-1','result':{'observations':{'qa-trajectory.json':json.dumps({'events':[{'step':0,'action':{'command':'measure'},'observation':{'stdout':'  keys = ["new_key"]  ','return_code':0}}]})}}}]}
        response={'coverage':{'0':{'status':'violated','reason':'required key absent','evidence_ids':['o0']}},'limitations':[]}
        with tempfile.TemporaryDirectory() as temp,patch('environment_qa.executors.inputs',return_value={'consumer.py':'required_key'}),patch('environment_qa.contract_analysis.request_json',return_value=response):
            result=analyze(SimpleNamespace(root=Path(temp)),run,{'id':'contract-analysis','attempt':'one'},None)
        self.assertEqual(result['findings'][0]['evidence'],'  keys = ["new_key"]  ')
        self.assertEqual(result['coverage']['0']['citations'][0]['line'],1)

    def test_verified_original_replaces_lossy_preview(self):
        contract=dict(package='adapter',source_path='consumer.py',evidence='required_key',contract='required_key present',consumer_requirement='required_key')
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);raw=json.dumps([{'step':0,'action':{'command':'inspect'},'observation':{'stdout':'required_key absent','return_code':0}}]).encode()
            (root/'qa-trajectory.json').write_bytes(raw)
            run={'id':'r','findings':[],'evidence':[{'gate':'dependency-contracts','result':{'contracts':[contract]}},{'gate':'experiment-1','result':{'artifacts':[{'path':'qa-trajectory.json','sha256':hashlib.sha256(raw).hexdigest()}],'observations':{'qa-trajectory.json':json.dumps({'events':[]})}}}]}
            response={'coverage':{'0':{'status':'violated','reason':'required key missing','observation_path':'observations/experiment-1-0.stdout.txt','evidence':'required_key absent'}},'limitations':[]}
            with patch('environment_qa.executors.inputs',return_value={'consumer.py':'required_key'}),patch('environment_qa.contract_analysis.request_json',return_value=response):
                self.assertEqual(len(analyze(SimpleNamespace(root=root),run,{'id':'contract-analysis','attempt':'one'},None)['findings']),1)
                (root/'qa-trajectory.json').write_text('tampered')
                with self.assertRaisesRegex(ValueError,'digest mismatch'):
                    analyze(SimpleNamespace(root=root),run,{'id':'contract-analysis','attempt':'two'},None)

    def test_observations_not_agent_success_claims_are_reviewed(self):
        contract=dict(package='adapter',source_path='consumer.py',evidence="x['old_key']",relevance_path='instruction.md',relevance_evidence='output',contract='old_key must be present',consumer_requirement='consumer indexes old_key')
        run={'id':'r','findings':[],'evidence':[{'gate':'dependency-contracts','result':{'contracts':[contract]}},{'gate':'experiment-1','result':{'observations':{'qa-trajectory.json':json.dumps({'events':[{'step':0,'action':{'command':'inspect keys','rationale':'SUCCESS, EVERYTHING PASSED'},'observation':{'return_code':0,'stdout':"keys=['new_key']",'stderr':''}}]})}}}]}
        result={'coverage':{'0':{'status':'violated','reason':'new_key does not satisfy old_key','observation_path':'observations/experiment-1-0.stdout.txt','evidence':"keys=['new_key']"}},'limitations':[]}
        with tempfile.TemporaryDirectory() as temp,patch('environment_qa.executors.inputs',return_value={'consumer.py':"x['old_key']",'instruction.md':'output'}),patch('environment_qa.contract_analysis.request_json',return_value=result) as request:
            output=analyze(SimpleNamespace(root=Path(temp)),run,{'id':'contract-analysis','attempt':'one'},None)
        self.assertEqual(len(output['findings']),1)
        self.assertNotIn('EVERYTHING PASSED',json.dumps(request.call_args.args[3]))
        self.assertIn("keys=['new_key']",json.dumps(request.call_args.args[3]))
    def test_empty_inventory_does_not_call_provider(self):
        with patch('environment_qa.contract_analysis.request_json') as request:
            self.assertEqual(analyze(None,{'evidence':[]},None,None)['findings'],[])
        request.assert_not_called()

    def test_satisfied_requires_supported_observation_too(self):
        contract=dict(package='builder',source_path='build.py',evidence='hook()',contract='hook executes')
        run={'id':'r','findings':[],'evidence':[{'gate':'dependency-contracts','result':{'contracts':[contract]}}]}
        response={'coverage':{'0':{'status':'satisfied','reason':'wheel exists','observation_path':'','evidence':''}},'limitations':[]}
        with tempfile.TemporaryDirectory() as temp,patch('environment_qa.executors.inputs',return_value={'build.py':'hook()'}),patch('environment_qa.contract_analysis.request_json',return_value=response):
            output=analyze(SimpleNamespace(root=Path(temp)),run,{'id':'contract-analysis','attempt':'one'},None)
        self.assertEqual(output['coverage']['0']['status'],'not_checked')
        self.assertEqual(output['findings'],[])
        self.assertTrue(output['limitations'])
