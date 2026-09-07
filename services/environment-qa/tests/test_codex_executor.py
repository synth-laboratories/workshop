import json
import threading
import time
import unittest
from pathlib import Path

from environment_qa.codex_executor import (ApprovalLedger, CodexGateExecutor, GateBlocked, GateCancelled,
                                           ProtocolError, ServerPool, decision_word, fake_launcher,
                                           is_approval_method, redactor, terminal_method)
from environment_qa.harbor_bridge import HarborBridge, ToolRefused, tool_definitions

SCHEMA = {"type": "object", "required": ["findings", "limitations"], "additionalProperties": False,
          "properties": {"findings": {"type": "array", "items": {"type": "object"}},
                         "limitations": {"type": "array", "items": {"type": "string"}}}}

SUBMISSION = {"findings": [], "limitations": ["bounded"]}


def spec(**overrides):
    base = {"run_id": "run-1", "gate_id": "review", "attempt": "attempt-1",
            "profile": "tbench-non-hitl", "profile_version": "1", "input_sha256": "abc123",
            "model": "gpt-5.6-luna", "effort": "medium", "tools": ["read_evidence", "submit_result"],
            "limits": {"turn_seconds": 10, "shutdown_seconds": 2},
            "approval_policy": {"mode": "non_hitl", "preauthorized": [], "revision": 3}}
    return {**base, **overrides}


def tool_call(name, arguments, item_id="i-tool"):
    return {"method": "item/completed",
            "params": {"item": {"id": item_id, "type": "function_call", "name": name,
                                "arguments": json.dumps(arguments)}}}


COMPLETED = {"method": "turn/completed", "params": {"turn": {"id": "turn-1", "status": "completed"}}}


def scenario(*events, **rest):
    return {"events": list(events), **rest}


class ProtocolUnitTests(unittest.TestCase):
    def test_a_completed_turn_carrying_a_failure_is_a_failure(self):
        self.assertEqual(terminal_method("turn/completed", {"turn": {"status": "completed"}}), "turn/completed")
        self.assertEqual(terminal_method("turn/completed", {"turn": {"status": "failed"}}), "turn/failed")
        self.assertEqual(terminal_method("turn/completed", {"turn": {"error": {"code": 1}}}), "turn/failed")
        self.assertEqual(terminal_method("turn/started", {}), "turn/started")

    def test_approval_methods_include_suffixed_variants(self):
        for method in ("permissions/request", "execCommandApproval", "applyPatchApproval",
                       "shell/requestApproval", "patch/request_approval"):
            self.assertTrue(is_approval_method(method))
        self.assertFalse(is_approval_method("item/completed"))

    def test_decision_word_uses_whatever_synonym_the_server_offers(self):
        self.assertEqual(decision_word("reject", ["decline"]), "decline")
        self.assertEqual(decision_word("once", ["approve", "always"]), "approve")
        self.assertIsNone(decision_word("reject", ["approve"]))

    def test_redactor_scrubs_nested_secrets_and_ignores_short_strings(self):
        scrub = redactor("sk-super-secret-value", "", None, "tiny")
        cleaned = scrub({"error": "failed with sk-super-secret-value", "items": ["sk-super-secret-value"]})
        self.assertEqual(cleaned["error"], "failed with [REDACTED]")
        self.assertEqual(cleaned["items"], ["[REDACTED]"])
        self.assertEqual(scrub("tiny"), "tiny")


class HandshakeTests(unittest.TestCase):
    def test_handshake_records_server_identity_and_opens_a_thread(self):
        launch = fake_launcher({"initialize": {"serverInfo": {"name": "fake-codex", "version": "9.9"}},
                                "thread": {"threadId": "thread-x"}})
        with CodexGateExecutor(spec(), launch) as executor:
            self.assertEqual(executor.state, "running")
            self.assertEqual(executor.thread_id, "thread-x")
            receipt = executor.receipt()
            self.assertEqual(receipt["process"]["server"]["serverInfo"]["version"], "9.9")
            self.assertIsNotNone(receipt["process"]["pid"])
            self.assertEqual([e["kind"] for e in receipt["events"]][:3], ["gate.state", "process.started", "server.initialized"])

    def test_a_refused_handshake_fails_the_gate_and_reaps_the_process(self):
        launch = fake_launcher({"initialize": {"__error__": {"code": -32000, "message": "unauthorized"}}})
        executor = CodexGateExecutor(spec(), launch)
        with self.assertRaises(ProtocolError) as caught:
            executor.start()
        self.assertIn("unauthorized", str(caught.exception))
        self.assertIsNone(executor.process)

    def test_a_thread_without_an_id_is_a_protocol_error(self):
        launch = fake_launcher({"thread": {}})
        with self.assertRaises(ProtocolError):
            CodexGateExecutor(spec(), launch).start()

    def test_resume_uses_the_resume_method(self):
        launch = fake_launcher({"thread": {"threadId": "thread-resumed"}})
        with CodexGateExecutor(spec(resume_thread_id="thread-resumed"), launch) as executor:
            opened = next(e for e in executor.journal if e["kind"] == "thread.opened")
            self.assertEqual(opened["payload"]["method"], "thread/resume")


