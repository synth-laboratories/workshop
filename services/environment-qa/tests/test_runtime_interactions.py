import time
import unittest
from concurrent.futures import ThreadPoolExecutor

import test_dispatch as fixtures
from test_dispatch import ANSWER, submit, completed
from environment_qa import dispatch
from environment_qa.core import Conflict
from environment_qa.runtime_interactions import respond
from environment_qa.worker import recover


class PermissionIntegrationTests(unittest.TestCase):
    setUp = fixtures.DispatchTests.setUp
    tearDown = fixtures.DispatchTests.tearDown
    call = fixtures.DispatchTests.call

    def test_clarification_answers_resume_the_same_turn(self):
        self.store.mutate(self.run["id"], "test.hitl", lambda r: r.update(mode="hitl"))
        question = {"__request__": {"method":"item/tool/requestUserInput", "params":{
            "threadId":"t", "turnId":"u", "itemId":"i", "questions":[
                {"id":"scope", "header":"Scope", "question":"Which fixture scope?"}]}}}
        with ThreadPoolExecutor(max_workers=1) as pool:
            future = pool.submit(self.call, question, submit(ANSWER), completed())
            try:
                end = time.monotonic() + 5
                interaction = None
                while time.monotonic() < end:
                    interaction = next((i for i in self.store.get(self.run["id"])["interactions"] if i["status"] == "open"), None)
                    if interaction: break
                    time.sleep(.02)
                self.assertIsNotNone(interaction)
                body = dict(interaction_id=interaction["id"], context_digest=interaction["context_digest"],
                            decision="answer", reason="Fixture context only", actor="agent-cua", request_key="answer-1",
                            answers={"scope":{"answers":["Only this isolated fixture"]}})
                with self.assertRaises(ValueError):
                    respond(self.store, self.run["id"], body | {"answers":{}}, "clarification")
                respond(self.store, self.run["id"], body, "clarification")
                self.assertEqual(future.result(timeout=5), ANSWER)
                recorded = self.store.get(self.run["id"])["interactions"][-1]
                self.assertEqual(recorded["answers"], body["answers"])
                self.assertEqual(recorded["actor"], "agent-cua")
            finally:
                if not future.done():
                    self.store.mutate(self.run["id"], "test.cancel", lambda r:r.update(status="cancelling"))

    def test_non_hitl_does_not_invent_clarification(self):
        from environment_qa.codex_executor import GateBlocked
        with self.assertRaises(GateBlocked):
            self.call({"__request__":{"method":"item/tool/requestUserInput", "params":{"questions":[]}}})

    def test_permission_wait_can_be_resolved_without_restarting_the_turn(self):
        self.store.mutate(self.run["id"], "test.hitl", lambda r: r.update(mode="hitl"))
        approval = {"__approval__": {"method": "item/commandExecution/requestApproval",
                    "params": {"command": "fixture-only", "availableDecisions": ["accept", "decline"]}}}
        with ThreadPoolExecutor(max_workers=1) as pool:
            future = pool.submit(self.call, approval, submit(ANSWER), completed())
            try:
                end = time.monotonic() + 5
                interaction = None
                while time.monotonic() < end:
                    run = self.store.get(self.run["id"])
                    interaction = next((i for i in run["interactions"] if i["status"] == "open"), None)
                    if interaction: break
                    time.sleep(.02)
                self.assertIsNotNone(interaction)
                self.assertFalse(future.done())
                body = dict(interaction_id=interaction["id"], context_digest=interaction["context_digest"],
                            decision="once", reason="Authorized fixture action", actor="agent-cua", request_key="permission-1")
                with self.assertRaises(Conflict):
                    respond(self.store, run["id"], body | {"context_digest": "stale"})
                with self.assertRaises(Conflict):
                    self.store.decide(run["id"], interaction["id"], "confirm", "wrong API", interaction["context_digest"], run["revision"], "wrong")
                result = respond(self.store, run["id"], body)
                self.assertEqual(respond(self.store, run["id"], body), result)
                with self.assertRaises(Conflict):
                    respond(self.store, run["id"], body | {"actor": "local-human"})
                self.assertEqual(future.result(timeout=5), ANSWER)
                events = self.store.events(run["id"])
                decision = next(e for e in events if e["kind"] == "codex.approval.decided")
                self.assertEqual(decision["payload"]["payload"]["actor"], "agent-cua")
                self.assertFalse(decision["payload"]["payload"]["human"])
                self.assertEqual(sum(e["kind"] == "codex.turn.acknowledged" for e in events), 1)
            finally:
                if not future.done():
                    self.store.mutate(self.run["id"], "test.cancel", lambda r: r.update(status="cancelling"))

    def test_recovery_supersedes_old_connection_permission(self):
        def pending(run):
            run["interactions"].append({"id":"i", "type":"permission", "status":"open"})
        self.store.mutate(self.run["id"], "test.pending", pending)
        recover(self.store)
        self.assertEqual(self.store.get(self.run["id"])["interactions"][0]["status"], "superseded")
