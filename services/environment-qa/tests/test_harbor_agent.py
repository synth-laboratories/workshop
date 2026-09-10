import asyncio
import importlib.util
import os
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import AsyncMock, MagicMock, patch


@unittest.skipUnless(importlib.util.find_spec('harbor'),'Requires the Harbor adapter runtime')
class HarborAgentTests(unittest.TestCase):
    def test_shortcut_prompt_requests_the_candidate_not_the_original_task(self):
        from environment_qa.harbor_agent import QaAgent
        with tempfile.TemporaryDirectory() as temp:
            run = {'status':'running', 'gates':[{'id':'shortcut-candidate','attempt':'token','plan_gate':'candidate-plan'}],
                   'evidence':[{'gate':'candidate-plan','result':{'plan':{'cheat':'Write a constant-nine counterexample.'}}}],
                   'policy':{'pipeline':{'agent_steps':1}}}
            store = MagicMock(); store.get.return_value = run
            with patch.dict(os.environ, {'QA_STORE':temp,'QA_RUN_ID':'run','QA_GATE_ID':'shortcut-candidate','QA_TRIAL_MODE':'cheat'}), patch('environment_qa.harbor_agent.Store',return_value=store), patch('environment_qa.harbor_agent.request_json',return_value={'command':'','done':True,'rationale':'fixture'}) as request:
                asyncio.run(QaAgent(logs_dir=Path(temp)/'logs').run('Build a correct square program.',None,None))
            user_message = request.call_args.args[3][-1]['content']
            self.assertIn('constant-nine', user_message)
            self.assertIn('background', user_message)
            self.assertIn('Create the candidate', user_message)
            self.assertIn('even if', user_message)
            self.assertIn('Leave the candidate', user_message)

    def test_worker_releases_session_on_completion_and_cancellation(self):
        from environment_qa.harbor_agent import QaAgent
        for error in (None, asyncio.CancelledError()):
            with self.subTest(cancelled=error is not None), tempfile.TemporaryDirectory() as temp:
                store = MagicMock()
                store.get.return_value = {"gates": [{"id": "experiment-1", "attempt": "token"}]}
                with patch.dict(os.environ, {"QA_STORE": temp, "QA_RUN_ID": "run", "QA_GATE_ID": "experiment-1"}), patch('environment_qa.harbor_agent.Store', return_value=store), patch.object(QaAgent, '_run', new=AsyncMock(side_effect=error)), patch('environment_qa.dispatch.SESSIONS.release') as release, patch('environment_qa.dispatch.publish') as publish:
                    agent = QaAgent(logs_dir=Path(temp)/'logs')
                    if error is None:
                        asyncio.run(agent.run('fixture', None, None))
                    else:
                        with self.assertRaises(asyncio.CancelledError):
                            asyncio.run(agent.run('fixture', None, None))
                    release.assert_called_once_with('run', 'experiment-1', 'token')
                    publish.assert_called_once_with(store, 'run', release.return_value)

    def test_measured_limits_are_labeled_instrumentation(self):
        from environment_qa.harbor_agent import QaAgent
        with tempfile.TemporaryDirectory() as temp:
            run={'status':'running','bundle':{'sha256':'fixture'},'gates':[{'id':'experiment-1','executor':'targeted_trial','slot':0,'attempt':'token'}],
                 'evidence':[{'gate':'probe-plan','result':{'plan':{'cheat':''},'experiments':[{'hypothesis':'fixture'}]}}], 'policy':{'pipeline':{'agent_steps':1,'collect_runtime_limits':True}}}
            store=MagicMock();store.get.return_value=run
            environment=SimpleNamespace(upload_dir=AsyncMock(),exec=AsyncMock(return_value=SimpleNamespace(return_code=0,stdout='/sys/fs/cgroup/memory.max=4096\n/sys/fs/cgroup/memory.swap.max=2048\n',stderr='')))
            with patch.dict(os.environ,{'QA_STORE':temp,'QA_RUN_ID':'run','QA_GATE_ID':'experiment-1','QA_TRIAL_MODE':'cheat'}),patch('environment_qa.harbor_agent.Store',return_value=store),patch('environment_qa.bundles.verified_path',return_value=Path(temp)),patch('environment_qa.harbor_agent.request_json',return_value={'command':'','done':True,'rationale':'none'}) as request:
                asyncio.run(QaAgent(logs_dir=Path(temp)/'logs').run('Fixture',environment,None))
            self.assertIn('swap_limit_bytes',request.call_args.args[3][0]['content'])
            self.assertIn('read_only_cgroup_limits',(Path(temp)/'logs/qa-trajectory.json').read_text())
            environment.exec.assert_awaited_once()

    def test_timed_out_build_is_not_restarted_with_changed_shell(self):
        from environment_qa.harbor_agent import QaAgent
        with tempfile.TemporaryDirectory() as temp:
            run={'status':'running','bundle':{'sha256':'fixture'},'gates':[{'id':'experiment-1','executor':'targeted_trial','slot':0,'attempt':'token'}],
                 'evidence':[{'gate':'probe-plan','result':{'plan':{'cheat':''},'experiments':[{'hypothesis':'fixture'}]}}], 'policy':{'pipeline':{'agent_steps':3}}}
            store=MagicMock();store.get.return_value=run
            environment=SimpleNamespace(upload_dir=AsyncMock(),exec=AsyncMock(side_effect=[
                SimpleNamespace(return_code=124,stdout='Building wheel for slow-lib (pyproject.toml): started',stderr=''),
                SimpleNamespace(return_code=0,stdout='QA SOURCE-ONLY RECOVERY',stderr=''),
                SimpleNamespace(return_code=0,stdout='source archive inspected',stderr='')]))
            actions=[{'command':'python -m pip install --target /tmp/a slow-lib','done':False,'rationale':'install'},
                     {'command':'find /tmp/a; python -m pip install --target /tmp/b slow-lib','done':False,'rationale':'retry'},
                     {'command':'tar -tf /tmp/slow-lib.tar.gz','done':False,'rationale':'inspect source'}]
            with patch.dict(os.environ,{'QA_STORE':temp,'QA_RUN_ID':'run','QA_GATE_ID':'experiment-1','QA_TRIAL_MODE':'cheat'}),patch('environment_qa.harbor_agent.Store',return_value=store),patch('environment_qa.bundles.verified_path',return_value=Path(temp)),patch('environment_qa.harbor_agent.request_json',side_effect=actions):
                asyncio.run(QaAgent(logs_dir=Path(temp)/'logs').run('Fixture',environment,None))
            self.assertEqual(environment.exec.await_count,3)
            self.assertIn('Repeated source build', (Path(temp)/'logs/qa-trajectory.json').read_text())

    def test_inspector_observes_nested_failure_before_finish(self):
        from environment_qa.harbor_agent import QaAgent
        with tempfile.TemporaryDirectory() as temp:
            run={'status':'running','bundle':{'sha256':'fixture'},'gates':[{'id':'experiment-1','executor':'targeted_trial','slot':0,'attempt':'token'}],
                 'evidence':[{'gate':'probe-plan','result':{'plan':{'cheat':''},'experiments':[{'hypothesis':'fixture'}]}}], 'policy':{'pipeline':{'agent_steps':2}}}
            store=MagicMock();store.get.return_value=run
            environment=SimpleNamespace(upload_dir=AsyncMock(),exec=AsyncMock(return_value=SimpleNamespace(return_code=0,stdout='ModuleNotFoundError: nested check failed',stderr='')))
            actions=[{'command':'run checks','done':True,'rationale':'intent'},{'command':'','done':True,'rationale':'not checked'}]
            with patch.dict(os.environ,{'QA_STORE':temp,'QA_RUN_ID':'run','QA_GATE_ID':'experiment-1','QA_TRIAL_MODE':'cheat'}),patch('environment_qa.harbor_agent.Store',return_value=store),patch('environment_qa.bundles.verified_path',return_value=Path(temp)),patch('environment_qa.harbor_agent.request_json',side_effect=actions) as request:
                asyncio.run(QaAgent(logs_dir=Path(temp)/'logs').run('Fixture',environment,None))
            self.assertEqual(request.call_count,2)
            environment.exec.assert_awaited_once()
            self.assertIn('dependencies are missing',request.call_args.args[3][-1]['content'])
    def test_targeted_inspector_gets_labeled_sources(self):
        from environment_qa.harbor_agent import QaAgent
        with tempfile.TemporaryDirectory() as temp:
            run={'status':'running','bundle':{'sha256':'fixture'},
                 'gates':[{'id':'experiment-1','executor':'targeted_trial','slot':0,'attempt':'token'}],
                 'evidence':[{'gate':'probe-plan','result':{'plan':{'cheat':''},'experiments':[{'hypothesis':'fixture'}]}}],
                 'policy':{'pipeline':{'agent_steps':1}}}
            store=MagicMock();store.get.return_value=run
            environment=SimpleNamespace(upload_dir=AsyncMock(),exec=AsyncMock())
            with patch.dict(os.environ,{'QA_STORE':temp,'QA_RUN_ID':'run','QA_GATE_ID':'experiment-1','QA_TRIAL_MODE':'cheat'}), patch('environment_qa.harbor_agent.Store',return_value=store), patch('environment_qa.bundles.verified_path',return_value=Path(temp)), patch('environment_qa.harbor_agent.request_json',return_value={'command':'','done':True,'rationale':'done'}) as request:
                asyncio.run(QaAgent(logs_dir=Path(temp)/'logs').run('Fixture',environment,None))
            environment.upload_dir.assert_awaited_once_with(Path(temp),'/qa-review-sources')
            self.assertIn('NOT evidence',request.call_args.args[3][0]['content'])
            self.assertIn('reviewer_source_injection',(Path(temp)/'logs/qa-trajectory.json').read_text())

    def test_final_command_executes_before_completion(self):
        from environment_qa.harbor_agent import QaAgent
        with tempfile.TemporaryDirectory() as temp:
            run = {'status':'running','gates':[{'id':'frontier','attempt':'token'}],
                   'evidence':[{'gate':'probe-plan','result':{'plan':{'frontier':'unused'}}}],
                   'policy':{'pipeline':{'agent_steps':2}}}
            store = MagicMock(); store.get.return_value = run
            environment = SimpleNamespace(exec=AsyncMock(return_value=SimpleNamespace(return_code=0,stdout='ok',stderr='')))
            with patch.dict(os.environ,{'QA_STORE':temp,'QA_RUN_ID':'run','QA_GATE_ID':'frontier','QA_TRIAL_MODE':'frontier'}), patch('environment_qa.harbor_agent.Store',return_value=store), patch('environment_qa.harbor_agent.request_json',return_value={'command':'echo ok','done':True,'rationale':'test'}):
                agent = QaAgent(logs_dir=Path(temp)/'logs')
                asyncio.run(agent.run('Fixture',environment,None))
            environment.exec.assert_awaited_once_with(command='echo ok',timeout_sec=60)

    def test_pause_before_dispatch_makes_no_provider_call(self):
        from environment_qa.harbor_agent import QaAgent
        with tempfile.TemporaryDirectory() as temp:
            run = {'status':'paused','gates':[{'id':'frontier','attempt':'token'}],
                   'evidence':[{'gate':'probe-plan','result':{'plan':{'frontier':'unused'}}}],
                   'policy':{'pipeline':{'agent_steps':2}}}
            store = MagicMock(); store.get.return_value = run
            with patch.dict(os.environ,{'QA_STORE':temp,'QA_RUN_ID':'run','QA_GATE_ID':'frontier','QA_TRIAL_MODE':'frontier'}), patch('environment_qa.harbor_agent.Store',return_value=store), patch('environment_qa.harbor_agent.request_json') as request:
                asyncio.run(QaAgent(logs_dir=Path(temp)/'logs').run('Fixture',None,None))
            request.assert_not_called()