class TurnTests(unittest.TestCase):
    def run_turn(self, *events, gate_spec=None, bridge=None, **rest):
        launch = fake_launcher(scenario(*events, **rest))
        executor = CodexGateExecutor(gate_spec or spec(), launch)
        with executor:
            return executor, executor.run_turn("review this", SCHEMA, bridge or HarborBridge())

    def test_a_submitted_result_is_validated_and_returned(self):
        executor, result = self.run_turn(tool_call("submit_result", SUBMISSION), COMPLETED)
        self.assertEqual(result, SUBMISSION)
        self.assertEqual(executor.state, "completed")
        self.assertEqual(executor.turn_ids, ["turn-1"])
        self.assertEqual([r["tool"] for r in executor.tool_results], ["submit_result"])

    def test_every_event_is_journaled_in_order(self):
        executor, _ = self.run_turn(
            {"method": "item/started", "params": {"item": {"id": "i1", "type": "agentMessage"}}},
            {"method": "item/agentMessage/delta", "params": {"item": {"id": "i1"}, "text": "thinking"}},
            tool_call("submit_result", SUBMISSION), COMPLETED)
        kinds = [e["kind"] for e in executor.journal]
        self.assertEqual(kinds.index("item/started") < kinds.index("item/completed"), True)
        self.assertIn("turn/completed", kinds)
        self.assertEqual([e["sequence"] for e in executor.journal], list(range(1, len(executor.journal) + 1)))

    def test_a_turn_acknowledgement_is_not_completion(self):
        # The server acknowledges turn/start and then simply stops talking.
        with self.assertRaises(ProtocolError):
            self.run_turn(gate_spec=spec(limits={"turn_seconds": 1, "shutdown_seconds": 1}))

    def test_a_failed_turn_wearing_a_completed_method_fails_the_gate(self):
        launch = fake_launcher(scenario({"method": "turn/completed",
                                         "params": {"turn": {"id": "turn-1", "status": "failed"}}}))
        executor = CodexGateExecutor(spec(), launch)
        with executor, self.assertRaises(ProtocolError):
            executor.run_turn("go", SCHEMA, HarborBridge())
        self.assertEqual(executor.state, "failed")

    # --- invalid output ---

    def test_output_failing_the_schema_fails_closed_and_keeps_the_raw_response(self):
        bad = {"findings": [], "limitations": [], "extra": "not in schema"}
        launch = fake_launcher(scenario(tool_call("submit_result", bad), COMPLETED))
        executor = CodexGateExecutor(spec(), launch)
        with executor, self.assertRaises(ProtocolError) as caught:
            executor.run_turn("go", SCHEMA, HarborBridge())
        self.assertIn("schema validation", str(caught.exception))
        self.assertEqual(executor.state, "failed")
        retained = [e for e in executor.journal if e["kind"] == "item/completed"]
        self.assertIn("not in schema", json.dumps(retained))

    def test_non_json_output_fails_closed(self):
        launch = fake_launcher(scenario(
            {"method": "item/completed", "params": {"item": {"id": "i9", "type": "agentMessage",
                                                             "text": "here is my prose answer"}}},
            COMPLETED))
        executor = CodexGateExecutor(spec(), launch)
        with executor, self.assertRaises(ProtocolError) as caught:
            executor.run_turn("go", SCHEMA, HarborBridge())
        self.assertIn("not JSON", str(caught.exception))

    # --- duplicate events ---

    def test_a_redelivered_event_is_journaled_and_executed_once(self):
        executor, result = self.run_turn(
            {**tool_call("submit_result", SUBMISSION), "__repeat__": 3}, COMPLETED)
        self.assertEqual(result, SUBMISSION)
        self.assertEqual(len([e for e in executor.journal if e["kind"] == "item/completed"]), 1)
        self.assertEqual(len(executor.tool_results), 1)

    # --- crash ---

    def test_a_server_that_dies_mid_turn_fails_the_gate(self):
        launch = fake_launcher(scenario(
            {"method": "item/started", "params": {"item": {"id": "i1", "type": "agentMessage"}}},
            {"__crash__": True}))
        executor = CodexGateExecutor(spec(), launch)
        with executor, self.assertRaises(ProtocolError) as caught:
            executor.run_turn("go", SCHEMA, HarborBridge())
        self.assertIn("app_server_exited", str(caught.exception))
        self.assertEqual(executor.state, "failed")
        self.assertIn("turn.failed", [e["kind"] for e in executor.journal])

    # --- cancellation ---

    def test_an_interrupted_turn_cancels_the_gate(self):
        launch = fake_launcher(scenario({"method": "turn/interrupted",
                                         "params": {"turn": {"id": "turn-1", "status": "interrupted"}}}))
        executor = CodexGateExecutor(spec(), launch)
        with executor, self.assertRaises(GateCancelled):
            executor.run_turn("go", SCHEMA, HarborBridge())
        self.assertEqual(executor.state, "cancelled")

    def test_cancel_stops_only_the_process_it_owns(self):
        launch = fake_launcher({})
        executor = CodexGateExecutor(spec(), launch).start()
        owned = executor.process
        executor.cancel()
        self.assertEqual(executor.state, "cancelled")
        self.assertIsNotNone(owned.poll(), "the owned process should be reaped")
        self.assertIsNone(executor.process)

    def test_operator_cancellation_interrupts_a_silent_turn(self):
        cancelled = threading.Event()
        executor = CodexGateExecutor(spec(), fake_launcher(scenario()), should_cancel=cancelled.is_set)
        with executor:
            timer = threading.Timer(0.1, cancelled.set)
            timer.start()
            started = time.monotonic()
            try:
                with self.assertRaises(GateCancelled):
                    executor.run_turn("go", SCHEMA, HarborBridge())
            finally:
                timer.join()
            self.assertLess(time.monotonic() - started, 3)
            self.assertEqual(executor.state, "cancelled")
            self.assertIsNone(executor.process)

    def test_a_turn_cannot_start_once_the_gate_is_blocked(self):
        executor = CodexGateExecutor(spec(), fake_launcher({}))
        executor.state = "blocked"
        with self.assertRaises(GateBlocked):
            executor.run_turn("go", SCHEMA)


