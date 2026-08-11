"""The hosted-gateway passthrough contract.

`RemoteResponsesBackend` sits in front of a hosted native Responses gateway
that is itself a stateless passthrough (no `previous_response_id` session
store). Codex's own Responses client never sends `previous_response_id` or
asks for `store: true` — see `RESPONSES_SERVER_PLAN.md` and upstream
`codex-rs` (PR #3212, "Never store requests") — but *any* client the
coordinator serves may resolve a local `previous_response_id` into history
before the backend ever sees the request. This test proves the backend
forwards that fully resolved history (`function_call` + `function_call_output`
included) upstream, and never leaks a `previous_response_id` or a `store:
true` that would ask the hosted gateway to keep session state it does not
have.
"""

from __future__ import annotations

import tempfile
import time
import unittest
from pathlib import Path
from typing import Any, ClassVar, Self
from unittest.mock import patch

import httpx
from fastapi.testclient import TestClient

from laguna_daemon.app import build_app
from laguna_daemon.config import LagunaConfig
from laguna_daemon.responses_api.backends import (
    remote_responses as remote_responses_module,
)


def config(path: Path, *, external_url: str) -> LagunaConfig:
    models = path / "models"
    data = path / "data"
    models.mkdir(parents=True, exist_ok=True)
    data.mkdir(parents=True, exist_ok=True)
    return LagunaConfig(
        host="127.0.0.1",
        port=7333,
        backend="external",
        api_key="test-key",
        models_dir=models,
        default_model="poolside/Laguna-XS-2.1-NVFP4-mlx",
        model="poolside/Laguna-XS-2.1-NVFP4-mlx",
        revision=None,
        draft_model=None,
        adapter=None,
        external_url=external_url,
        upstream_api_key="upstream-secret",
        data_dir=data,
        auto_load=False,
        idle_unload_after_seconds=900,
        context_length=262_144,
        started_at=time.time(),
    )


class _CapturingResponse:
    def __init__(self, status_code: int, payload: dict[str, Any]) -> None:
        self.status_code = status_code
        self._payload = payload
        self.text = str(payload)

    def json(self) -> dict[str, Any]:
        return self._payload

    def raise_for_status(self) -> None:
        if self.status_code >= 400:
            raise httpx.HTTPStatusError("error", request=None, response=None)  # type: ignore[arg-type]


class _CapturingClient:
    """Stand-in for `httpx.AsyncClient` that records every outgoing call.

    No socket is opened; this is the seam that lets the test assert on the
    exact JSON body `RemoteResponsesBackend` sends upstream without a live
    gateway.
    """

    calls: ClassVar[list[dict[str, Any]]] = []
    responses_call_count: ClassVar[int] = 0

    def __init__(self, *_args: Any, **_kwargs: Any) -> None:
        pass

    async def __aenter__(self) -> Self:
        return self

    async def __aexit__(self, *_exc_info: object) -> bool:
        return False

    async def post(
        self, url: str, *, json: dict[str, Any] | None = None, headers: dict[str, str] | None = None
    ) -> _CapturingResponse:
        _CapturingClient.calls.append({"url": url, "json": json, "headers": headers})
        if url.endswith("/input_tokens"):
            return _CapturingResponse(200, {"input_tokens": 12})
        _CapturingClient.responses_call_count += 1
        if _CapturingClient.responses_call_count == 1:
            # First turn: the upstream model issues a tool call.
            return _CapturingResponse(
                200,
                {
                    "output": [
                        {
                            "type": "function_call",
                            "name": "do_thing",
                            "call_id": "call_xyz",
                            "arguments": "{}",
                        }
                    ],
                    "usage": {"input_tokens": 12, "output_tokens": 3},
                },
            )
        # Second turn: the upstream model answers using the tool's output.
        return _CapturingResponse(
            200,
            {
                "output": [
                    {
                        "type": "message",
                        "role": "assistant",
                        "content": [{"type": "output_text", "text": "42 it is."}],
                    }
                ],
                "usage": {"input_tokens": 20, "output_tokens": 4},
            },
        )


