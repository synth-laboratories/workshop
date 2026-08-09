from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from synth_local_runtime.models import EventInput
from synth_local_runtime.store import RuntimeStore


class RuntimeStoreTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.path = Path(self.temp.name) / "runtime.sqlite3"
        self.store = RuntimeStore(self.path)

    def tearDown(self) -> None:
        self.store.close_thread_connection()
        self.temp.cleanup()

    def test_event_sequences_replay_and_persist(self) -> None:
        session = self.store.create_session(
            {"kind": "local", "model": "laguna-xs-2.1", "adapter": None}
        )
        first, inserted = self.store.append_event(
            EventInput(
                session_id=session["id"],
                source="local",
                event_kind="one",
                payload={"value": 1},
            )
        )
        second, _ = self.store.append_event(
            EventInput(
                session_id=session["id"],
                source="local",
                event_kind="two",
                payload={"value": 2},
            )
        )
        self.assertTrue(inserted)
        self.assertEqual(first["sequence"], 1)
        self.assertEqual(second["sequence"], 2)

        page = self.store.list_events(session["id"], after_sequence=1)
        self.assertEqual([event["eventKind"] for event in page["events"]], ["two"])
        self.assertEqual(page["nextSequence"], 2)

        reopened = RuntimeStore(self.path)
        try:
            self.assertEqual(reopened.get_session(session["id"])["latestCursor"], 2)
        finally:
            reopened.close_thread_connection()

    def test_remote_event_deduplication(self) -> None:
        session = self.store.create_session({"kind": "intern", "mode": "sync"})
        event = EventInput(
            session_id=session["id"],
            source="intern",
            remote_sequence=9,
            event_kind="agent_message",
            payload={"body": "hello"},
        )
        first, inserted_first = self.store.append_event(event)
        second, inserted_second = self.store.append_event(event)
        self.assertTrue(inserted_first)
        self.assertFalse(inserted_second)
        self.assertEqual(first["sequence"], second["sequence"])
        self.assertEqual(self.store.get_cursor(session["id"], "intern"), 9)


if __name__ == "__main__":
    unittest.main()