class ApprovalTests(unittest.TestCase):
    REQUEST = {"method": "permissions/request", "params": {"tool": "container_exec"}}

    def ledger_spec(self):
        return spec()

    def test_a_decision_is_bound_to_its_attempt(self):
        ledger, gate = ApprovalLedger(), self.ledger_spec()
        ledger.record(gate, self.REQUEST, "once", actor="local-human", revision=3)
        entry, why = ledger.lookup(spec(attempt="attempt-2"), self.REQUEST, 3)
        self.assertIsNone(entry)
        self.assertEqual(why, "stale_attempt")

    def test_a_decision_from_a_superseded_revision_does_not_unblock(self):
        ledger, gate = ApprovalLedger(), self.ledger_spec()
        ledger.record(gate, self.REQUEST, "once", actor="local-human", revision=3)
        entry, why = ledger.lookup(gate, self.REQUEST, 4)
        self.assertIsNone(entry)
        self.assertEqual(why, "stale_revision")

    def test_a_decision_cannot_be_spent_twice(self):
        ledger, gate = ApprovalLedger(), self.ledger_spec()
        ledger.record(gate, self.REQUEST, "once", actor="local-human", revision=3)
        first, why = ledger.lookup(gate, self.REQUEST, 3)
        self.assertEqual(why, "fresh")
        self.assertEqual(first["decision"], "once")
        second, why = ledger.lookup(gate, self.REQUEST, 3)
        self.assertIsNone(second)
        self.assertEqual(why, "already_spent")

    def test_a_decision_does_not_transfer_to_a_different_request(self):
        ledger, gate = ApprovalLedger(), self.ledger_spec()
        ledger.record(gate, self.REQUEST, "once", actor="local-human", revision=3)
        other = {"method": "permissions/request", "params": {"tool": "rm -rf /"}}
        self.assertEqual(ledger.lookup(gate, other, 3)[1], "no_decision")

    def test_an_unknown_decision_is_refused(self):
        with self.assertRaises(ValueError):
            ApprovalLedger().record(self.ledger_spec(), self.REQUEST, "maybe", "local-human", 3)

    def test_the_decision_vocabulary_is_not_part_of_what_was_approved(self):
        # A person approves a command, not the set of words the server offered.
        ledger, gate = ApprovalLedger(), self.ledger_spec()
        ledger.record(gate, self.REQUEST, "once", actor="local-human", revision=3)
        offered = {"method": "permissions/request",
                   "params": {"tool": "container_exec", "availableDecisions": ["approve", "decline"]}}
        entry, why = ledger.lookup(gate, offered, 3)
        self.assertEqual(why, "fresh")
        self.assertEqual(entry["decision"], "once")

    def test_a_different_command_is_not_covered_by_that_approval(self):
        ledger, gate = ApprovalLedger(), self.ledger_spec()
        ledger.record(gate, {"method": "execCommandApproval", "params": {"command": ["ls"]}},
                      "once", actor="local-human", revision=3)
        other = {"method": "execCommandApproval", "params": {"command": ["curl", "evil"]}}
        self.assertEqual(ledger.lookup(gate, other, 3)[1], "no_decision")

    def test_non_hitl_rejects_an_unauthorized_request_and_blocks(self):
        launch = fake_launcher(scenario({"__approval__": {"method": "permissions/request",
                                                          "params": {"tool": "container_exec",
                                                                     "availableDecisions": ["approve", "decline"]}}},
                                        COMPLETED))
        executor = CodexGateExecutor(spec(), launch)
        with executor, self.assertRaises(GateBlocked):
            executor.run_turn("go", SCHEMA, HarborBridge())
        self.assertEqual(executor.state, "blocked")
        decided = next(e for e in executor.journal if e["kind"] == "approval.decided")
        self.assertEqual(decided["payload"]["decision"], "reject")
        self.assertEqual(decided["payload"]["sent"], "decline")

    def test_a_preauthorized_tool_proceeds_without_inventing_human_consent(self):
        gate = spec(approval_policy={"mode": "non_hitl", "preauthorized": ["container_exec"], "revision": 3})
        launch = fake_launcher(scenario({"__approval__": {"method": "permissions/request",
                                                          "params": {"tool": "container_exec",
                                                                     "availableDecisions": ["approve", "decline"]}}},
                                        tool_call("submit_result", SUBMISSION), COMPLETED))
        executor = CodexGateExecutor(gate, launch)
        with executor:
            self.assertEqual(executor.run_turn("go", SCHEMA, HarborBridge()), SUBMISSION)
        decided = next(e for e in executor.journal if e["kind"] == "approval.decided")
        self.assertEqual((decided["payload"]["decision"], decided["payload"]["reason"]), ("once", "preauthorized"))
        self.assertEqual(decided["payload"]["sent"], "approve")

    def test_a_server_offering_no_rejection_word_gets_a_protocol_error(self):
        launch = fake_launcher(scenario({"__approval__": {"method": "permissions/request",
                                                          "params": {"tool": "container_exec",
                                                                     "availableDecisions": ["approve"]}}},
                                        COMPLETED))
        executor = CodexGateExecutor(spec(), launch)
        with executor, self.assertRaises(GateBlocked):
            executor.run_turn("go", SCHEMA, HarborBridge())
        decided = next(e for e in executor.journal if e["kind"] == "approval.decided")
        self.assertIsNone(decided["payload"]["sent"])

    def test_an_agent_decision_is_never_recorded_as_human_approval(self):
        # agent-cua is a permitted decision actor, and the one most likely to be
        # miscounted: the certificate counts human decisions.
        ledger, gate = ApprovalLedger(), self.ledger_spec()
        ledger.record(gate, self.REQUEST, "once", actor="agent-cua", revision=3)
        launch = fake_launcher(scenario({"__approval__": {"method": "permissions/request",
                                                          "params": {"tool": "container_exec",
                                                                     "availableDecisions": ["approve", "decline"]}}},
                                        tool_call("submit_result", SUBMISSION), COMPLETED))
        executor = CodexGateExecutor(gate, launch, approvals=ledger)
        with executor:
            executor.run_turn("go", SCHEMA, HarborBridge())
        decided = next(e for e in executor.journal if e["kind"] == "approval.decided")
        self.assertEqual(decided["payload"]["actor"], "agent-cua")
        self.assertFalse(decided["payload"]["human"])
        self.assertEqual(executor.receipt()["human_approvals"], 0)

    def test_a_human_decision_is_counted_as_one(self):
        ledger, gate = ApprovalLedger(), self.ledger_spec()
        ledger.record(gate, self.REQUEST, "once", actor="local-human", revision=3)
        launch = fake_launcher(scenario({"__approval__": {"method": "permissions/request",
                                                          "params": {"tool": "container_exec",
                                                                     "availableDecisions": ["approve", "decline"]}}},
                                        tool_call("submit_result", SUBMISSION), COMPLETED))
        executor = CodexGateExecutor(gate, launch, approvals=ledger)
        with executor:
            executor.run_turn("go", SCHEMA, HarborBridge())
        self.assertEqual(executor.receipt()["human_approvals"], 1)

    def test_a_preauthorized_decision_is_policy_not_a_person(self):
        gate = spec(approval_policy={"mode": "non_hitl", "preauthorized": ["container_exec"], "revision": 3})
        launch = fake_launcher(scenario({"__approval__": {"method": "permissions/request",
                                                          "params": {"tool": "container_exec",
                                                                     "availableDecisions": ["approve", "decline"]}}},
                                        tool_call("submit_result", SUBMISSION), COMPLETED))
        executor = CodexGateExecutor(gate, launch)
        with executor:
            executor.run_turn("go", SCHEMA, HarborBridge())
        decided = next(e for e in executor.journal if e["kind"] == "approval.decided")
        self.assertEqual(decided["payload"]["actor"], "policy")
        self.assertFalse(decided["payload"]["human"])
        self.assertEqual(executor.receipt()["human_approvals"], 0)

