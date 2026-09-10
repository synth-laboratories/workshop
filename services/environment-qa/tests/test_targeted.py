import unittest
import tempfile
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch
from environment_qa.policy import targeted_policy, full_policy
from environment_qa.executors import execute_gate


class TargetedTests(unittest.TestCase):
    def test_explicit_line_spans_resolve_quotes_without_model_retyping(self):
        source='configure()\nrun()\n'
        item=dict(package='api',source_path='source.py',evidence='badly copied',source_span={'start':1,'end':2},
                  relevance_path='instruction.md',relevance_evidence='also badly copied',relevance_span={'start':1,'end':1},
                  consumer_path='source.py',consumer_evidence='wrong',consumer_span={'start':2,'end':2},contract='configuration precedes run',probe='tiny invocation')
        with patch('environment_qa.executors.inputs',return_value={'source.py':source,'instruction.md':'produce output'}),patch('environment_qa.executors.request_json',return_value={'contracts':[item],'limitations':[]}):
            result=execute_gate(None,{'id':'r','evidence':[],'findings':[]},{'id':'contracts','attempt':'one','executor':'contract_plan'},None)
        self.assertEqual(result['contracts'][0]['evidence'],'configure()\nrun()')
        self.assertEqual(result['contracts'][0]['consumer_evidence'],'run()')
        self.assertEqual(result['quote_normalizations'][0]['rule'],'explicit_source_line_span')
    def test_rejected_contract_cannot_leave_candidate_marked_covered(self):
        source='a="https://example.org/a"\nb="https://example.org/b"'
        item=dict(package='api',source_path='solution/solve.sh',evidence='https://example.org/a',relevance_path='instruction.md',relevance_evidence='produce output',contract='usable response',probe='inspect',candidate_ids=['0'])
        raw={'contracts':[item,dict(item,evidence='invented',candidate_ids=['1'])],
             'assumption_coverage':{'0':{'status':'covered','reason':'probe A'},'1':{'status':'covered','reason':'probe B'}},'limitations':[]}
        with patch('environment_qa.executors.inputs',return_value={'solution/solve.sh':source,'instruction.md':'produce output'}),patch('environment_qa.executors.request_json',return_value=raw):
            result=execute_gate(None,{'id':'r','evidence':[],'findings':[]},{'id':'contracts','attempt':'one','executor':'contract_plan'},None)
        self.assertEqual(result['assumption_coverage']['0']['status'],'covered')
        self.assertEqual(result['assumption_coverage']['1']['status'],'deferred')
        self.assertEqual(len(result['contracts']),1)
    def test_wrapped_instruction_quote_does_not_discard_required_contract(self):
        instruction=' * Returned values must match the supplied\n   reference data exactly.'
        item=dict(package='pkg',source_path='api.py',evidence="value['key']",relevance_path='instruction.md',
                  relevance_evidence='* Returned values must match the supplied reference data exactly.',contract='key exists',probe='inspect result')
        run={'id':'r','evidence':[],'findings':[]}
        with patch('environment_qa.executors.inputs',return_value={'api.py':"value['key']",'instruction.md':instruction}),patch('environment_qa.executors.request_json',return_value={'contracts':[item],'limitations':[]}):
            result=execute_gate(None,run,{'id':'contracts','attempt':'one','executor':'contract_plan'},None)
        self.assertEqual(len(result['contracts']),1)
        self.assertIn(result['contracts'][0]['relevance_evidence'],instruction)
        self.assertEqual(result['quote_normalizations'][0]['proposed_quote'],item['relevance_evidence'])

    def test_causal_chain_retained_and_support_quotes_checked(self):
        from environment_qa.executors import review
        item=dict(category='environment',severity='warning',title='API mismatch',path='api.py',evidence="value['key']",mechanism='api_contract',causal_claim='Missing key rejects output',failure_condition='key absent',affected_behavior='output',supporting_evidence=[{'path':'instruction.md','evidence':'produce output'}])
        run={'id':'r','evidence':[],'findings':[],'policy':{'charter':{},'task_goals':''}}
        with tempfile.TemporaryDirectory() as directory, patch('environment_qa.executors.inputs',return_value={'api.py':"value['key']",'instruction.md':'produce output'}), patch('environment_qa.executors.request_json',return_value={'findings':[item,dict(item,supporting_evidence=[{'path':'instruction.md','evidence':'invented'}])],'limitations':[]}):
            result=review(SimpleNamespace(root=Path(directory)),run,{'id':'review','role':'environment','attempt':'one'},None)
        self.assertEqual(len(result['findings']),1)
        self.assertEqual(result['findings'][0]['causal_claim'],item['causal_claim'])
        self.assertEqual(len(result['rejected_findings']),1)

    def test_contract_quotes_and_independent_inputs(self):
        item=dict(package='pkg',source_path='api.py',evidence="value['key']",relevance_path='instruction.md',relevance_evidence='produce output',contract='key exists',probe='inspect result')
        run={'id':'r','evidence':[{'gate':'dependency-inventory'},{'gate':'peer'}],'findings':[{'id':'peer'}]}
        with patch('environment_qa.executors.inputs',return_value={'api.py':"value['key']",'instruction.md':'produce output'}) as inputs, patch('environment_qa.executors.request_json',return_value={'contracts':[item,dict(item,evidence='invented')],'limitations':[]}):
            result=execute_gate(None,run,{'id':'contracts','attempt':'one','executor':'contract_plan'},None)
        self.assertEqual(result['contracts'],[item])
        self.assertEqual(len(result['limitations']),1)
        self.assertEqual(inputs.call_args.args[0]['evidence'],[{'gate':'dependency-inventory'}])
        self.assertEqual(inputs.call_args.args[0]['findings'],[])

    def test_contract_smoke_preserves_two_probe_ceiling(self):
        run={'id':'r','evidence':[{'gate':'dependency-contracts','result':{'contracts':[{'contract':'keys'}]}}]}
        with tempfile.TemporaryDirectory() as directory, patch('environment_qa.executors.inputs',return_value={}), patch('environment_qa.executors.request_json',return_value={'experiments':[{'hypothesis':'a'},{'hypothesis':'b'}],'deferred':[]}):
            result=execute_gate(SimpleNamespace(root=Path(directory)),run,{'id':'plan','attempt':'one','executor':'targeted_plan'},None)
        self.assertEqual(len(result['experiments']),2)
        self.assertEqual(result['experiments'][0]['execution_kind'],'component')
        self.assertEqual(result['experiments'][1]['hypothesis'],'a')
        self.assertTrue(result['limitations'])

    def test_specialists_are_independent(self):
        from environment_qa.executors import review
        run={'id':'run','evidence':[{'gate':'peer'}],'findings':[{'id':'peer'}],
             'policy':{'charter':{},'task_goals':''}}
        with tempfile.TemporaryDirectory() as directory:
            for role in ('specification','verifier','environment','boundaries'):
                with patch('environment_qa.executors.inputs',return_value={}) as inputs, patch('environment_qa.executors.request_json',return_value={'findings':[],'limitations':[]}):
                    review(SimpleNamespace(root=Path(directory)),run,{'id':role,'role':role,'attempt':'one'},None)
                    self.assertEqual(inputs.call_args.args[0]['evidence'],[])
                    self.assertEqual(inputs.call_args.args[0]['findings'],[])
        self.assertEqual(len(run['evidence']),1)

    def test_no_blanket_solves(self):
        p=targeted_policy()
        self.assertEqual(sum(n['executor']=='targeted_trial' for n in p['nodes']),2)
        self.assertFalse(any(n['executor'] in {'trial','agent_trial'} for n in p['nodes']))
        self.assertEqual(p['trial_timeout_seconds'],120)
        self.assertEqual(len(full_policy()['nodes']),26)
        self.assertIn('probe-approval',next(n for n in p['nodes'] if n['id']=='experiment-1')['depends_on'])

    def test_unselected_slot_does_not_launch(self):
        run={'evidence':[{'gate':'probe-plan','result':{'experiments':[]}}]}
        with patch('environment_qa.runtime.trial') as trial:
            result=execute_gate(None,run,{'executor':'targeted_trial','slot':0},None)
            trial.assert_not_called()
        self.assertEqual(result['execution'],'not_selected')

    def test_selected_slot_passes_hypothesis(self):
        experiment={'hypothesis':'x','objective':'y','confirmation':'z'}
        run={'evidence':[{'gate':'probe-plan','result':{'experiments':[experiment]}}]}
        with patch('environment_qa.runtime.trial',return_value={'findings':[]}) as trial:
            result=execute_gate(None,run,{'executor':'targeted_trial','slot':0},None)
            self.assertEqual(trial.call_args.args[2]['mode'],'cheat')
        self.assertEqual(result['experiment'],experiment)
