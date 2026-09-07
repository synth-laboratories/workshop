import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from environment_qa.bundles import export_bundle
from environment_qa.core import Store, verify_seal
from environment_qa.dag import run_until_idle
from environment_qa.follow import (SCHEMA_STREAM_EVENT, envelope, follow, format_sse, poll_payload, render)
from environment_qa.policy import full_policy


class FollowTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.task = self.root / "task"
        self.task.mkdir()
        (self.task / "instruction.md").write_text("Test fixture")
        (self.task / "task.toml").write_text('version="1"')
        self.store = Store(self.root / "store")
        self.bundle = export_bundle(self.task, self.store.root, [self.root])

    def tearDown(self):
        self.tmp.cleanup()

    @staticmethod
    def fake(store, run, gate, path):
        return {"findings": [], "limitations": []}

    def completed_run(self):
        run = self.store.create(self.bundle, reviewer="ai", pipeline=full_policy(), surface="test")
        result = run_until_idle(self.store, run["id"], self.fake)
        self.assertTrue(verify_seal(result))
        return run["id"]

    # --- the event payload has to say which gate moved ---

    def test_events_name_the_gate_that_changed(self):
        run_id = self.completed_run()
        moves = [(g["id"], g["from"], g["to"])
                 for e in self.store.events(run_id) for g in e["payload"].get("gates", [])]
        self.assertTrue(moves, "no gate transition was recorded")
        self.assertIn("running", [to for _, _, to in moves])
        self.assertIn("succeeded", [to for _, _, to in moves])
        for _, previous, to in moves:
            self.assertNotEqual(previous, to)

    def test_a_gate_reaches_running_before_it_succeeds(self):
        run_id = self.completed_run()
        order = [(g["id"], g["to"]) for e in self.store.events(run_id) for g in e["payload"].get("gates", [])]
        first = order[0][0]
        self.assertEqual(order.index((first, "running")) < order.index((first, "succeeded")), True)

    def test_run_status_transition_is_recorded_once_it_changes(self):
        run_id = self.completed_run()
        runs = [e["payload"]["run"] for e in self.store.events(run_id) if "run" in e["payload"]]
        self.assertTrue(runs)
        self.assertEqual(runs[-1]["to"], "completed")

    # --- envelopes stay byte-compatible with the house trace-stream contract ---

    def test_envelope_digest_matches_the_platform_algorithm(self):
        run_id = self.completed_run()
        row = self.store.events(run_id)[0]
        event = envelope(row)
        blob = json.dumps({"kind": row["kind"], "sequence": row["seq"], "payload": row["payload"]},
                          sort_keys=True, separators=(",", ":"), default=str)
        self.assertEqual(event["digest"], hashlib.sha256(blob.encode()).hexdigest()[:16])
        self.assertEqual(event["schema"], SCHEMA_STREAM_EVENT)
        self.assertEqual(event["event_id"], str(row["seq"]))
        self.assertFalse(event["control"])

    def test_sse_framing_carries_the_sequence_as_its_id(self):
        run_id = self.completed_run()
        event = envelope(self.store.events(run_id)[0])
        record = format_sse(event)
        self.assertTrue(record.startswith(f"id: {event['sequence']}\n"))
        self.assertIn(f"event: {event['kind']}\n", record)
        self.assertTrue(record.endswith("\n\n"))
        self.assertEqual(json.loads(record.split("data: ", 1)[1]), event)

    # --- the cursor is what makes reattaching safe ---

    def test_poll_replays_from_zero_and_advances_the_cursor(self):
        run_id = self.completed_run()
        page = poll_payload(self.store, run_id)
        self.assertEqual(page["cursor"]["kind"], "sequence")
        self.assertEqual(page["cursor"]["after"], 0)
        self.assertTrue(page["cursor"]["closed"])
        self.assertEqual(page["events"][0]["kind"], "stream.subscribed")
        self.assertTrue(page["events"][0]["control"])
        self.assertEqual(page["cursor"]["next"], page["events"][-1]["sequence"])

    def test_resuming_from_a_cursor_does_not_replay(self):
        run_id = self.completed_run()
        page = poll_payload(self.store, run_id)
        resumed = poll_payload(self.store, run_id, after=page["cursor"]["next"])
        self.assertEqual(resumed["events"], [])
        midpoint = page["events"][2]["sequence"]
        tail = poll_payload(self.store, run_id, after=midpoint)
        self.assertTrue(all(e["sequence"] > midpoint for e in tail["events"]))
        self.assertNotIn("stream.subscribed", [e["kind"] for e in tail["events"]])

    def test_a_short_page_reports_more_is_waiting(self):
        run_id = self.completed_run()
        page = poll_payload(self.store, run_id, limit=1)
        self.assertTrue(page["cursor"]["has_more"])
        self.assertEqual(len([e for e in page["events"] if not e["control"]]), 1)

    def test_an_invalid_page_limit_is_refused(self):
        run_id = self.completed_run()
        for bad in (0, -1, 10_001, True):
            with self.assertRaises(ValueError):
                poll_payload(self.store, run_id, limit=bad)

    # --- following a run that has already closed terminates ---

    def test_follow_replays_a_closed_run_and_stops(self):
        run_id = self.completed_run()
        events = list(follow(self.store, run_id, poll=0))
        self.assertEqual(events[0]["kind"], "stream.subscribed")
        self.assertEqual([e["sequence"] for e in events[1:]],
                         [r["seq"] for r in self.store.events(run_id)])

    def test_follow_resumes_without_a_subscribe_record(self):
        run_id = self.completed_run()
        every = self.store.events(run_id)
        events = list(follow(self.store, run_id, after=every[0]["seq"], poll=0))
        self.assertNotIn("stream.subscribed", [e["kind"] for e in events])
        self.assertEqual([e["sequence"] for e in events], [r["seq"] for r in every[1:]])

    def test_render_turns_a_transition_into_one_line_per_gate(self):
        run_id = self.completed_run()
        lines = [render(envelope(r), 0.0) for r in self.store.events(run_id)]
        text = "\n".join(l for l in lines if l)
        self.assertIn("->", text)
        self.assertIn("succeeded", text)
        self.assertIsNone(render(None, 0.0))


if __name__ == "__main__":
    unittest.main()
