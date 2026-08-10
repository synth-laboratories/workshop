from __future__ import annotations

import asyncio
import json
import tempfile
import time
import unittest
from concurrent.futures import ThreadPoolExecutor
from copy import deepcopy
from pathlib import Path
from typing import Any

from fastapi.testclient import TestClient
from openresponses_types import ResponseResource

from laguna_daemon.app import DisconnectAwareStreamingResponse, build_app
from laguna_daemon.config import LagunaConfig
from laguna_daemon.responses_api.backends.mlx import (
    NativeMlxBackend,
    _ActivatedCustomGrammarProcessor,
    _envelope_event,
    _rehydrate_tool_calls,
    _split_reasoning,
    _TurnStateMachine,
)
from laguna_daemon.responses_api.backends.fake import FakeBackend
from laguna_daemon.responses_api.backends.protocol import ModelEvent, ToolBinding
from laguna_daemon.responses_api.errors import ResponsesError
from laguna_daemon.responses_api.ids import new_id
from laguna_daemon.responses_api.service import ResponsesService


def config(path: Path, *, context_length: int = 262_144) -> LagunaConfig:
    models = path / "models"
    data = path / "data"
    models.mkdir(parents=True, exist_ok=True)
    data.mkdir(parents=True, exist_ok=True)
    return LagunaConfig(
        host="127.0.0.1",
        port=7333,
        backend="mock",
        api_key="test-key",
        models_dir=models,
        default_model="poolside/Laguna-XS-2.1-NVFP4-mlx",
        model="poolside/Laguna-XS-2.1-NVFP4-mlx",
        revision=None,
        draft_model=None,
        adapter=None,
        external_url=None,
        upstream_api_key=None,
        data_dir=data,
        auto_load=False,
        idle_unload_after_seconds=900,
        context_length=context_length,
        started_at=time.time(),
    )


def parse_sse(text: str) -> list[dict[str, Any]]:
    events: list[dict[str, Any]] = []
    for frame in text.split("\n\n"):
        data = next((line[6:] for line in frame.splitlines() if line.startswith("data: ")), None)
        if data and data != "[DONE]":
            events.append(json.loads(data))
    return events


def normalize_generated(value: Any) -> Any:
    if isinstance(value, dict):
        return {
            key: normalize_generated(child)
            for key, child in value.items()
            if key not in {"id", "call_id", "created_at", "completed_at"}
        }
    if isinstance(value, list):
        return [normalize_generated(child) for child in value]
    return value


class NativeResponsesHttpTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory(prefix="laguna-native-responses-")
        self.client_context = TestClient(build_app(config(Path(self.temp.name))))
        self.client = self.client_context.__enter__()
        self.headers = {"Authorization": "Bearer test-key"}

    def tearDown(self) -> None:
        self.client_context.__exit__(None, None, None)
        self.temp.cleanup()

    def post(self, path: str, body: dict[str, Any]):
        return self.client.post(path, headers=self.headers, json=body)

    def test_portable_response_validates_generated_schema(self) -> None:
        response = self.post("/v1/responses", {"input": "hello", "store": False})
        self.assertEqual(response.status_code, 200)
        validated = ResponseResource.model_validate(response.json())
        self.assertEqual(validated.status, "completed")

    def test_output_limit_is_validated_even_with_extension_tools(self) -> None:
        request = {
            "input": "hello",
            "store": False,
            "tools": [{"type": "custom", "name": "shell", "format": {"type": "text"}}],
        }
        invalid_type = self.post(
            "/v1/responses", {**request, "max_output_tokens": "lots"}
        )
        self.assertEqual(invalid_type.status_code, 400)
        self.assertEqual(invalid_type.json()["error"]["param"], "max_output_tokens")

        above_ceiling = self.post(
            "/v1/responses", {**request, "max_output_tokens": 32_769}
        )
        self.assertEqual(above_ceiling.status_code, 400)
        self.assertEqual(above_ceiling.json()["error"]["param"], "max_output_tokens")

    def test_stream_semantics_and_nonstream_equivalence(self) -> None:
        request = {"input": "equivalent response", "store": False}
        nonstream = self.post("/v1/responses", request).json()
        with self.client.stream(
            "POST",
            "/v1/responses",
            headers=self.headers,
            json={**request, "stream": True},
        ) as response:
            stream_text = "".join(response.iter_text())
        self.assertTrue(stream_text.endswith("data: [DONE]\n\n"))
        events = parse_sse(stream_text)
        self.assertEqual([event["sequence_number"] for event in events], list(range(len(events))))
        event_types = [event["type"] for event in events]
        self.assertEqual(event_types[:2], ["response.created", "response.in_progress"])
        self.assertLess(event_types.index("response.output_item.added"), event_types.index("response.output_text.delta"))
        self.assertEqual(event_types[-1], "response.completed")
        streamed = events[-1]["response"]
        self.assertEqual(normalize_generated(streamed), normalize_generated(nonstream))

    def test_stream_disconnect_cancels_the_backend_generation(self) -> None:
        async def scenario() -> None:
            backend = FakeBackend()
            service = ResponsesService(config(Path(self.temp.name) / "disconnect"), backend)
            await service.start()
            disconnected = False

            async def is_disconnected() -> bool:
                return disconnected

            stream = service.stream(
                {
                    "input": "keep generating until the client leaves",
                    "stream": True,
                    "store": False,
                    "x_synth": {"fake_delay_ms": 100},
                },
                disconnected=is_disconnected,
            )
            try:
                first = await anext(stream)
                self.assertIn(b"response.created", first)
                disconnected = True
                self.assertEqual([chunk async for chunk in stream], [])
                self.assertEqual(service.coordinator.active, {})
                self.assertTrue(backend._cancelled)
            finally:
                await stream.aclose()
                await service.close()

        asyncio.run(scenario())

    def test_asgi_disconnect_cancels_stream_during_prefill_gap(self) -> None:
        async def scenario() -> None:
            started = asyncio.Event()
            cancelled = asyncio.Event()

            async def body():
                try:
                    started.set()
                    await asyncio.sleep(60)
                    yield b"too late"
                finally:
                    cancelled.set()

            async def receive() -> dict[str, str]:
                await started.wait()
                return {"type": "http.disconnect"}

            async def send(_message: dict[str, Any]) -> None:
                return None

            response = DisconnectAwareStreamingResponse(body())
            await response(
                {
                    "type": "http",
                    "asgi": {"version": "3.0", "spec_version": "2.4"},
                },
                receive,
                send,
            )
            self.assertTrue(cancelled.is_set())

        asyncio.run(scenario())

    def test_custom_tool_identity_and_continuation(self) -> None:
        tool = {
            "type": "custom",
            "name": "mcp__synth_containers",
            "description": "Use the Synth container registry.",
            "format": {"type": "text"},
        }
        first = self.post(
            "/v1/responses",
            {
                "input": "Run exactly two Craftax rollouts.",
                "tools": [tool],
                "store": True,
                # This test pins tool-item identity by output index; thinking
                # (on by default) would prepend a reasoning item.
                "reasoning": {"effort": "none"},
            },
        ).json()
        call = first["output"][0]
        self.assertEqual(call["type"], "custom_tool_call")
        self.assertEqual(call["name"], "mcp__synth_containers")
        self.assertEqual(json.loads(call["input"])["count"], 2)
        second = self.post(
            "/v1/responses",
            {
                "previous_response_id": first["id"],
                "reasoning": {"effort": "none"},
                "input": [
                    {
                        "type": "custom_tool_call_output",
                        "call_id": call["call_id"],
                        "output": "{\"rollout_ids\":[\"r1\",\"r2\"]}",
                    }
                ],
            },
        ).json()
        self.assertEqual(second["status"], "completed")
        self.assertEqual(second["previous_response_id"], first["id"])
        self.assertEqual(second["output"][0]["type"], "message")

    def test_custom_tool_stream_events_have_identity_fields(self) -> None:
        with self.client.stream(
            "POST",
            "/v1/responses",
            headers=self.headers,
            json={
                "input": "List containers",
                "stream": True,
                "store": False,
                "tools": [{"type": "custom", "name": "mcp__synth_containers"}],
                # Pins output_index == 0 for the tool item; default thinking
                # would prepend a reasoning item.
                "reasoning": {"effort": "none"},
            },
        ) as response:
            events = parse_sse("".join(response.iter_text()))
        delta = next(event for event in events if event["type"] == "response.custom_tool_call_input.delta")
        done = next(event for event in events if event["type"] == "response.custom_tool_call_input.done")
        self.assertTrue(delta["item_id"].startswith("ctc_"))
        self.assertEqual(delta["output_index"], 0)
        self.assertEqual(delta["delta"], done["input"])
        final = events[-1]["response"]["output"][0]
        self.assertEqual(final["type"], "custom_tool_call")
        self.assertNotIn("arguments", final)

    def test_function_namespace_shell_patch_and_mcp_item_families(self) -> None:
        cases = [
            ({"type": "function", "name": "weather", "parameters": {"type": "object"}}, "function_call"),
            (
                {
                    "type": "namespace",
                    "name": "repo_tools",
                    "description": "Repository tools",
                    "tools": [
                        {
                            "type": "function",
                            "name": "status",
                            "parameters": {"type": "object", "properties": {}},
                        }
                    ],
                },
                "function_call",
            ),
            ({"type": "shell", "name": "shell"}, "shell_call"),
            ({"type": "apply_patch", "name": "apply_patch"}, "apply_patch_call"),
            ({"type": "mcp", "server_label": "client_bridge"}, "mcp_call"),
        ]
        for tool, expected in cases:
            with self.subTest(expected=expected):
                response = self.post(
                    "/v1/responses",
                    {
                        "input": "use the tool",
                        "tools": [tool],
                        "store": False,
                        "reasoning": {"effort": "none"},
                    },
                )
                self.assertEqual(response.status_code, 200)
                self.assertEqual(response.json()["output"][0]["type"], expected)
                if tool["type"] == "namespace":
                    call = response.json()["output"][0]
                    self.assertEqual(call["namespace"], "repo_tools")
                    self.assertEqual(call["name"], "status")

    def test_codex_metadata_optional_web_and_models_envelope(self) -> None:
        response = self.post(
            "/v1/responses",
            {
                "input": "Reply without tools. Do not call tools.",
                "client_metadata": {"thread_id": "fixture-thread"},
                "tools": [{"type": "web_search", "external_web_access": False}],
                "tool_choice": "auto",
                "store": False,
                "reasoning": {"effort": "none"},
            },
        )
        self.assertEqual(response.status_code, 200)
        self.assertEqual(response.json()["output"][0]["type"], "message")
        forced = self.post(
            "/v1/responses",
            {
                "input": "search",
                "tools": [{"type": "web_search"}],
                "tool_choice": {"type": "web_search"},
            },
        )
        self.assertEqual(forced.status_code, 400)
        self.assertEqual(forced.json()["error"]["code"], "hosted_web_search_disabled")
        models = self.client.get("/v1/models", headers=self.headers).json()
        self.assertEqual(models["models"][0]["slug"], models["data"][0]["id"])
        self.assertFalse(models["models"][0]["supports_search_tool"])

    def test_hosted_mcp_and_modalities_fail_explicitly(self) -> None:
        hosted = self.post(
            "/v1/responses",
            {
                "input": "use mcp",
                "tools": [{"type": "mcp", "server_label": "remote", "server_url": "https://example.com/mcp"}],
            },
        )
        self.assertEqual(hosted.status_code, 400)
        self.assertEqual(hosted.json()["error"]["code"], "hosted_mcp_disabled")

    def test_strict_structured_output(self) -> None:
        response = self.post(
            "/v1/responses",
            {
                "input": "return a result",
                "store": False,
                "text": {
                    "format": {
                        "type": "json_schema",
                        "name": "result",
                        "strict": True,
                        "schema": {
                            "type": "object",
                            "properties": {"answer": {"type": "string"}},
                            "required": ["answer"],
                            "additionalProperties": False,
                        },
                    }
                },
            },
        )
        self.assertEqual(response.status_code, 200)
        text = response.json()["output"][0]["content"][0]["text"]
        self.assertEqual(json.loads(text), {"answer": "example"})

    def test_persistence_pagination_token_count_and_delete(self) -> None:
        created = self.post("/v1/responses", {"input": "persist me", "store": True}).json()
        fetched = self.client.get(f"/v1/responses/{created['id']}", headers=self.headers)
        self.assertEqual(fetched.json(), created)
        items = self.client.get(
            f"/v1/responses/{created['id']}/input_items?limit=1&order=asc",
            headers=self.headers,
        ).json()
        self.assertEqual(items["object"], "list")
        self.assertEqual(items["data"][0]["role"], "user")
        count = self.post("/v1/responses/input_tokens", {"input": "count these tokens"}).json()
        self.assertGreater(count["input_tokens"], 0)
        deleted = self.client.delete(f"/v1/responses/{created['id']}", headers=self.headers)
        self.assertTrue(deleted.json()["deleted"])
        self.assertEqual(
            self.client.get(f"/v1/responses/{created['id']}", headers=self.headers).status_code,
            404,
        )

    def test_store_false_is_not_retrievable(self) -> None:
        created = self.post("/v1/responses", {"input": "ephemeral", "store": False}).json()
        response = self.client.get(f"/v1/responses/{created['id']}", headers=self.headers)
        self.assertEqual(response.status_code, 404)

    def test_compaction_round_trip(self) -> None:
        compacted = self.post(
            "/v1/responses/compact",
            {
                "model": "poolside/Laguna-XS-2.1-NVFP4-mlx",
                "input": [
                    {"type": "message", "role": "user", "content": "Remember cobalt."},
                    {"type": "message", "role": "assistant", "content": "OK."},
                ],
            },
        ).json()
        self.assertEqual(compacted["object"], "response.compaction")
        encrypted = compacted["output"][0]["encrypted_content"]
        self.assertNotIn("cobalt", encrypted)
        continued = self.post(
            "/v1/responses",
            {
                "input": compacted["output"]
                + [{"type": "message", "role": "user", "content": "Continue."}],
                "store": False,
            },
        )
        self.assertEqual(continued.status_code, 200)
        self.assertEqual(continued.json()["status"], "completed")

    def test_unknown_fields_and_unmatched_output_are_rejected(self) -> None:
        unknown = self.post("/v1/responses", {"input": "x", "surprise": True})
        self.assertEqual(unknown.status_code, 400)
        unmatched = self.post(
            "/v1/responses",
            {
                "input": [
                    {"type": "function_call_output", "call_id": "call_missing", "output": "x"}
                ]
            },
        )
        self.assertEqual(unmatched.status_code, 400)
        self.assertEqual(unmatched.json()["error"]["code"], "tool_output_without_call")

    def test_concurrent_requests_and_background_cancellation(self) -> None:
        def invoke(index: int) -> int:
            response = self.post(
                "/v1/responses", {"input": f"parallel {index}", "store": True}
            )
            return response.status_code

        with ThreadPoolExecutor(max_workers=8) as executor:
            self.assertEqual(list(executor.map(invoke, range(16))), [200] * 16)
        queued = self.post(
            "/v1/responses",
            {
                "input": "slow background response",
                "background": True,
                "x_synth": {"fake_delay_ms": 100},
            },
        ).json()
        cancelled = self.post(f"/v1/responses/{queued['id']}/cancel", {}).json()
        self.assertIn(cancelled["status"], {"cancelled", "completed"})
        again = self.post(f"/v1/responses/{queued['id']}/cancel", {})
        self.assertEqual(again.status_code, 200)