class RemoteResponsesPassthroughTests(unittest.TestCase):
    def setUp(self) -> None:
        _CapturingClient.calls = []
        _CapturingClient.responses_call_count = 0
        self.patcher = patch.object(remote_responses_module.httpx, "AsyncClient", _CapturingClient)
        self.patcher.start()
        self.temp = tempfile.TemporaryDirectory(prefix="laguna-remote-passthrough-")
        self.client_context = TestClient(
            build_app(config(Path(self.temp.name), external_url="http://upstream.internal:9000"))
        )
        self.client = self.client_context.__enter__()
        self.headers = {"Authorization": "Bearer test-key"}

    def tearDown(self) -> None:
        self.client_context.__exit__(None, None, None)
        self.temp.cleanup()
        self.patcher.stop()

    def _responses_calls(self) -> list[dict[str, Any]]:
        return [call for call in _CapturingClient.calls if call["url"].endswith("/v1/responses")]

    def test_tool_continuation_forwards_full_history_without_previous_response_id(self) -> None:
        first = self.client.post(
            "/v1/responses",
            headers=self.headers,
            json={
                "input": "call the tool",
                "tools": [
                    {
                        "type": "function",
                        "name": "do_thing",
                        "parameters": {"type": "object", "properties": {}},
                    }
                ],
                "store": True,
            },
        ).json()
        self.assertEqual(first["output"][0]["type"], "function_call")
        call_id = first["output"][0]["call_id"]

        second = self.client.post(
            "/v1/responses",
            headers=self.headers,
            json={
                "previous_response_id": first["id"],
                "input": [
                    {
                        "type": "function_call_output",
                        "call_id": call_id,
                        "output": "42",
                    }
                ],
            },
        ).json()
        self.assertEqual(second["status"], "completed")

        responses_calls = self._responses_calls()
        self.assertEqual(len(responses_calls), 2)
        first_upstream_body = responses_calls[0]["json"]
        second_upstream_body = responses_calls[1]["json"]

        # The upstream gateway is a stateless passthrough: neither call may
        # ask it to keep session state, regardless of what the client asked
        # this daemon to do locally.
        self.assertNotIn("previous_response_id", first_upstream_body)
        self.assertFalse(first_upstream_body["store"])
        self.assertNotIn("previous_response_id", second_upstream_body)
        self.assertFalse(second_upstream_body["store"])

        # The continuation call's `input` must be the fully resolved
        # transcript — the original user turn, the model's function_call,
        # and the tool's function_call_output — never a bare
        # `previous_response_id` reference the upstream cannot resolve.
        second_input = second_upstream_body["input"]
        types = [item.get("type") for item in second_input]
        self.assertIn("function_call", types)
        self.assertIn("function_call_output", types)
        call = next(item for item in second_input if item.get("type") == "function_call")
        output = next(item for item in second_input if item.get("type") == "function_call_output")
        self.assertEqual(call["call_id"], call_id)
        self.assertEqual(output["call_id"], call_id)
        self.assertEqual(output["output"], "42")
        # The continuation body's `input` is strictly longer than the raw
        # turn-2 client input (one item): this is the regression the fix
        # closes — forwarding `turn.request` verbatim would have sent only
        # the `function_call_output` and dropped the call it answers.
        self.assertGreater(len(second_input), 1)

    def test_client_supplied_store_true_never_reaches_the_upstream(self) -> None:
        self.client.post(
            "/v1/responses",
            headers=self.headers,
            json={"input": "hello", "store": True},
        )
        responses_calls = self._responses_calls()
        self.assertEqual(len(responses_calls), 1)
        self.assertFalse(responses_calls[0]["json"]["store"])


if __name__ == "__main__":
    unittest.main()