class ResourceAdmissionTests(unittest.TestCase):
    def test_the_pool_bounds_simultaneous_servers(self):
        pool = ServerPool(1)
        launch = fake_launcher({})
        first = CodexGateExecutor(spec(), launch, pool=pool).start()
        self.assertEqual(pool.in_use, 1)
        second = CodexGateExecutor(spec(gate_id="other"), launch, pool=pool)
        with self.assertRaises(GateBlocked):
            second.pool.lease("run-1:other:attempt-1", timeout=0.2).__enter__()
        first.close()
        self.assertEqual(pool.in_use, 0)

    def test_a_lease_is_released_when_the_gate_closes(self):
        pool = ServerPool(2)
        with CodexGateExecutor(spec(), fake_launcher({}), pool=pool):
            self.assertEqual(pool.in_use, 1)
        self.assertEqual(pool.in_use, 0)

    def test_capacity_freed_by_one_gate_admits_the_next(self):
        pool = ServerPool(1)
        holder = CodexGateExecutor(spec(), fake_launcher({}), pool=pool).start()
        admitted = threading.Event()

        def waiter():
            with pool.lease("run-1:second:attempt-1", timeout=5):
                admitted.set()

        thread = threading.Thread(target=waiter, daemon=True)
        thread.start()
        time.sleep(0.1)
        self.assertFalse(admitted.is_set())
        holder.close()
        thread.join(timeout=5)
        self.assertTrue(admitted.is_set())

    def test_an_invalid_pool_limit_is_refused(self):
        for bad in (0, -1, True, 1.5):
            with self.assertRaises(ValueError):
                ServerPool(bad)