class NativeResponsesWebSocketTests(unittest.TestCase):
    def test_sequential_continuation_recovery_and_eviction(self) -> None:
        with tempfile.TemporaryDirectory(prefix="laguna-websocket-") as temp:
            with TestClient(build_app(config(Path(temp)))) as client:
                headers = {"Authorization": "Bearer test-key"}
                with client.websocket_connect("/v1/responses", headers=headers) as socket:
                    def turn(body: dict[str, Any]) -> list[dict[str, Any]]:
                        socket.send_json(body)
                        result = []
                        while True:
                            event = socket.receive_json()
                            result.append(event)
                            if event["type"] in {
                                "response.completed",
                                "response.failed",
                                "response.incomplete",
                                "error",
                            }:
                                return result

                    first = turn(
                        {
                            "type": "response.create",
                            "model": "poolside/Laguna-XS-2.1-NVFP4-mlx",
                            "store": False,
                            "input": "remember cobalt",
                        }
                    )
                    first_id = first[-1]["response"]["id"]
                    second = turn(
                        {
                            "type": "response.create",
                            "model": "poolside/Laguna-XS-2.1-NVFP4-mlx",
                            "store": False,
                            "previous_response_id": first_id,
                            "input": "continue",
                        }
                    )
                    self.assertEqual(second[-1]["type"], "response.completed")
                    failed = turn(
                        {
                            "type": "response.create",
                            "model": "poolside/Laguna-XS-2.1-NVFP4-mlx",
                            "store": False,
                            "previous_response_id": first_id,
                            "input": [
                                {
                                    "type": "function_call_output",
                                    "call_id": "call_missing",
                                    "output": "bad",
                                }
                            ],
                        }
                    )
                    self.assertEqual(failed[-1]["type"], "error")
                    retry = turn(
                        {
                            "type": "response.create",
                            "model": "poolside/Laguna-XS-2.1-NVFP4-mlx",
                            "store": False,
                            "previous_response_id": first_id,
                            "input": "stale",
                        }
                    )
                    self.assertEqual(retry[-1]["error"]["code"], "previous_response_not_found")


