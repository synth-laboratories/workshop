import json
import tempfile
import unittest
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from unittest.mock import patch

from environment_qa.bundles import export_bundle, verified_path
from environment_qa.core import Store, Conflict, verify_seal
from environment_qa.worker import step, recover


class QaTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.task = self.root / "task"
        (self.task / "tests").mkdir(parents=True)
        (self.task / "instruction.md").write_text("Create a source file; compilation byproducts are permitted.")
        (self.task / "task.toml").write_text('version="1.0"\n')
        (self.task / "tests/test.sh").write_text("#!/bin/sh\nexit 0\n")
        (self.task / "tests/test_outputs.py").write_text('import os\ndef test_files():\n    files = os.listdir("/app")\n    assert files == ["main.c"]\n')
        self.store = Store(self.root / "store")
        self.bundle = export_bundle(self.task, self.store.root, [self.root])

    def tearDown(self):
        self.temp.cleanup()

    def run_to_gate(self, mode="automated"):
        run = self.store.create(self.bundle, mode=mode)
        while step(self.store, run["id"]):
            pass
        return self.store.get(run["id"])

    def test_auto_seal_and_no_false_pass(self):
        run = self.run_to_gate()
        self.assertEqual(run["status"], "completed")
        self.assertEqual(run["verdict"], "inconclusive")
        self.assertEqual(len(run["findings"]), 1)
        self.assertTrue(verify_seal(run))
        self.assertFalse(run["qualified"])
        with self.assertRaises(Conflict):
            self.store.control(run["id"], "resume", run["revision"], "key")

    def test_post_seal_review_is_separate_and_revisioned(self):
        from environment_qa.adjudication import seed, decide, get
        run = self.run_to_gate()
        proposal = {"prediction_seal": run["seal"]["sha256"], "gold": [{"id": "public-1"}],
                    "findings": [{"prediction_id": run["findings"][0]["id"], "disposition": "needs_evidence", "reason": "proposed"}]}
        with self.assertRaises(ValueError):
            seed(self.store, run["id"], proposal | {"prediction_seal": "wrong"})
        seed(self.store, run["id"], proposal)
        body = {"revision": 0, "prediction_seal": run["seal"]["sha256"], "request_key": "review-1",
                "prediction_id": run["findings"][0]["id"], "disposition": "matched_public", "gold_id": "public-1", "reason": "Checked source"}
        result = decide(self.store, run["id"], body)
        self.assertEqual(decide(self.store, run["id"], body), result)
        self.assertTrue(result["human_confirmed"])
        self.assertEqual(result["primary_metrics_status"], "pending_independent_gold_adjudication")
        self.assertEqual(self.store.get(run["id"]), run)
        self.assertEqual(get(Store(self.store.root), run["id"]), result)
        with self.assertRaises(Conflict):
            decide(self.store, run["id"], body | {"request_key": "stale"})
        with self.assertRaises(Conflict):
            decide(self.store, run["id"], body | {"reason": "changed"})

    def test_decision_after_restart_and_duplicate(self):
        run = self.run_to_gate("hitl")
        self.assertEqual(run["status"], "waiting_interaction")
        reopened = Store(self.store.root)
        recover(reopened)
        interaction = run["interactions"][0]
        args = (run["id"], interaction["id"], "confirm", "Source evidence checked", interaction["context_digest"], run["revision"], "decision-key")
        decided = reopened.decide(*args)
        self.assertTrue(verify_seal(decided))
        self.assertEqual(decided, reopened.decide(*args))
        self.assertEqual(decided["findings"][0]["disposition"], "confirmed")

    def test_cua_decision_preserves_actor_and_idempotency(self):
        run = self.run_to_gate("hitl")
        i = run["interactions"][0]
        args = (run["id"], i["id"], "request_evidence", "Agent reviewed source; execution evidence missing", i["context_digest"], run["revision"], "cua-review")
        with self.assertRaises(ValueError):
            self.store.decide(*args, actor="invented")
        result = self.store.decide(*args, actor="agent-cua")
        self.assertEqual(result["interactions"][0]["actor"], "agent-cua")
        self.assertTrue(verify_seal(result))
        self.assertEqual(result, self.store.decide(*args, actor="agent-cua"))
        with self.assertRaises(Conflict):
            self.store.decide(*args, actor="local-human")

    def test_stale_decision_and_paused_decision(self):
        run = self.run_to_gate("hitl")
        i = run["interactions"][0]
        paused = self.store.control(run["id"], "pause", run["revision"], "pause")
        with self.assertRaises(Conflict):
            self.store.decide(run["id"], i["id"], "confirm", "checked", i["context_digest"], run["revision"], "stale")
        with self.assertRaises(Conflict):
            self.store.decide(run["id"], i["id"], "confirm", "checked", "wrong", paused["revision"], "wrong")
        decided = self.store.decide(run["id"], i["id"], "dismiss", "Allowed restriction", i["context_digest"], paused["revision"], "decision")
        self.assertEqual(decided["status"], "paused")
        self.assertIsNone(decided["seal"])
        resumed = self.store.control(run["id"], "resume", decided["revision"], "resume")
        step(self.store, run["id"])
        self.assertTrue(verify_seal(self.store.get(run["id"])))

    def test_cancel_never_passes(self):
        run = self.store.create(self.bundle)
        cancelled = self.store.control(run["id"], "cancel", 0, "cancel")
        self.assertEqual(cancelled["status"], "cancelled")
        self.assertIsNone(cancelled["verdict"])
        self.assertFalse(step(self.store, run["id"]))

    def test_only_one_worker_claims(self):
        run = self.store.create(self.bundle)
        with ThreadPoolExecutor(max_workers=4) as pool:
            list(pool.map(lambda _: step(self.store, run["id"]), range(4)))
        events = self.store.events(run["id"])
        self.assertEqual(sum(e["kind"] == "gate.started" for e in events), len(self.store.get(run["id"])["evidence"]))

    def test_recovery_retains_reservation(self):
        run = self.store.create(self.bundle)
        def interrupted(r):
            r["status"] = "running"
            r["gates"][0].update(status="running", attempt="old")
            r["budget"]["reserved_usd"] = 0.5
        self.store.mutate(run["id"], "test.interrupted", interrupted)
        recover(self.store)
        recovered = self.store.get(run["id"])
        self.assertEqual(recovered["status"], "paused")
        self.assertEqual(recovered["gates"][0]["status"], "inconclusive")
        self.assertEqual(recovered["budget"]["reserved_usd"], 0.5)

    def test_bundle_excludes_later_metadata_and_rejects_links(self):
        (self.task / "gold").mkdir()
        (self.task / "gold/answer.json").write_text("secret answer")
        (self.task / ".env").write_text("TOKEN=secret")
        self.assertEqual(export_bundle(self.task, self.store.root, [self.root]), self.bundle)
        (self.task / "tests/leak").symlink_to(self.task / ".env")
        with self.assertRaises(ValueError):
            export_bundle(self.task, self.store.root, [self.root])

    def test_mutated_bundle_fails_integrity(self):
        path = verified_path(self.store, self.bundle)
        (path / "instruction.md").write_text("tampered")
        with self.assertRaises(ValueError):
            verified_path(self.store, self.bundle)

    def test_budget_prevents_dispatch(self):
        run = self.store.create(self.bundle, reviewer="ai", budget_usd=0.01)
        step(self.store, run["id"])
        with patch("environment_qa.worker.ai_request", return_value=(None, None, None, 1.0)), patch("environment_qa.worker.call_ai") as call:
            step(self.store, run["id"])
            call.assert_not_called()
        self.assertIn("budget", self.store.get(run["id"])["limitations"][0])

    def test_create_idempotency(self):
        first = self.store.create(self.bundle, request_key="create")
        self.assertEqual(first, self.store.create(self.bundle, request_key="create"))
        with self.assertRaises(Conflict):
            self.store.create(self.bundle, request_key="create", mode="hitl")


if __name__ == "__main__":
    unittest.main()
