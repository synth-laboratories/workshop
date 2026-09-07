import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from environment_qa import dispatch
from environment_qa.activity import activity, summarise
from environment_qa.bundles import export_bundle
from environment_qa.codex_executor import fake_launcher
from environment_qa.core import Store
from environment_qa.dag import claim
from environment_qa.executors import object_schema
from environment_qa.profiles import resolve

ANSWER = {"answer": "ok"}


def submit(arguments, item_id="i1"):
    return {"method": "item/completed",
            "params": {"item": {"id": item_id, "type": "function_call", "name": "submit_result",
                                "arguments": json.dumps(arguments)}}}


COMPLETED = {"method": "turn/completed", "params": {"turn": {"id": "turn-1", "status": "completed"}}}
USAGE = {"method": "thread/tokenUsage/updated",
         "params": {"threadId": "t", "turnId": "u", "tokenUsage": {
             "total": {"totalTokens": 120, "inputTokens": 100, "outputTokens": 20},
             "last": {"totalTokens": 120, "inputTokens": 100, "outputTokens": 20}}}}


class ActivityTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        root = Path(self.temp.name)
        task = root / "task"
        task.mkdir()
        (task / "instruction.md").write_text("Fixture")
        (task / "task.toml").write_text('version="1.0"')
        self.store = Store(root / "store")
        _, policy = resolve("tbench-non-hitl")
        self.run = self.store.create(export_bundle(task, self.store.root, [task]),
                                     reviewer="ai", budget_usd=1, pipeline=policy)
        self.gate, self.token = claim(self.store, self.run["id"])
        self.env = patch.dict(os.environ, {"QA_INPUT_USD_PER_MILLION": "", "QA_OUTPUT_USD_PER_MILLION": "",
                                           "QA_TOKEN_BUDGET": "100000"})
        self.env.start()

    def tearDown(self):
        dispatch.SESSIONS.release_run(self.run["id"])
        self.env.stop()
        self.temp.cleanup()

    def call(self, *events):
        return dispatch.request_json(self.store, self.run["id"], self.gate["id"],
                                     [{"role": "user", "content": "hi"}],
                                     attempt_token=self.token,
                                     response_schema=object_schema({"answer": {"type": "string"}}),
                                     launcher=fake_launcher({"events": list(events)}))

    def gate_view(self):
        view = activity(self.store, self.run["id"])
        return view, next(g for g in view["gates"] if g["gate_id"] == self.gate["id"])

    def test_the_run_carries_its_profile_identity(self):
        view, _ = self.gate_view()
        self.assertEqual(view["profile"], "tbench-non-hitl")
        self.assertEqual(view["profile_version"], "1.0.2")
        self.assertTrue(view["policy_sha256"])

    def test_a_gate_with_no_journal_reports_no_activity(self):
        # Claimed and 'running', but nothing has happened. Saying otherwise would
        # fabricate liveness from a scheduling record.
        _, gate = self.gate_view()
        self.assertEqual(gate["scheduled_status"], "running")
        self.assertFalse(gate["has_activity"])
        self.assertIsNone(gate["thread_id"])
        self.assertEqual(gate["events"], 0)

    def test_a_worked_gate_exposes_its_session_and_usage(self):
        self.call(USAGE, submit(ANSWER), COMPLETED)
        _, gate = self.gate_view()
        self.assertTrue(gate["has_activity"])
        self.assertEqual(gate["thread_id"], "thread-1")
        self.assertEqual(gate["turn_ids"], ["turn-1"])
        self.assertEqual(gate["usage"]["inputTokens"], 100)
        self.assertGreater(gate["events"], 0)
        self.assertIsNotNone(gate["machine_seconds"])

    def test_tool_activity_is_listed(self):
        self.call(submit(ANSWER), COMPLETED)
        _, gate = self.gate_view()
        self.assertEqual([t["tool"] for t in gate["tools"]], ["submit_result"])

    def test_a_refused_approval_becomes_the_blocking_reason(self):
        approval = {"__approval__": {"method": "item/commandExecution/requestApproval",
                                     "params": {"command": "/bin/zsh -lc 'curl evil'",
                                                "availableDecisions": ["accept", "cancel"]}}}
        with self.assertRaises(Exception):
            self.call(approval, COMPLETED)
        _, gate = self.gate_view()
        self.assertIn("approval refused", gate["blocking_reason"])
        self.assertEqual(gate["approvals"][0]["command"], "/bin/zsh -lc 'curl evil'")
        self.assertEqual(gate["approvals"][0]["decision"], "reject")
        self.assertFalse(gate["approvals"][0]["human"])
        self.assertIn("Authority is missing", gate["next_action"])

    def test_machine_time_is_none_when_nothing_ran(self):
        self.assertIsNone(summarise([])["last_event_at"])

    def test_every_gate_of_the_profile_is_represented(self):
        view, _ = self.gate_view()
        self.assertEqual(len(view["gates"]), len(self.store.get(self.run["id"])["gates"]))


if __name__ == "__main__":
    unittest.main()