class RedactionTests(unittest.TestCase):
    SECRET = "sk-live-abcdef0123456789"

    def test_a_secret_in_a_server_event_never_reaches_the_journal(self):
        launch = fake_launcher(scenario(
            {"method": "item/completed",
             "params": {"item": {"id": "i1", "type": "agentMessage",
                                 "text": f"provider rejected key {self.SECRET}"}}},
            tool_call("submit_result", SUBMISSION), COMPLETED))
        executor = CodexGateExecutor(spec(), launch, secrets=[self.SECRET])
        with executor:
            executor.run_turn("go", SCHEMA, HarborBridge())
        serialized = json.dumps(executor.receipt())
        self.assertNotIn(self.SECRET, serialized)
        self.assertIn("[REDACTED]", serialized)


class HarborBridgeTests(unittest.TestCase):
    def test_executed_command_cannot_be_resubmitted_for_execution(self):
        bridge = HarborBridge(exec_=lambda command, timeout: {'stdout': 'done', 'exit_code': 0})
        bridge.call('container_exec', {'command': ['echo', 'done']})
        with self.assertRaisesRegex(ToolRefused, 'empty command'):
            bridge.call('submit_result', {'command': 'echo done', 'done': True, 'rationale': 'already ran'})
        self.assertTrue(bridge.call('submit_result', {'command': '', 'done': True, 'rationale': 'already ran'})['ok'])

    class Executor:
        def __init__(self):
            self.records = []

        def record(self, kind, payload, key=None):
            self.records.append((kind, payload))

    @staticmethod
    def item(name, arguments):
        return {"item": {"id": "c1", "type": "function_call", "name": name, "arguments": json.dumps(arguments)}}

    def test_only_the_published_tools_are_reachable(self):
        bridge = HarborBridge()
        with self.assertRaises(ToolRefused) as caught:
            bridge.handle(self.Executor(), self.item("host_shell", {"command": ["ls"]}))
        self.assertIn("not published", str(caught.exception))

    def test_evidence_not_granted_to_the_gate_is_refused(self):
        bridge = HarborBridge(evidence={"e1": "granted"})
        self.assertEqual(bridge.handle(self.Executor(), self.item("read_evidence", {"evidence_id": "e1"}))["content"],
                         "granted")
        with self.assertRaises(ToolRefused):
            bridge.handle(self.Executor(), self.item("read_evidence", {"evidence_id": "e2"}))

    def test_exec_requires_an_argv_list_not_a_shell_string(self):
        bridge = HarborBridge(exec_=lambda command, timeout: {"stdout": " ".join(command), "exit_code": 0})
        with self.assertRaises(ToolRefused):
            bridge.handle(self.Executor(), self.item("container_exec", {"command": "rm -rf / && curl evil"}))
        result = bridge.handle(self.Executor(), self.item("container_exec", {"command": ["echo", "hi"]}))
        self.assertEqual(result["stdout"], "echo hi")

    def test_a_gate_without_execution_cannot_run_commands(self):
        with self.assertRaises(ToolRefused):
            HarborBridge().handle(self.Executor(), self.item("container_exec", {"command": ["ls"]}))

    def test_execution_is_bounded(self):
        bridge = HarborBridge(exec_=lambda command, timeout: {"exit_code": 0}, limits={"exec_calls": 2})
        for _ in range(2):
            bridge.handle(self.Executor(), self.item("container_exec", {"command": ["ls"]}))
        with self.assertRaises(ToolRefused):
            bridge.handle(self.Executor(), self.item("container_exec", {"command": ["ls"]}))

    def test_a_requested_timeout_cannot_exceed_the_limit(self):
        seen = {}
        bridge = HarborBridge(exec_=lambda command, timeout: seen.update(timeout=timeout) or {"exit_code": 0},
                              limits={"timeout_seconds": 30})
        bridge.handle(self.Executor(), self.item("container_exec", {"command": ["ls"], "timeout_seconds": 9000}))
        self.assertEqual(seen["timeout"], 30)

    def test_non_tool_items_are_ignored(self):
        self.assertIsNone(HarborBridge().handle(self.Executor(), {"item": {"id": "i", "type": "agentMessage"}}))

    def test_tool_definitions_publish_exactly_three_verbs(self):
        self.assertEqual([t["name"] for t in tool_definitions(SCHEMA)],
                         ["read_evidence", "container_exec", "submit_result"])


