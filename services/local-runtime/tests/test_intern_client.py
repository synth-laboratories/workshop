from __future__ import annotations

import unittest
from typing import Any

from synth_local_runtime.intern_client import InternHttpClient


class RecordingInternHttpClient(InternHttpClient):
    def __init__(self) -> None:
        super().__init__(base_url="https://example.invalid", api_key="test-key")
        self.calls: list[dict[str, Any]] = []

    def request(
        self,
        method: str,
        path: str,
        *,
        body: dict[str, Any] | None = None,
        query: dict[str, Any] | None = None,
    ) -> Any:
        call = {"method": method, "path": path, "body": body, "query": query}
        self.calls.append(call)
        return call


class InternHttpClientContractTests(unittest.TestCase):
    def test_async_message_uses_openapi_command_envelope(self) -> None:
        client = RecordingInternHttpClient()

        result = client.send_async(
            command_id="cmd-1",
            idempotency_key="idem-1",
            expected_generation=7,
            kind="message",
            body="Investigate the failure",
            context={"desktop_session_id": "session-1"},
        )

        self.assertEqual(result["method"], "POST")
        self.assertEqual(result["path"], "/smr/research-intern/async/messages")
        self.assertEqual(
            result["body"],
            {
                "command_id": "cmd-1",
                "idempotency_key": "idem-1",
                "expected_generation": 7,
                "command_kind": "message",
                "payload": {
                    "body": "Investigate the failure",
                    "context": {"desktop_session_id": "session-1"},
                },
            },
        )
        self.assertNotIn("instruction_kind", result["body"])

    def test_sync_message_uses_typed_sync_envelope(self) -> None:
        client = RecordingInternHttpClient()

        result = client.send_sync(
            "sync-1",
            command_id="cmd-2",
            idempotency_key="idem-2",
            expected_generation=3,
            body="Continue",
        )

        self.assertEqual(
            result["path"], "/smr/research-intern/sync-sessions/sync-1/commands"
        )
        self.assertEqual(result["body"]["command_kind"], "operator_message")
        self.assertEqual(result["body"]["payload"]["body"], "Continue")
        self.assertEqual(result["body"]["mode"], "sync")


if __name__ == "__main__":
    unittest.main()
