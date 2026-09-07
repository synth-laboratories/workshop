import ast
import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from environment_qa import audit as audit_module
from environment_qa import dispatch
from environment_qa.bundles import export_bundle
from environment_qa.codex_executor import ProtocolError, fake_launcher
from environment_qa.core import Store
from environment_qa.dag import claim
from environment_qa.executors import object_schema
from environment_qa.policy import full_policy

RATES = {"QA_INPUT_USD_PER_MILLION": "0.2", "QA_OUTPUT_USD_PER_MILLION": "1.2"}
ANSWER = {"answer": "ok"}


def submit(arguments, item_id="i1"):
    return {"method": "item/completed",
            "params": {"item": {"id": item_id, "type": "function_call", "name": "submit_result",
                                "arguments": json.dumps(arguments)}}}


def completed(usage=None):
    turn = {"id": "turn-1", "status": "completed"}
    if usage:
        turn["usage"] = usage
    return {"method": "turn/completed", "params": {"turn": turn}}


class DispatchTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        root = Path(self.temp.name)
        task = root / "task"
        task.mkdir()
        (task / "instruction.md").write_text("Fixture")
        (task / "task.toml").write_text('version="1.0"')
        self.store = Store(root / "store")
        self.run = self.store.create(export_bundle(task, self.store.root, [task]),
                                     reviewer="ai", budget_usd=1, pipeline=full_policy())
        self.gate, self.token = claim(self.store, self.run["id"])
        self.schema = object_schema({"answer": {"type": "string"}})
        self.env = patch.dict(os.environ, RATES)
        self.env.start()

    def tearDown(self):
        dispatch.SESSIONS.release_run(self.run["id"])
        self.env.stop()
        self.temp.cleanup()

    def call(self, *events, **kwargs):
        return dispatch.request_json(self.store, self.run["id"], self.gate["id"], [{"role": "user", "content": "hi"}],
                                     attempt_token=self.token, response_schema=self.schema,
                                     launcher=fake_launcher({"events": list(events)}), **kwargs)

    def budget_call(self):
        return next(iter(self.store.get(self.run["id"])["budget"]["calls"].values()))

    # --- no fallback ---

    def test_without_a_configured_app_server_a_gate_stops_rather_than_falling_back(self):
        with patch.dict(os.environ, {"QA_CODEX_APP_SERVER": ""}):
            with self.assertRaises(dispatch.NoAppServer) as caught:
                dispatch.configured_launcher()
        self.assertIn("no direct-provider fallback", str(caught.exception))

    def test_a_dispatch_never_touches_the_direct_transports(self):
        tripped = []
        with patch("environment_qa.inference.request_json", side_effect=lambda *a, **k: tripped.append("inference")), \
             patch("environment_qa.review.call_ai", side_effect=lambda *a, **k: tripped.append("review")):
            result = self.call(submit(ANSWER), completed())
        self.assertEqual(result, ANSWER)
        self.assertEqual(tripped, [], "a direct provider transport was reached")

    def test_rates_must_be_configured_before_anything_can_spend(self):
        with patch.dict(os.environ, {"QA_INPUT_USD_PER_MILLION": ""}):
            with self.assertRaisesRegex(ValueError, "cost bound"):
                dispatch.provider_rates()

    def test_invalid_accounting_does_not_start_a_process(self):
        with patch.dict(os.environ, {"QA_INPUT_USD_PER_MILLION": "nan"}), \
             patch.object(dispatch.SESSIONS, "acquire") as acquire:
            with self.assertRaises(ValueError):
                self.call(submit(ANSWER), completed())
            acquire.assert_not_called()

    def test_stale_attempt_does_not_start_a_process(self):
        with patch.object(dispatch.SESSIONS, "acquire") as acquire:
            with self.assertRaises(Exception):
                dispatch.request_json(self.store, self.run["id"], self.gate["id"], [],
                                      attempt_token="stale", response_schema=self.schema)
            acquire.assert_not_called()

    # --- accounting parity with the direct path ---

    def test_a_successful_turn_is_admitted_and_settled(self):
        self.call(submit(ANSWER), completed({"input_tokens": 100, "output_tokens": 20}))
        call = self.budget_call()
        self.assertEqual(call["gate_id"], self.gate["id"])
        self.assertGreater(call["reserved_usd"], 0)
        self.assertAlmostEqual(call["actual_usd"], (100 * 0.2 + 20 * 1.2) / 1_000_000)

    def test_an_unreported_usage_keeps_the_conservative_reservation(self):
        self.call(submit(ANSWER), completed())
        call = self.budget_call()
        self.assertIsNone(call["actual_usd"])
        self.assertGreater(call["reserved_usd"], 0)

    def test_a_failed_turn_keeps_its_reservation_and_records_why(self):
        with self.assertRaises(ProtocolError):
            self.call({"method": "turn/completed", "params": {"turn": {"id": "turn-1", "status": "failed"}}})
        call = self.budget_call()
        self.assertIn("provider_error", call)
        self.assertIsNone(call["actual_usd"])
        self.assertGreater(call["reserved_usd"], 0)

    def test_a_stale_attempt_spends_nothing(self):
        with self.assertRaises(Exception):
            dispatch.request_json(self.store, self.run["id"], self.gate["id"], [],
                                  attempt_token="not-the-attempt", response_schema=self.schema,
                                  launcher=fake_launcher({"events": [submit(ANSWER), completed()]}))
        self.assertEqual(self.store.get(self.run["id"])["budget"]["calls"], {})

    # --- session isolation and reuse ---

    def test_turns_in_one_gate_reuse_one_process(self):
        launcher = fake_launcher({"events": [submit(ANSWER), completed()]})
        first = dispatch.SESSIONS.acquire(self.store, self.store.get(self.run["id"]), self.gate["id"], self.token,
                                          launcher=launcher)
        second = dispatch.SESSIONS.acquire(self.store, self.store.get(self.run["id"]), self.gate["id"], self.token,
                                           launcher=launcher)
        self.assertIs(first, second)
        self.assertEqual(first.process.pid, second.process.pid)

    def test_a_new_attempt_does_not_inherit_the_previous_session(self):
        launcher = fake_launcher({"events": []})
        run = self.store.get(self.run["id"])
        first = dispatch.SESSIONS.acquire(self.store, run, self.gate["id"], self.token, launcher=launcher)
        second = dispatch.SESSIONS.acquire(self.store, run, self.gate["id"], "attempt-2", launcher=launcher)
        self.assertIsNot(first, second)
        self.assertNotEqual(first.process.pid, second.process.pid)
        dispatch.SESSIONS.release(self.run["id"], self.gate["id"], "attempt-2")

    def test_releasing_a_gate_stops_its_process(self):
        launcher = fake_launcher({"events": []})
        executor = dispatch.SESSIONS.acquire(self.store, self.store.get(self.run["id"]), self.gate["id"], self.token,
                                             launcher=launcher)
        owned = executor.process
        dispatch.SESSIONS.release(self.run["id"], self.gate["id"], self.token)
        self.assertIsNotNone(owned.poll())

    # --- the journal reaches the follower's cursor ---

    def test_startup_and_cleanup_are_persisted_without_a_turn(self):
        executor = dispatch.SESSIONS.acquire(self.store, self.store.get(self.run["id"]), self.gate["id"], self.token,
                                             launcher=fake_launcher({"events": []}))
        kinds = [row["kind"] for row in self.store.events(self.run["id"])]
        self.assertIn("codex.thread.opened", kinds)
        self.assertIn("codex.process.started", kinds)
        executor.record("tool.result", {"tool": "read_evidence", "ok": True})
        self.assertIn("codex.tool.result", [r["kind"] for r in self.store.events(self.run["id"])])
        dispatch.SESSIONS.release(self.run["id"], self.gate["id"], self.token)
        self.assertIn("codex.process.stopped", [r["kind"] for r in self.store.events(self.run["id"])])

    def test_usd_settlement_uses_this_turn_not_thread_total(self):
        from unittest.mock import Mock
        executor = Mock(usage={"input_tokens": 900, "output_tokens": 90},
                        turn_usage={"input_tokens": 100, "output_tokens": 10}, journal=[], published_upto=0)
        executor.run_turn.return_value = ANSWER
        with patch.object(dispatch.SESSIONS, "acquire", return_value=executor):
            self.call(publish_events=False)
        self.assertAlmostEqual(self.budget_call()["actual_usd"], (100 * .2 + 10 * 1.2) / 1_000_000)

    def test_dynamic_container_tool_reaches_only_the_bound_runner(self):
        from environment_qa.harbor_bridge import HarborBridge
        calls = []
        bridge = HarborBridge(exec_=lambda argv, timeout: calls.append((argv, timeout)) or {"stdout":"fixture"})
        tool = {"__approval__": {"method":"item/tool/call", "params": {
            "tool":"container_exec", "callId":"call-1", "arguments":{"command":["echo", "fixture"], "timeout_seconds":1}}}}
        self.assertEqual(self.call(tool, submit(ANSWER), completed(), bridge=bridge), ANSWER)
        self.assertEqual(calls, [(["echo", "fixture"], 1)])
        executor = dispatch.SESSIONS.acquire(self.store, self.store.get(self.run["id"]), self.gate["id"], self.token)
        self.assertIn("container_exec", executor.spec["tools"])
        self.assertTrue(all(t["type"] == "function" and "inputSchema" in t for t in executor.spec["dynamic_tools"]))

    def test_turn_events_are_published_onto_the_run_event_log(self):
        self.call(submit(ANSWER), completed())
        banked = [e for e in self.store.events(self.run["id"]) if e["kind"].startswith("codex.")]
        self.assertTrue(banked, "no gate transcript reached the run event log")
        self.assertTrue(any(e["payload"]["kind"] == "turn/completed" for e in banked))
        self.assertEqual({e["payload"]["gate_id"] for e in banked}, {self.gate["id"]})
        self.assertNotIn("codex_events", self.store.get(self.run["id"]),
                         "transcripts must stay on the log, not bloat the document")


