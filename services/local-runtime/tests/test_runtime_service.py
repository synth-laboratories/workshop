from __future__ import annotations

import tempfile
import time
import unittest
from pathlib import Path

from synth_local_runtime.config import RuntimeConfig
from synth_local_runtime.service import RuntimeService


def config_for(path: Path, *, delay_ms: int = 1) -> RuntimeConfig:
    return RuntimeConfig(
        host="127.0.0.1",
        port=0,
        data_dir=path,
        runtime_token=None,
        connection_file=None,
        backend_url="https://example.invalid",
        synth_api_key=None,
        intern_demo=True,
        laguna_base_url=None,
        laguna_stub_delay_ms=delay_ms,
        openrouter_api_key=None,
        laguna_model_path=None,
        visuals_root=None,
        workshop_root=None,
    )


def wait_for_terminal(service: RuntimeService, session_id: str, timeout: float = 5.0) -> list[dict]:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        events = service.events(session_id, after_sequence=0, limit=500)["events"]
        if any(
            event["eventKind"] in {"run.completed", "run.failed", "run.cancelled"}
            for event in events
        ):
            return events
        time.sleep(0.02)
    raise AssertionError("run did not become terminal")


class RuntimeServiceTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.service = RuntimeService(config_for(Path(self.temp.name)))

    def tearDown(self) -> None:
        self.service.store.close_thread_connection()
        self.temp.cleanup()

    def test_local_stream_records_identity_usage_and_completion(self) -> None:
        session = self.service.create_session(
            {"kind": "local", "model": "laguna-xs-2.1", "adapter": None}
        )
        response = self.service.send_message(session["id"], "Inspect the repository")
        self.assertTrue(response["runId"].startswith("run_"))
        events = wait_for_terminal(self.service, session["id"])
        kinds = [event["eventKind"] for event in events]
        self.assertIn("message.delta", kinds)
        self.assertIn("message.completed", kinds)
        self.assertIn("usage.recorded", kinds)
        self.assertIn("thought.created", kinds)
        self.assertIn("tool.requested", kinds)
        self.assertIn("approval.requested", kinds)
        usage = next(event for event in events if event["eventKind"] == "usage.recorded")
        self.assertEqual(usage["payload"]["model"], "laguna-xs-2.1")
        self.assertIsNone(usage["payload"]["adapter"])
        self.assertEqual(self.service.get_session(session["id"])["status"], "ready")

    def test_intern_sync_demo_uses_receipt_and_cursor_events(self) -> None:
        session = self.service.create_session({"kind": "intern", "mode": "sync"})
        self.assertEqual(session["metadata"]["internTransport"], "demo")
        self.service.send_message(session["id"], "Investigate the cursor bug")
        events = wait_for_terminal(self.service, session["id"], timeout=6.0)
        kinds = [event["eventKind"] for event in events]
        self.assertIn("command.receipt", kinds)
        self.assertIn("agent_message", kinds)
        self.assertIn("resource_ref.created", kinds)
        self.assertNotIn("resource.linked", kinds)
        agent_message = next(
            event for event in events if event["eventKind"] == "agent_message"
        )
        self.assertTrue(agent_message["payload"]["body"])
        resource_ref = next(
            event for event in events if event["eventKind"] == "resource_ref.created"
        )
        self.assertEqual(resource_ref["payload"]["kind"], "artifact")
        self.assertTrue(resource_ref["payload"]["id"].startswith("demo-artifact-"))
        intern_events = [event for event in events if event["source"] == "intern"]
        remote_sequences = [
            event["remoteSequence"]
            for event in intern_events
            if event["remoteSequence"] is not None
        ]
        self.assertEqual(remote_sequences, sorted(set(remote_sequences)))

    def test_async_is_singleton_leave_safe_and_checkpointed(self) -> None:
        first = self.service.create_session({"kind": "intern", "mode": "async"})
        second = self.service.create_session({"kind": "intern", "mode": "async"})
        self.assertEqual(first["id"], second["id"])
        self.assertTrue(first["metadata"]["leaveSafe"])
        self.service.send_message(first["id"], "Run a background investigation")
        events = wait_for_terminal(self.service, first["id"], timeout=7.0)
        checkpoint = next(
            event for event in events if event["eventKind"] == "checkpoint.created"
        )
        self.assertTrue(checkpoint["payload"]["leave_safe"])
        refreshed = self.service.get_session(first["id"])
        self.assertTrue(refreshed["metadata"]["leaveSafe"])
        self.assertEqual(refreshed["status"], "ready")

    def test_projects_attach_to_sessions_and_store_counts(self) -> None:
        project = self.service.create_project({"path": self.temp.name, "name": "Workshop"})
        session = self.service.create_session(
            {"kind": "local", "model": "laguna-xs-2.1"}, project_id=project["id"]
        )
        self.assertEqual(session["projectId"], project["id"])
        self.assertEqual(self.service.get_project(project["id"])["name"], "Workshop")
        self.assertGreaterEqual(self.service.store.counts()["projects"], 1)


if __name__ == "__main__":
    unittest.main()