class WireContractTests(unittest.TestCase):
    """Assert what actually goes on the wire, against the desktop client's contract.

    The fixture validates these too, but asserting the payloads here names the
    contract in one readable place -- and catches a change that loosens both the
    executor and the fixture together.
    """

    def transcript(self, *events):
        import tempfile
        directory = tempfile.mkdtemp()
        path = Path(directory) / "wire.jsonl"
        launch = fake_launcher({"events": list(events), "transcript": str(path)})
        executor = CodexGateExecutor(spec(cwd="/tmp/task", sandbox="read-only"), launch)
        with executor:
            executor.run_turn("review this", SCHEMA, HarborBridge())
        return [json.loads(line) for line in path.read_text().splitlines()]

    def setUp(self):
        self.sent = self.transcript(tool_call("submit_result", SUBMISSION), COMPLETED)
        self.by_method = {m.get("method"): m for m in self.sent}

    def test_initialize_carries_a_named_and_versioned_client(self):
        info = self.by_method["initialize"]["params"]["clientInfo"]
        self.assertEqual(info["name"], "workshop-environment-qa")
        self.assertTrue(info["version"], "clientInfo.version must be sent")
        self.assertTrue(self.by_method["initialize"]["params"]["capabilities"]["experimentalApi"])

    def test_initialized_is_announced_after_the_handshake(self):
        order = [m.get("method") for m in self.sent]
        self.assertIn("initialized", order)
        self.assertLess(order.index("initialize"), order.index("initialized"))
        self.assertLess(order.index("initialized"), order.index("thread/start"))
        self.assertIsNone(self.by_method["initialized"].get("id"), "initialized is a notification")

    def test_thread_start_carries_the_confinement_controls(self):
        params = self.by_method["thread/start"]["params"]
        self.assertEqual(params["model"], "gpt-5.6-luna")
        self.assertEqual(params["cwd"], "/tmp/task")
        self.assertEqual(params["sandbox"], "read-only")
        self.assertEqual(params["approvalPolicy"], "untrusted")

    def test_turn_start_sends_typed_content_not_a_bare_string(self):
        params = self.by_method["turn/start"]["params"]
        self.assertEqual(params["threadId"], "thread-1")
        self.assertEqual(params["approvalPolicy"], "untrusted")
        self.assertIsInstance(params["input"], list)
        self.assertEqual(params["input"][0]["type"], "text")
        self.assertIn("review this", params["input"][0]["text"])
        self.assertEqual(params["effort"], "medium")


class FixtureEnforcementTests(unittest.TestCase):
    """The fixture must reject a client that regresses, or it proves nothing."""

    def raw(self, scenario=None):
        from environment_qa.codex_executor import Connection
        launch = fake_launcher(scenario or {})
        return Connection(launch(spec()))

    def test_a_thread_opened_before_initialize_is_refused(self):
        connection = self.raw()
        rpc_id = connection.request("thread/start", {"model": "m", "cwd": ".",
                                                     "approvalPolicy": "untrusted", "sandbox": "read-only"})
        reply = connection.next_message(timeout=5)
        self.assertEqual(reply["id"], rpc_id)
        self.assertIn("before initialize", reply["error"]["message"])

    def test_an_initialize_without_a_version_is_refused(self):
        connection = self.raw()
        connection.request("initialize", {"clientInfo": {"name": "x"}, "capabilities": {}})
        reply = connection.next_message(timeout=5)
        self.assertIn("clientInfo.version", reply["error"]["message"])

    def test_a_bare_string_input_is_refused(self):
        connection = self.raw()
        connection.request("initialize", {"clientInfo": {"name": "x", "version": "1"}, "capabilities": {}})
        connection.next_message(timeout=5)
        connection.notify("initialized")
        connection.request("thread/start", {"model": "m", "cwd": ".",
                                            "approvalPolicy": "untrusted", "sandbox": "read-only"})
        connection.next_message(timeout=5)
        connection.request("turn/start", {"threadId": "thread-1", "input": "a bare string",
                                          "approvalPolicy": "untrusted"})
        reply = connection.next_message(timeout=5)
        self.assertIn("input must be a non-empty list", reply["error"]["message"])