class TokenAccountingTests(unittest.TestCase):
    """A subscription-metered runtime reports tokens, so tokens are the ceiling."""

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        root = Path(self.temp.name)
        task = root / "task"
        task.mkdir()
        (task / "instruction.md").write_text("Fixture")
        (task / "task.toml").write_text('version="1.0"')
        self.store = Store(root / "store")
        self.run = self.store.create(export_bundle(task, self.store.root, [task]),
                                     reviewer="ai", budget_usd=1, pipeline=full_policy())
        self.gate, self.token = claim(self.store, self.run["id"])
        self.schema = object_schema({"answer": {"type": "string"}})
        # No USD rates: this is the subscription case.
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
                                     attempt_token=self.token, response_schema=self.schema,
                                     launcher=fake_launcher({"events": list(events)}))

    def ledger(self):
        return self.store.get(self.run["id"])["budget"]["tokens"]

    @staticmethod
    def usage_event(last_in, last_out, total_in, total_out):
        return {"method": "thread/tokenUsage/updated",
                "params": {"threadId": "t", "turnId": "u", "tokenUsage": {
                    "total": {"totalTokens": total_in + total_out, "inputTokens": total_in,
                              "outputTokens": total_out},
                    "last": {"totalTokens": last_in + last_out, "inputTokens": last_in,
                             "outputTokens": last_out}}}}

    def test_neither_ceiling_declared_refuses_to_spend(self):
        with patch.dict(os.environ, {"QA_TOKEN_BUDGET": ""}):
            with self.assertRaisesRegex(ValueError, "No accounting policy"):
                dispatch.accounting_policy()

    def test_a_token_ceiling_is_used_when_no_price_exists(self):
        unit, rates, ceiling = dispatch.accounting_policy()
        self.assertEqual((unit, rates, ceiling), ("tokens", None, 100000))

    def test_a_turn_is_admitted_and_settled_in_tokens(self):
        self.call(self.usage_event(100, 20, 100, 20), submit(ANSWER), completed())
        ledger = self.ledger()
        self.assertEqual(ledger["limit"], 100000)
        entry = next(iter(ledger["calls"].values()))
        self.assertGreater(entry["reserved"], 0)
        self.assertEqual(entry["actual"], 120)
        self.assertEqual(ledger["actual"], 120)

    def test_settlement_includes_all_requests_inside_the_first_turn(self):
        # `last` is one request, not the full agent turn. With no prior turn,
        # all 500 tokens belong to this turn, including tool round trips.
        self.call(self.usage_event(100, 20, 400, 100), submit(ANSWER), completed())
        self.assertEqual(next(iter(self.ledger()["calls"].values()))["actual"], 500)

    def test_a_failed_turn_keeps_its_token_reservation(self):
        with self.assertRaises(ProtocolError):
            self.call({"method": "turn/completed", "params": {"turn": {"id": "turn-1", "status": "failed"}}})
        entry = next(iter(self.ledger()["calls"].values()))
        self.assertIn("error", entry)
        self.assertIsNone(entry["actual"])
        self.assertGreater(entry["reserved"], 0)

    def test_an_exhausted_token_budget_refuses_the_next_call(self):
        with patch.dict(os.environ, {"QA_TOKEN_BUDGET": "100"}):
            with self.assertRaisesRegex(ValueError, "Token budget exhausted"):
                self.call(submit(ANSWER), completed())
        # A refused admission rolls back entirely: no reservation, and no ledger
        # left behind implying one was ever taken.
        self.assertEqual(self.store.get(self.run["id"])["budget"].get("tokens", {}).get("calls", {}), {})

    def test_a_ceiling_cannot_be_raised_mid_run(self):
        self.call(self.usage_event(10, 5, 10, 5), submit(ANSWER), completed())
        with patch.dict(os.environ, {"QA_TOKEN_BUDGET": "999999"}):
            with self.assertRaisesRegex(ValueError, "never raised"):
                self.call(submit(ANSWER), completed())

    def test_the_reservation_covers_the_runtime_context_not_just_the_prompt(self):
        # A live turn measured ~14.5k tokens against a 275-token prompt, so a
        # prompt-only estimate under-reserves by ~50x and the ceiling stops binding.
        prompt_only = dispatch.accounting.estimate_tokens("hi" * 10, 256, overhead=0)
        realistic = dispatch.accounting.estimate_tokens("hi" * 10, 256)
        self.assertLess(prompt_only, 1000)
        self.assertGreater(realistic, 15000)

    def test_an_under_reservation_is_recorded_as_an_overrun(self):
        with patch.dict(os.environ, {"QA_TOKEN_CONTEXT_OVERHEAD": "0"}):
            self.call(self.usage_event(9000, 100, 9000, 100), submit(ANSWER), completed())
        entry = next(iter(self.ledger()["calls"].values()))
        self.assertEqual(entry["actual"], 9100)
        self.assertGreater(entry["overrun"], 0)
        self.assertEqual(entry["overrun"], 9100 - entry["reserved"])

    def test_a_reservation_that_held_records_no_overrun(self):
        self.call(self.usage_event(100, 20, 100, 20), submit(ANSWER), completed())
        self.assertNotIn("overrun", next(iter(self.ledger()["calls"].values())))

    def test_a_stale_attempt_reserves_no_tokens(self):
        with self.assertRaises(Exception):
            dispatch.request_json(self.store, self.run["id"], self.gate["id"], [],
                                  attempt_token="wrong", response_schema=self.schema,
                                  launcher=fake_launcher({"events": [submit(ANSWER), completed()]}))
        self.assertEqual(self.store.get(self.run["id"])["budget"].get("tokens", {}).get("calls", {}), {})