class NativeBackendContractTests(unittest.TestCase):
    def test_native_backend_rejects_insufficient_memory_before_loading(self) -> None:
        with tempfile.TemporaryDirectory(prefix="laguna-native-low-memory-") as tmp:
            backend = NativeMlxBackend(
                model_path=Path(tmp),
                system_memory_bytes=16 * 1024**3,
            )
            with self.assertRaises(ResponsesError) as raised:
                asyncio.run(backend._ensure_loaded())
            self.assertEqual(raised.exception.code, "insufficient_system_memory")
            self.assertEqual(raised.exception.status_code, 503)
            self.assertIn("16.0 GiB", raised.exception.message)
            self.assertFalse(backend.residency(900)["loaded"])
            asyncio.run(backend.close())

    def test_custom_grammar_activation_decodes_only_a_bounded_suffix(self) -> None:
        class Tokenizer:
            def __init__(self) -> None:
                self.decoded_lengths: list[int] = []

            def decode(self, ids: list[int], **_: object) -> str:
                self.decoded_lengths.append(len(ids))
                return "unrelated generated text"

        processor = object.__new__(_ActivatedCustomGrammarProcessor)
        processor._tokenizer = Tokenizer()
        processor._choices = [("<tool_call>visual", object())]
        processor._activation_window_tokens = 40
        processor._active = None
        processor._active_completed = False

        logits = object()
        ids = type(
            "Ids", (), {"ndim": 1, "tolist": lambda self: list(range(12_000))}
        )()
        returned = processor(ids, logits)

        self.assertIs(returned, logits)
        self.assertEqual(processor._tokenizer.decoded_lengths, [40])

    def test_turn_state_machine_emits_envelopes_and_keeps_parallel_calls(self) -> None:
        machine = _TurnStateMachine(thinking_open=False, tools=True)
        events = machine.feed(
            "Let me check. <tool_call>one<arg_key>path<arg_value>.</arg_value>"
            "</arg_key></tool_call>"
        )
        # Pre-call prose streams as answer text; the envelope arrives whole.
        self.assertIn(("answer", "Let me check. "), events)
        closes = [event for event in events if event[0] == "tool_call"]
        self.assertEqual(len(closes), 1)
        # A partial second opening marker is held back, not misclassified.
        events = machine.feed("<tool_ca")
        self.assertEqual(events, [])
        self.assertTrue(machine.maybe_in_tool_call)
        events = machine.feed("ll>two</tool_call>")
        self.assertEqual([event[0] for event in events], ["tool_call"])

    def test_arguments_are_typed_by_the_declared_schema(self) -> None:
        """String-typed args stay verbatim text; only non-strings JSON-decode.

        The chat template renders string values raw and non-strings as JSON,
        so a schema-blind json.loads would turn a path spelled "123" into an
        integer and strip meaning from JSON-looking file content.
        """
        binding = ToolBinding(
            model_name="write_file",
            original_name="write_file",
            kind="function",
            schema={
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "contents": {"type": "string"},
                    "mode": {"type": "integer"},
                },
            },
        )
        event = _envelope_event(
            "write_file"
            "<arg_key>path</arg_key><arg_value>123</arg_value>"
            "<arg_key>contents</arg_key><arg_value>{\"not\": \"decoded\"}\n</arg_value>"
            "<arg_key>mode</arg_key><arg_value>644</arg_value>",
            {"write_file": binding},
        )
        arguments = json.loads(event.arguments)
        self.assertEqual(arguments["path"], "123")
        self.assertEqual(arguments["contents"], '{"not": "decoded"}\n')
        self.assertEqual(arguments["mode"], 644)

    def test_turn_state_machine_discards_a_truncated_envelope(self) -> None:
        machine = _TurnStateMachine(thinking_open=False, tools=True)
        machine.feed("<tool_call>write<arg_key>path</arg_key><arg_value>half of a")
        leaked = machine.flush()
        self.assertEqual(leaked, [])
        self.assertTrue(machine.truncated_tool_call)

    def test_turn_state_machine_reenters_an_interleaved_thinking_span(self) -> None:
        machine = _TurnStateMachine(thinking_open=True, tools=True)
        events = machine.feed("plan a</think>Answer. <think>plan b</think> More.")
        kinds = [kind for kind, _ in events]
        self.assertEqual(kinds, ["reasoning", "answer", "reasoning", "answer"])
        self.assertEqual(events[2], ("reasoning", "plan b"))

    def test_turn_state_machine_reasoning_may_open_a_tool_call_directly(self) -> None:
        """Models sometimes skip </think> straight into an envelope."""
        machine = _TurnStateMachine(thinking_open=True, tools=True)
        events = machine.feed(
            "I should list files.<tool_call>ls<arg_key>path</arg_key>"
            "<arg_value>.</arg_value></tool_call>"
        )
        self.assertEqual(events[0], ("reasoning", "I should list files."))
        self.assertEqual(events[1][0], "tool_call")

    def test_native_backend_releases_idle_weights_without_stopping_the_daemon(self) -> None:
        with tempfile.TemporaryDirectory(prefix="laguna-native-idle-") as tmp:
            backend = NativeMlxBackend(model_path=Path(tmp))
            backend._model = object()
            backend._tokenizer = object()
            backend._last_used_at = time.time() - 31

            self.assertTrue(asyncio.run(backend.unload_if_idle(30)))
            self.assertIsNone(backend._model)
            self.assertIsNone(backend._tokenizer)
            self.assertFalse(backend.residency(30)["loaded"])
            asyncio.run(backend.close())

    def test_coordinator_closes_backend_stream_when_event_sink_fails(self) -> None:
        class LeaseBackend(FakeBackend):
            def __init__(self) -> None:
                super().__init__()
                self.closed = False

            async def stream(self, turn):
                try:
                    yield ModelEvent(kind="text_delta", delta="hello")
                    yield ModelEvent(kind="finish", finish_reason="stop")
                finally:
                    self.closed = True

        async def exercise() -> tuple[bool, dict[str, Any]]:
            with tempfile.TemporaryDirectory(prefix="laguna-stream-lease-") as temp:
                backend = LeaseBackend()
                service = ResponsesService(config(Path(temp)), backend=backend)
                request = service.normalize({"input": "hello", "store": False})

                async def sink(event: dict[str, Any]) -> None:
                    if event["type"] == "response.output_text.delta":
                        raise ResponsesError(
                            "sink_failed",
                            "The downstream event consumer failed.",
                            500,
                            error_type="server_error",
                        )

                response = await service.coordinator.run(request, sink=sink)
                return backend.closed, response

        closed, response = asyncio.run(exercise())
        self.assertTrue(closed)
        self.assertEqual(response["status"], "failed")
        self.assertEqual(response["error"]["code"], "sink_failed")

    def test_missing_opening_think_marker_never_leaks_reasoning(self) -> None:
        reasoning, answer = _split_reasoning("private analysis</think>public answer")
        self.assertEqual(reasoning, "private analysis")
        self.assertEqual(answer, "public answer")

    def test_sanitized_real_codex_fixture_keeps_extension_shapes(self) -> None:
        fixture = Path(__file__).parent / "fixtures" / "codex" / "codex-request-http-0001.json"
        request = json.loads(fixture.read_text(encoding="utf-8"))["request"]
        self.assertTrue(all(value == "<redacted>" for value in request["client_metadata"].values()))
        tools = request["tools"]
        apply_patch = next(tool for tool in tools if tool.get("name") == "apply_patch")
        self.assertEqual(apply_patch["type"], "custom")
        self.assertEqual(apply_patch["format"]["type"], "grammar")
        self.assertEqual(apply_patch["format"]["syntax"], "lark")
        self.assertGreaterEqual(sum(tool.get("type") == "namespace" for tool in tools), 9)

    def test_mlx_tool_rehydration_preserves_custom_kind(self) -> None:
        binding = ToolBinding(
            model_name="mcp__synth_containers",
            original_name="mcp__synth_containers",
            kind="custom",
        )
        events, remainder = _rehydrate_tool_calls(
            '<tool_call>mcp__synth_containers<arg_key>input</arg_key><arg_value>{"method":"container_list"}</arg_value></tool_call>',
            {binding.model_name: binding},
        )
        self.assertEqual(remainder, "")
        self.assertEqual(events[0].kind, "custom_tool_call")
        self.assertEqual(events[0].input, '{"method":"container_list"}')

    def test_mlx_tool_rehydration_suppresses_terminal_prose(self) -> None:
        binding = ToolBinding(
            model_name="container_list",
            original_name="container_list",
            namespace="mcp__synth_containers",
            kind="function",
            schema={"type": "object", "properties": {}},
        )
        events, remainder = _rehydrate_tool_calls(
            "<tool_call>container_list</tool_call>I'll start by discovering the service.",
            {binding.model_name: binding},
        )
        self.assertEqual(events[0].kind, "function_call")
        self.assertEqual(events[0].namespace, "mcp__synth_containers")
        self.assertEqual(remainder, "")

    def test_mlx_rehydrates_evidenced_read_alias_as_exec_command(self) -> None:
        binding = ToolBinding(
            model_name="exec_command",
            original_name="exec_command",
            kind="function",
            schema={
                "type": "object",
                "properties": {"cmd": {"type": "string"}},
                "required": ["cmd"],
            },
        )
        for alias in ("read", "read_file"):
            with self.subTest(alias=alias):
                events, remainder = _rehydrate_tool_calls(
                    f"<tool_call>{alias}<arg_key>path</arg_key>"
                    "<arg_value>folder/AGENTS.md</arg_value></tool_call>",
                    {binding.model_name: binding},
                )
                self.assertEqual(events[0].kind, "function_call")
                self.assertEqual(events[0].name, "exec_command")
                self.assertEqual(
                    json.loads(events[0].arguments or "{}"),
                    {"cmd": "sed -n '1,240p' folder/AGENTS.md"},
                )
                self.assertEqual(remainder, "")

    def test_mlx_rehydrates_evidenced_grep_alias_as_exec_command(self) -> None:
        binding = ToolBinding(
            model_name="exec_command",
            original_name="exec_command",
            kind="function",
            schema={
                "type": "object",
                "properties": {"cmd": {"type": "string"}},
                "required": ["cmd"],
            },
        )
        events, remainder = _rehydrate_tool_calls(
            "<tool_call>grep"
            "<arg_key>pattern</arg_key><arg_value>needle</arg_value>"
            "<arg_key>path</arg_key><arg_value>folder</arg_value>"
            "<arg_key>output_mode</arg_key><arg_value>content</arg_value>"
            "</tool_call>",
            {binding.model_name: binding},
        )
        self.assertEqual(events[0].name, "exec_command")
        self.assertEqual(
            json.loads(events[0].arguments or "{}"),
            {"cmd": "rg -n -- needle folder"},
        )
        self.assertEqual(remainder, "")

    def test_mlx_rehydrates_evidenced_write_alias_as_exec_command(self) -> None:
        binding = ToolBinding(
            model_name="exec_command",
            original_name="exec_command",
            kind="function",
            schema={
                "type": "object",
                "properties": {"cmd": {"type": "string"}},
                "required": ["cmd"],
            },
        )
        for content_key in ("input", "contents"):
            with self.subTest(content_key=content_key):
                events, remainder = _rehydrate_tool_calls(
                    "<tool_call>write"
                    "<arg_key>path</arg_key><arg_value>folder/file.txt</arg_value>"
                    f"<arg_key>{content_key}</arg_key><arg_value>hello\n</arg_value>"
                    "</tool_call>",
                    {binding.model_name: binding},
                )
                self.assertEqual(events[0].name, "exec_command")
                self.assertEqual(
                    json.loads(events[0].arguments or "{}"),
                    {
                        "cmd": "mkdir -p folder && printf %s aGVsbG8K | "
                        "base64 -D > folder/file.txt"
                    },
                )
                self.assertEqual(remainder, "")

    def test_id_families_are_distinct(self) -> None:
        self.assertTrue(new_id("response").startswith("resp_"))
        self.assertTrue(new_id("custom_tool_call").startswith("ctc_"))
        self.assertTrue(new_id("call").startswith("call_"))

    def test_native_backend_does_not_reference_chat_completions(self) -> None:
        root = Path(__file__).parents[1] / "laguna_daemon" / "responses_api"
        source = "\n".join(path.read_text(encoding="utf-8") for path in root.rglob("*.py"))
        self.assertNotIn("/v1/chat/completions", source)


if __name__ == "__main__":
    unittest.main()