class UsageTests(unittest.TestCase):
    """Usage comes from `thread/tokenUsage/updated`, in the real server's shape."""

    @staticmethod
    def usage(total_in, total_out, last_in, last_out, item_id):
        return {"method": "thread/tokenUsage/updated",
                "params": {"threadId": "thread-1", "turnId": item_id,
                           "tokenUsage": {
                               "total": {"totalTokens": total_in + total_out, "inputTokens": total_in,
                                         "cachedInputTokens": 0, "cacheWriteInputTokens": 0,
                                         "outputTokens": total_out, "reasoningOutputTokens": 0},
                               "last": {"totalTokens": last_in + last_out, "inputTokens": last_in,
                                        "cachedInputTokens": 0, "cacheWriteInputTokens": 0,
                                        "outputTokens": last_out, "reasoningOutputTokens": 0},
                               "modelContextWindow": 258400}}}

    def test_usage_is_read_from_the_token_usage_notification(self):
        launch = fake_launcher(scenario(self.usage(100, 20, 100, 20, "t1"),
                                        tool_call("submit_result", SUBMISSION), COMPLETED))
        executor = CodexGateExecutor(spec(), launch)
        with executor:
            executor.run_turn("go", SCHEMA, HarborBridge())
        self.assertEqual(executor.usage["input_tokens"], 100)
        self.assertEqual(executor.usage["output_tokens"], 20)
        self.assertEqual(executor.usage["total_tokens"], 120)
        self.assertEqual(executor.turn_usage["input_tokens"], 100)
        self.assertEqual(executor.receipt()["context_window"], 258400)

    def test_a_cumulative_total_is_assigned_not_summed(self):
        # `total` is per-thread, so adding it once per turn would multiply the
        # bill of any gate that runs more than one turn.
        launch = fake_launcher(scenario(self.usage(180, 30, 80, 10, "t2"),
                                        tool_call("submit_result", SUBMISSION), COMPLETED))
        executor = CodexGateExecutor(spec(), launch)
        executor.usage = {"input_tokens":100, "output_tokens":20}
        with executor:
            executor.run_turn("first", SCHEMA, HarborBridge(), final=False)
        self.assertEqual(executor.usage["input_tokens"], 180)
        self.assertEqual(executor.turn_usage["input_tokens"], 80, "charge only the delta since turn start")

    def test_rate_limits_are_recorded_as_the_quota_signal(self):
        limits = {"primary": {"usedPercent": 12.5}}
        launch = fake_launcher(scenario({"method": "account/rateLimits/updated", "params": limits},
                                        tool_call("submit_result", SUBMISSION), COMPLETED))
        executor = CodexGateExecutor(spec(), launch)
        with executor:
            executor.run_turn("go", SCHEMA, HarborBridge())
        self.assertEqual(executor.receipt()["rate_limits"], limits)

    def test_absent_usage_stays_absent_rather_than_zero(self):
        launch = fake_launcher(scenario(tool_call("submit_result", SUBMISSION), COMPLETED))
        executor = CodexGateExecutor(spec(), launch)
        with executor:
            executor.run_turn("go", SCHEMA, HarborBridge())
        self.assertEqual(executor.usage, {})


class RealApprovalShapeTests(unittest.TestCase):
    """Shapes taken from a live `item/commandExecution/requestApproval`."""

    LIVE = {"threadId": "t", "turnId": "u", "itemId": "exec-1", "environmentId": "local",
            "command": "/bin/zsh -lc 'echo hello > gate-probe.txt'",
            "cwd": "/tmp/work",
            "commandActions": [{"type": "unknown", "command": "echo hello > gate-probe.txt"}],
            "availableDecisions": ["accept",
                                   {"acceptWithExecpolicyAmendment":
                                    {"execpolicy_amendment": ["/bin/zsh", "-lc", "echo hello"]}},
                                   "cancel"]}

    def test_structured_decision_options_are_skipped_when_choosing_a_word(self):
        # The server mixes plain words with objects; only words can be sent back.
        self.assertEqual(decision_word("reject", self.LIVE["availableDecisions"]), "cancel")
        self.assertEqual(decision_word("once", self.LIVE["availableDecisions"]), "accept")
        self.assertIsNone(decision_word("always", self.LIVE["availableDecisions"]))

    def test_a_command_is_preauthorized_by_its_exact_string(self):
        executor = CodexGateExecutor(spec(), fake_launcher({}))
        policy = {"preauthorized": ["/bin/zsh -lc 'echo hello > gate-probe.txt'"]}
        self.assertTrue(executor._preauthorized(self.LIVE, policy))

    def test_preauthorization_is_never_a_prefix_match(self):
        executor = CodexGateExecutor(spec(), fake_launcher({}))
        for allowed in ("/bin/zsh", "/bin/zsh -lc", "echo hello"):
            self.assertFalse(executor._preauthorized(self.LIVE, {"preauthorized": [allowed]}),
                             f"{allowed!r} must not authorise the full command")

    def test_an_empty_preauthorization_list_authorises_nothing(self):
        executor = CodexGateExecutor(spec(), fake_launcher({}))
        self.assertFalse(executor._preauthorized(self.LIVE, {"preauthorized": []}))

    def test_the_request_is_journaled_alongside_the_decision(self):
        launch = fake_launcher(scenario({"__approval__": {"method": "item/commandExecution/requestApproval",
                                                          "params": self.LIVE}},
                                        COMPLETED))
        executor = CodexGateExecutor(spec(), launch)
        with executor, self.assertRaises(GateBlocked):
            executor.run_turn("go", SCHEMA, HarborBridge())
        requested = next(e for e in executor.journal if e["kind"] == "approval.requested")
        self.assertEqual(requested["payload"]["params"]["command"], self.LIVE["command"])
        self.assertEqual(requested["payload"]["method"], "item/commandExecution/requestApproval")


