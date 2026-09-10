import unittest
import test_dispatch as fixtures
from environment_qa.core import Conflict
from environment_qa.retry_unstarted import retry


class RetryTests(unittest.TestCase):
    setUp = fixtures.DispatchTests.setUp
    tearDown = fixtures.DispatchTests.tearDown

    def seed(self):
        def fail(run):
            run["status"]="paused"
            gate=next(g for g in run["gates"] if g["id"]==self.gate["id"])
            gate.update(status="inconclusive",evidence_id="failure")
            run["evidence"].append({"id":"failure","gate":gate["id"],"result":{
                "limitations":["GateBlocked: Host app-server capacity exhausted"]}})
        return self.store.mutate(self.run["id"],"fixture",fail)

    def body(self,run):
        return {"revision":run["revision"],"request_key":"retry","actor":"agent-cua","reason":"Capacity fixed"}

    def test_recovery_preserves_failed_attempt_and_evidence(self):
        run=self.seed()
        updated=retry(self.store,run["id"],self.body(run))
        gate=next(g for g in updated["gates"] if g["id"]==self.gate["id"])
        self.assertEqual(gate["status"],"pending")
        self.assertEqual(gate["recovery"][0]["old_attempt"],self.token)
        self.assertTrue(updated["evidence"])
        self.assertEqual(updated["budget"],run["budget"])

    def test_started_process_cannot_be_reclassified_as_unstarted(self):
        run=self.seed()
        self.store.append_event(run["id"],"codex.process.started",{"gate_id":self.gate["id"],"attempt":self.token})
        with self.assertRaises(Conflict): retry(self.store,run["id"],self.body(run))

    def test_running_run_cannot_be_recovered(self):
        with self.assertRaises(Conflict): retry(self.store,self.run["id"],self.body(self.store.get(self.run["id"])))
