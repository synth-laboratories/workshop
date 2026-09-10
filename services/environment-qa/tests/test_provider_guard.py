import tempfile
import unittest
from pathlib import Path

import test_dispatch as fixtures
from environment_qa.provider_guard import project_request, admit, OUTPUT_TOKENS, reconcile_completed_gates, validate_event
from environment_qa.process_pool import ProcessPool
from environment_qa.codex_executor import GateBlocked


class GuardTests(unittest.TestCase):
    setUp = fixtures.DispatchTests.setUp
    tearDown = fixtures.DispatchTests.tearDown

    def test_removes_ambient_tools_and_enforces_output_limit(self):
        result, wire, reserve = project_request({"model":"gpt-5.6-luna", "input":"hello",
            "max_output_tokens":999999, "tools":[{"type":"web_search"},
                {"type":"function","name":"exec_command"},{"type":"function","name":"read_evidence"}]}, ["read_evidence"])
        self.assertEqual(result["max_output_tokens"], OUTPUT_TOKENS)
        self.assertEqual([t["name"] for t in result["tools"]], ["read_evidence"])
        self.assertGreater(reserve, 0)
        self.assertNotIn("exec_command", wire.decode())

    def test_hidden_context_and_other_models_are_rejected(self):
        for extra in ({"model":"claude"}, {"previous_response_id":"hidden"}, {"conversation":"hidden"}):
            with self.assertRaises(ValueError):
                project_request({"model":"gpt-5.6-luna", "input":"hello"} | extra, [])

    def test_hallucinated_native_tool_is_blocked_before_delivery(self):
        with self.assertRaises(ValueError):
            validate_event({"item":{"type":"function_call","name":"exec_command"}},["read_evidence"])
        validate_event({"item":{"type":"function_call","name":"read_evidence"}},["read_evidence"])
        validate_event({"item":{"type":"custom_tool_call","name":"read_evidence"}},["read_evidence"])
        with self.assertRaises(ValueError):
            validate_event({"item":{"type":"custom_tool_call","name":"exec_command"}},["read_evidence"])
        validate_event({"item":{"type":"custom_tool_call","name":"exec"}},["read_evidence"],code_mode=True)
        validate_event({"item":{"type":"function_call","name":"wait"}},["read_evidence"],code_mode=True)
        with self.assertRaises(ValueError):
            validate_event({"item":{"type":"custom_tool_call","name":"exec_command"}},["read_evidence"],code_mode=True)

    def test_every_transport_request_consumes_shared_allowance(self):
        spec = {"run_id":self.run["id"], "gate_id":self.gate["id"], "attempt":self.token}
        admit(self.store, spec, .6, "first")
        with self.assertRaisesRegex(ValueError, "allowance exhausted"):
            admit(self.store, spec, .6, "retry")
        self.assertEqual(len(self.store.get(self.run["id"])["budget"]["transport"]["calls"]), 1)

    def test_pool_coordinates_independent_instances(self):
        with tempfile.TemporaryDirectory() as directory:
            a, b = ProcessPool(1, Path(directory)), ProcessPool(1, Path(directory))
            with a.lease("a"):
                with self.assertRaises(GateBlocked):
                    with b.lease("b", timeout=.01): pass
            with b.lease("b", timeout=.01): pass

    def test_only_completed_gate_usage_releases_unused_bounds(self):
        spec={"run_id":self.run["id"],"gate_id":self.gate["id"],"attempt":self.token}
        call_id = admit(self.store,spec,.6,"first")
        admit(self.store,spec,.2,"unknown-retry")
        self.store.append_event(self.run["id"],"transport.completed",{"call_id":call_id})
        self.store.append_event(self.run["id"],"codex.thread/tokenUsage/updated",{
            "gate_id":self.gate["id"],"attempt":self.token,"kind":"thread/tokenUsage/updated",
            "payload":{"tokenUsage":{"total":{"inputTokens":100,"outputTokens":20}}}})
        reconcile_completed_gates(self.store,self.run["id"])
        self.assertEqual(self.store.get(self.run["id"])["budget"]["transport"]["reserved_usd"],.8)
        self.store.mutate(self.run["id"],"fixture.completed",lambda r:r["gates"][0].update(status="succeeded"))
        reconcile_completed_gates(self.store,self.run["id"])
        guard=self.store.get(self.run["id"])["budget"]["transport"]
        self.assertAlmostEqual(guard["reserved_usd"],.200086)
        self.assertEqual(guard["calls"][call_id]["reserved_usd"],.6)