class DynamicToolCallTests(unittest.TestCase):
    """`item/tool/call` is a server request that must be answered."""

    def test_dynamic_submission_does_not_require_a_duplicate_final_report(self):
        import tempfile
        with tempfile.TemporaryDirectory() as temp:
            transcript = Path(temp) / 'wire.jsonl'
            launch = fake_launcher(scenario(tool_call('submit_result', SUBMISSION), COMPLETED,
                                            transcript=str(transcript)))
            with CodexGateExecutor(spec(dynamic_tools=tool_definitions(SCHEMA)), launch) as executor:
                self.assertEqual(executor.run_turn('go', SCHEMA, HarborBridge()), SUBMISSION)
            messages = [json.loads(line) for line in transcript.read_text().splitlines()]
            turn = next(m for m in messages if m.get('method') == 'turn/start')
            self.assertNotIn('outputSchema', turn['params'])

    def test_invalid_dynamic_submission_is_refused_before_acknowledgement(self):
        bad = {**SUBMISSION, 'extra': 'not allowed'}
        launch = fake_launcher(scenario(
            {'__request__': {'method': 'item/tool/call', 'params': {
                'callId': 'invalid', 'tool': 'submit_result', 'arguments': bad}}},
            {'__request__': {'method': 'item/tool/call', 'params': {
                'callId': 'corrected', 'tool': 'submit_result', 'arguments': SUBMISSION}}},
            COMPLETED))
        with CodexGateExecutor(spec(), launch) as executor:
            self.assertEqual(executor.run_turn('go', SCHEMA, HarborBridge()), SUBMISSION)
            results = [e['payload'] for e in executor.journal if e['kind'] == 'tool.result']
            self.assertEqual([r['ok'] for r in results], [False, True])

    def test_citation_limit_refusal_is_actionable_and_stays_in_one_turn(self):
        schema = {'type': 'object', 'required': ['evidence_ids'], 'properties': {
            'evidence_ids': {'type': 'array', 'maxItems': 6, 'items': {'type': 'string'}}}}
        launch = fake_launcher(scenario(*[
            {'__request__': {'method': 'item/tool/call', 'params': {
                'callId': str(n), 'tool': 'submit_result',
                'arguments': {'evidence_ids': ['o' + str(i) for i in range(n)]}}}}
            for n in (13, 6)], COMPLETED))
        with CodexGateExecutor(spec(), launch) as executor:
            result = executor.run_turn('go', schema, HarborBridge())
            self.assertEqual(len(result['evidence_ids']), 6)
            refusals = [e['payload'] for e in executor.journal
                        if e['kind'] == 'tool.result' and not e['payload']['ok']]
            self.assertEqual(len(refusals), 1)
            self.assertIn('evidence_ids', refusals[0]['error'])
            self.assertIn('6', refusals[0]['error'])
            self.assertEqual(len(executor.turn_ids), 1)

    def test_dynamic_submission_is_not_replaced_by_final_acknowledgement(self):
        launch = fake_launcher(scenario(
            {"__request__": {"method": "item/tool/call", "params": {
                "threadId": "t", "turnId": "u", "callId": "result",
                "tool": "submit_result", "arguments": SUBMISSION}}},
            {"method": "item/completed", "params": {"item": {
                "type": "agentMessage", "id": "ack", "text": "Result submitted."}}},
            COMPLETED))
        with CodexGateExecutor(spec(), launch) as executor:
            self.assertEqual(executor.run_turn('go', SCHEMA, HarborBridge()), SUBMISSION)

    def call(self, tool, arguments, bridge):
        launch = fake_launcher(scenario(
            {"__request__": {"method": "item/tool/call",
                             "params": {"threadId": "t", "turnId": "u", "callId": "c1",
                                        "tool": tool, "arguments": arguments}}},
            tool_call("submit_result", SUBMISSION), COMPLETED))
        published = ("read_evidence", "submit_result")
        if bridge.exec_ is not None:
            published += ("container_exec",)
        executor = CodexGateExecutor(spec(tools=published), launch)
        with executor:
            executor.run_turn("go", SCHEMA, bridge)
        return executor

    def test_a_published_tool_is_run_and_its_result_returned(self):
        seen = {}
        bridge = HarborBridge(exec_=lambda command, timeout: seen.update(cmd=command) or
                              {"stdout": "ok", "exit_code": 0})
        executor = self.call("container_exec", {"command": ["echo", "hi"]}, bridge)
        self.assertEqual(seen["cmd"], ["echo", "hi"])
        result = next(e for e in executor.journal if e["kind"] == "tool.result")
        self.assertTrue(result["payload"]["ok"])

    def test_an_unpublished_tool_is_refused_but_still_answered(self):
        executor = self.call("host_shell", {"command": ["rm", "-rf", "/"]}, HarborBridge())
        result = next(e for e in executor.journal if e["kind"] == "tool.result")
        self.assertFalse(result["payload"]["ok"])
        self.assertEqual(executor.state, "completed", "a refusal must not strand the turn")


if __name__ == "__main__":
    unittest.main()