class FallbackAuditTests(unittest.TestCase):
    def test_the_repository_passes_the_audit(self):
        report = audit_module.audit()
        self.assertTrue(report["passed"], json.dumps(report, indent=2))
        self.assertEqual(report["wrong_origin"], {})
        self.assertEqual(report["illegal_references"], {})

    def test_every_gate_module_takes_request_json_from_dispatch(self):
        origins = audit_module.audit()["request_json_origins"]
        for module in ("executors", "matching", "clause_matching", "contract_analysis", "harbor_agent"):
            self.assertEqual(origins.get(module), "dispatch", f"{module} does not dispatch through the executor")

    def test_the_legacy_direct_path_is_reported_not_hidden(self):
        report = audit_module.audit()
        self.assertEqual(report["direct_transport_references"], {"worker": ["review.call_ai"], "dispatch":["provider_guard.ProviderGuard"]})

    # --- the audit itself must not go stale ---

    def test_a_new_authenticated_transport_is_discovered_structurally(self):
        source = ('import urllib.request\n'
                  'def go(key):\n'
                  '    return urllib.request.urlopen("https://api", headers={"Authorization": "Bearer " + key})\n')
        tree = ast.parse(source)
        self.assertTrue(audit_module.dials_out(tree))
        self.assertTrue(audit_module.sends_credentials(tree))

    def test_serving_http_is_not_dialling_out(self):
        tree = ast.parse('from http.server import BaseHTTPRequestHandler\n'
                         'HEADERS = "Authorization"\n')
        self.assertFalse(audit_module.dials_out(tree))

    def test_unauthenticated_metadata_fetching_is_not_a_provider_transport(self):
        tree = ast.parse('import urllib.request\n'
                         'urllib.request.urlopen("https://pypi.org/simple")\n')
        self.assertTrue(audit_module.dials_out(tree))
        self.assertFalse(audit_module.sends_credentials(tree))

    def test_an_undeclared_transport_fails_the_audit(self):
        with patch.object(audit_module, "DECLARED_TRANSPORTS", {"inference"}):
            report = audit_module.audit()
        self.assertFalse(report["passed"])
        self.assertEqual(report["unexpected_transports"], ["provider_guard", "review"])

    def test_a_gate_module_importing_the_direct_transport_fails_the_audit(self):
        with patch.object(audit_module, "PERMITTED_REFERENCES", set()):
            report = audit_module.audit()
        self.assertFalse(report["passed"])
        self.assertIn("worker", report["illegal_references"])


if __name__ == "__main__":
    unittest.main()
