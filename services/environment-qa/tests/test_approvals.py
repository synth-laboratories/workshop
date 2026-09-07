import tempfile
import unittest
from pathlib import Path

from environment_qa.approvals import StoreApprovalLedger
from environment_qa.core import Store

REQUEST = {"method": "permissions/request", "params": {"tool": "container_exec"}}


def spec(**overrides):
    return {"run_id": "run-1", "gate_id": "review", "attempt": "attempt-1",
            "input_sha256": "abc123", **overrides}


class DurableApprovalTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name) / "store"
        self.now = [1000.0]
        self.ledger = self.open()

    def open(self):
        return StoreApprovalLedger(Store(self.root), clock=lambda: self.now[0])

    def tearDown(self):
        self.temp.cleanup()

    def test_a_decision_survives_a_restart(self):
        self.ledger.record(spec(), REQUEST, "once", actor="local-human", revision=3)
        entry, why = self.open().lookup(spec(), REQUEST, 3)   # a whole new process would do this
        self.assertEqual(why, "fresh")
        self.assertEqual((entry["decision"], entry["actor"]), ("once", "local-human"))

    def test_an_approval_is_single_use_across_processes(self):
        self.ledger.record(spec(), REQUEST, "once", actor="local-human", revision=3)
        self.assertEqual(self.open().lookup(spec(), REQUEST, 3)[1], "fresh")
        self.assertEqual(self.open().lookup(spec(), REQUEST, 3)[1], "already_spent")

    def test_a_new_attempt_does_not_inherit_consent(self):
        self.ledger.record(spec(), REQUEST, "once", actor="local-human", revision=3)
        self.assertEqual(self.ledger.lookup(spec(attempt="attempt-2"), REQUEST, 3)[1], "stale_attempt")

    def test_a_superseded_revision_does_not_unblock(self):
        self.ledger.record(spec(), REQUEST, "once", actor="local-human", revision=3)
        self.assertEqual(self.ledger.lookup(spec(), REQUEST, 4)[1], "stale_revision")

    def test_a_decision_does_not_cross_to_another_gate(self):
        self.ledger.record(spec(), REQUEST, "once", actor="local-human", revision=3)
        self.assertEqual(self.ledger.lookup(spec(gate_id="probe"), REQUEST, 3)[1], "no_decision")

    def test_an_expired_decision_is_refused_and_stays_refused(self):
        self.ledger.record(spec(), REQUEST, "once", actor="local-human", revision=3, ttl_seconds=60)
        self.now[0] += 61
        self.assertEqual(self.ledger.lookup(spec(), REQUEST, 3)[1], "expired")
        self.now[0] -= 61   # a clock that goes backwards must not resurrect it
        self.assertEqual(self.ledger.lookup(spec(), REQUEST, 3)[1], "fresh")

    def test_a_different_command_is_not_covered(self):
        self.ledger.record(spec(), {"method": "execCommandApproval", "params": {"command": ["ls"]}},
                           "once", actor="local-human", revision=3)
        other = {"method": "execCommandApproval", "params": {"command": ["curl", "evil"]}}
        self.assertEqual(self.ledger.lookup(spec(), other, 3)[1], "no_decision")

    def test_the_decision_vocabulary_is_not_part_of_the_subject(self):
        self.ledger.record(spec(), REQUEST, "once", actor="local-human", revision=3)
        offered = {"method": "permissions/request",
                   "params": {"tool": "container_exec", "availableDecisions": ["approve", "decline"]}}
        self.assertEqual(self.ledger.lookup(spec(), offered, 3)[1], "fresh")

    def test_re_recording_clears_a_spent_decision(self):
        self.ledger.record(spec(), REQUEST, "once", actor="local-human", revision=3)
        self.ledger.lookup(spec(), REQUEST, 3)
        self.ledger.record(spec(), REQUEST, "reject", actor="local-human", revision=3)
        entry, why = self.ledger.lookup(spec(), REQUEST, 3)
        self.assertEqual((why, entry["decision"]), ("fresh", "reject"))

    def test_an_unknown_decision_is_refused(self):
        with self.assertRaises(ValueError):
            self.ledger.record(spec(), REQUEST, "maybe", actor="local-human", revision=3)

    def test_pending_lists_unspent_decisions_for_a_resume_view(self):
        self.ledger.record(spec(), REQUEST, "once", actor="agent-cua", revision=3)
        self.assertEqual([p["actor"] for p in self.ledger.pending("run-1")], ["agent-cua"])
        self.ledger.lookup(spec(), REQUEST, 3)
        self.assertEqual(self.ledger.pending("run-1"), [])


if __name__ == "__main__":
    unittest.main()
