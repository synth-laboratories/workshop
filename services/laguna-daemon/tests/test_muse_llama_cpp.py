"""Muse Glimmer as a first-class local model on both wire surfaces.

Everything here runs against a fake llama.cpp engine built on httpx's mock
transport: no weights, no engine process, no network. The engine is scripted
to answer exactly as `llama-server` does — SSE chat chunks, `/health` while
loading, `/apply-template` plus `/tokenize` for counts — so the assertions are
about Laguna's mapping rather than about a mock's convenience.

The live counterpart is `scripts/muse/serve.sh` plus a real Codex turn; see
apps/synth_desktop/muse_sidecar.md for the gates that need real weights.
"""

from __future__ import annotations

import ast
import asyncio
import json
import tempfile
import time
import unittest
from pathlib import Path
from typing import Any, Callable

import httpx
from fastapi.testclient import TestClient

from laguna_daemon.app import build_app
from laguna_daemon.config import (
    MUSE_CONTEXT_LENGTH,
    MUSE_GLIMMER_LEGACY_MODEL,
    MUSE_GLIMMER_MODEL,
    LagunaConfig,
)
from laguna_daemon.responses_api.backends.llama_cpp import LlamaCppChatBackend
from laguna_daemon.responses_api.backends.remote_responses import RemoteResponsesBackend
from laguna_daemon.responses_api.compiler import compile_messages
from laguna_daemon.responses_api.errors import ResponsesError
from laguna_daemon.responses_api.service import ResponsesService
from laguna_daemon.responses_api.telemetry import GenerationTiming


ENGINE_URL = "http://127.0.0.1:9999"


def _chunk(**delta: Any) -> str:
    payload = {
        "id": "chatcmpl-1",
        "object": "chat.completion.chunk",
        "model": MUSE_GLIMMER_MODEL,
        "choices": [{"index": 0, "delta": delta, "finish_reason": None}],
    }
    return f"data: {json.dumps(payload)}\n\n"


def _final(finish_reason: str = "stop", *, prompt: int = 11, completion: int = 5) -> str:
    stop = {
        "id": "chatcmpl-1",
        "object": "chat.completion.chunk",
        "model": MUSE_GLIMMER_MODEL,
        "choices": [{"index": 0, "delta": {}, "finish_reason": finish_reason}],
    }
    usage = {
        "id": "chatcmpl-1",
        "object": "chat.completion.chunk",
        "model": MUSE_GLIMMER_MODEL,
        "choices": [],
        "usage": {
            "prompt_tokens": prompt,
            "completion_tokens": completion,
            "total_tokens": prompt + completion,
            "prompt_tokens_details": {"cached_tokens": 3},
        },
    }
    return (
        f"data: {json.dumps(stop)}\n\n"
        f"data: {json.dumps(usage)}\n\n"
        "data: [DONE]\n\n"
    )


class FakeEngine:
    """A scripted llama.cpp server, plus a record of what Laguna sent it."""

    def __init__(
        self,
        *,
        body: str = "",
        health_status: int = 200,
        chat_status: int = 200,
        chat_error: dict[str, Any] | None = None,
        tokens: int = 11,
        template: bool = True,
        slots: list[dict[str, Any]] | None = None,
    ) -> None:
        self.body = body
        self.health_status = health_status
        self.chat_status = chat_status
        self.chat_error = chat_error
        self.tokens = tokens
        self.template = template
        self.slots = slots or []
        self.requests: list[tuple[str, dict[str, Any] | None]] = []
        self.chat_calls = 0
        self.health_calls = 0

    def transport(self) -> httpx.MockTransport:
        return httpx.MockTransport(self._handle)

    def _handle(self, request: httpx.Request) -> httpx.Response:
        path = request.url.path
        payload = json.loads(request.content) if request.content else None
        self.requests.append((path, payload))
        if path == "/health":
            self.health_calls += 1
            if self.health_status != 200:
                return httpx.Response(
                    self.health_status, json={"status": "loading model"}
                )
            return httpx.Response(200, json={"status": "ok"})
        if path == "/apply-template":
            if not self.template:
                return httpx.Response(404, json={"error": "not found"})
            return httpx.Response(200, json={"prompt": "<rendered prompt>"})
        if path == "/tokenize":
            return httpx.Response(200, json={"tokens": list(range(self.tokens))})
        if path == "/slots":
            return httpx.Response(200, json=self.slots)
        if path == "/v1/chat/completions":
            self.chat_calls += 1
            if self.chat_error is not None:
                return httpx.Response(self.chat_status, json={"error": self.chat_error})
            return httpx.Response(
                200,
                content=self.body.encode(),
                headers={"Content-Type": "text/event-stream"},
            )
        return httpx.Response(404, json={"error": "unknown route"})


def backend_for(engine: FakeEngine, **kwargs: Any) -> LlamaCppChatBackend:
    return LlamaCppChatBackend(
        engine_url=ENGINE_URL,
        model=MUSE_GLIMMER_MODEL,
        context_length=MUSE_CONTEXT_LENGTH,
        transport=engine.transport(),
        **kwargs,
    )


def turn(
    *,
    tools: list[dict[str, Any]] | None = None,
    content: str = "hello",
    enable_thinking: bool = True,
) -> Any:
    return compile_messages(
        [{"role": "user", "content": content}],
        model=MUSE_GLIMMER_MODEL,
        generation_id="gen_test",
        tools=tools or [],
        enable_thinking=enable_thinking,
    )


async def collect(backend: LlamaCppChatBackend, compiled: Any) -> list[Any]:
    return [event async for event in backend.stream(compiled)]


def run(coroutine: Any) -> Any:
    return asyncio.run(coroutine)


class ChatStreamMappingTests(unittest.TestCase):
    """Engine SSE chunks become `ModelEvent`s, and nothing else."""

    def test_text_reasoning_and_usage_map_to_model_events(self) -> None:
        engine = FakeEngine(
            body=(
                _chunk(reasoning_content="weighing options")
                + _chunk(content="Hello")
                + _chunk(content=" there")
                + _final()
            )
        )
        events = run(collect(backend_for(engine), turn()))
        kinds = [event.kind for event in events]
        self.assertEqual(
            kinds, ["reasoning_delta", "text_delta", "text_delta", "usage", "finish"]
        )
        self.assertEqual(events[0].delta, "weighing options")
        self.assertEqual("".join(event.delta for event in events[1:3]), "Hello there")
        usage = events[-2]
        self.assertEqual(usage.input_tokens, 11)
        self.assertEqual(usage.output_tokens, 5)
        self.assertEqual(usage.metadata["cached_tokens"], 3)
        self.assertEqual(events[-1].finish_reason, "stop")

    def test_inline_think_span_never_reaches_assistant_text(self) -> None:
        """A template that keeps thinking in `content` must still be split."""
        engine = FakeEngine(
            body=(
                _chunk(content="<think>private plan</think>")
                + _chunk(content="public answer")
                + _final()
            )
        )
        events = run(collect(backend_for(engine), turn()))
        reasoning = "".join(e.delta for e in events if e.kind == "reasoning_delta")
        text = "".join(e.delta for e in events if e.kind == "text_delta")
        self.assertEqual(reasoning, "private plan")
        self.assertEqual(text, "public answer")
        self.assertNotIn("<think>", text)

    def test_split_marker_across_chunks_is_not_leaked(self) -> None:
        engine = FakeEngine(
            body=(
                _chunk(content="<think>secret</th")
                + _chunk(content="ink>answer")
                + _final()
            )
        )
        events = run(collect(backend_for(engine), turn()))
        text = "".join(e.delta for e in events if e.kind == "text_delta")
        self.assertEqual(text, "answer")
        self.assertEqual(
            "".join(e.delta for e in events if e.kind == "reasoning_delta"), "secret"
        )

    def test_usage_is_zero_rather_than_invented_when_engine_omits_it(self) -> None:
        engine = FakeEngine(
            body=_chunk(content="hi")
            + 'data: {"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}\n\n'
            + "data: [DONE]\n\n"
        )
        events = run(collect(backend_for(engine), turn()))
        usage = next(event for event in events if event.kind == "usage")
        self.assertEqual(usage.input_tokens, 0)
        self.assertEqual(usage.output_tokens, 0)

    def test_request_carries_sampling_tools_and_thinking(self) -> None:
        engine = FakeEngine(body=_chunk(content="ok") + _final())
        tools = [
            {
                "type": "function",
                "name": "exec_command",
                "description": "run",
                "parameters": {
                    "type": "object",
                    "properties": {"cmd": {"type": "string"}},
                },
            }
        ]
        run(collect(backend_for(engine), turn(tools=tools, enable_thinking=False)))
        body = next(
            payload for path, payload in engine.requests if path == "/v1/chat/completions"
        )
        self.assertTrue(body["stream"])
        self.assertTrue(body["stream_options"]["include_usage"])
        self.assertEqual(body["model"], MUSE_GLIMMER_MODEL)
        self.assertEqual(body["chat_template_kwargs"], {"enable_thinking": False})
        self.assertEqual(body["tools"][0]["function"]["name"], "exec_command")
        self.assertEqual(body["messages"][-1]["role"], "user")


class ToolCallTests(unittest.TestCase):
    """Bindings decide what a call becomes; the engine only supplies names."""

    def _tools(self, kind: str) -> list[dict[str, Any]]:
        if kind == "custom":
            return [{"type": "custom", "name": "patch", "format": {"type": "text"}}]
        return [
            {
                "type": kind,
                "name": "exec_command" if kind == "function" else kind,
                "parameters": {
                    "type": "object",
                    "properties": {"cmd": {"type": "string"}},
                },
            }
        ]

    def test_streamed_tool_call_deltas_become_one_typed_call(self) -> None:
        engine = FakeEngine(
            body=(
                _chunk(
                    tool_calls=[
                        {
                            "index": 0,
                            "id": "call_a",
                            "function": {"name": "exec_command", "arguments": '{"cmd"'},
                        }
                    ]
                )
                + _chunk(
                    tool_calls=[{"index": 0, "function": {"arguments": ': "ls"}'}}]
                )
                + _final(finish_reason="tool_calls")
            )
        )
        compiled = turn(tools=self._tools("function"))
        events = run(collect(backend_for(engine), compiled))
        calls = [event for event in events if event.kind == "function_call"]
        self.assertEqual(len(calls), 1)
        self.assertEqual(calls[0].name, "exec_command")
        self.assertEqual(json.loads(calls[0].arguments or "{}"), {"cmd": "ls"})
        self.assertEqual(events[-1].finish_reason, "tool_call")

    def test_shell_binding_returns_a_shell_call_not_a_function_call(self) -> None:
        engine = FakeEngine(
            body=_chunk(
                tool_calls=[
                    {
                        "index": 0,
                        "function": {
                            "name": "shell",
                            "arguments": '{"command": "pwd"}',
                        },
                    }
                ]
            )
            + _final(finish_reason="tool_calls")
        )
        compiled = turn(tools=self._tools("shell"))
        events = run(collect(backend_for(engine), compiled))
        self.assertEqual(
            [event.kind for event in events if event.kind.endswith("_call")],
            ["shell_call"],
        )

    def test_custom_tool_input_survives_as_a_raw_string(self) -> None:
        engine = FakeEngine(
            body=_chunk(
                tool_calls=[
                    {
                        "index": 0,
                        "function": {
                            "name": "patch",
                            "arguments": json.dumps({"input": "*** Begin Patch"}),
                        },
                    }
                ]
            )
            + _final(finish_reason="tool_calls")
        )
        compiled = turn(tools=self._tools("custom"))
        events = run(collect(backend_for(engine), compiled))
        call = next(event for event in events if event.kind == "custom_tool_call")
        self.assertEqual(call.input, "*** Begin Patch")
        self.assertEqual(call.name, "patch")

    def test_unknown_tool_name_fails_closed(self) -> None:
        engine = FakeEngine(
            body=_chunk(
                tool_calls=[
                    {"index": 0, "function": {"name": "rm_rf", "arguments": "{}"}}
                ]
            )
            + _final(finish_reason="tool_calls")
        )
        compiled = turn(tools=self._tools("function"))
        with self.assertRaises(ResponsesError) as raised:
            run(collect(backend_for(engine), compiled))
        self.assertEqual(raised.exception.code, "unknown_tool_call")

    def test_unparseable_arguments_fail_closed(self) -> None:
        engine = FakeEngine(
            body=_chunk(
                tool_calls=[
                    {
                        "index": 0,
                        "function": {"name": "exec_command", "arguments": "{not json"},
                    }
                ]
            )
            + _final(finish_reason="tool_calls")
        )
        compiled = turn(tools=self._tools("function"))
        with self.assertRaises(ResponsesError) as raised:
            run(collect(backend_for(engine), compiled))
        self.assertEqual(raised.exception.code, "invalid_tool_arguments")


class EngineLifecycleTests(unittest.TestCase):
    def test_unreachable_engine_is_a_typed_503_not_a_transport_error(self) -> None:
        def refuse(request: httpx.Request) -> httpx.Response:
            raise httpx.ConnectError("connection refused", request=request)

        backend = LlamaCppChatBackend(
            engine_url=ENGINE_URL,
            model=MUSE_GLIMMER_MODEL,
            context_length=MUSE_CONTEXT_LENGTH,
            transport=httpx.MockTransport(refuse),
        )
        with self.assertRaises(ResponsesError) as raised:
            run(collect(backend, turn()))
        self.assertEqual(raised.exception.code, "muse_engine_unavailable")
        self.assertEqual(raised.exception.status_code, 503)
        self.assertIn(ENGINE_URL, raised.exception.message)

    def test_loading_engine_is_waited_on_then_served(self) -> None:
        engine = FakeEngine(body=_chunk(content="ok") + _final(), health_status=503)

        async def scenario() -> list[Any]:
            backend = backend_for(engine, ready_timeout_seconds=5.0)

            async def flip() -> None:
                await asyncio.sleep(0.05)
                engine.health_status = 200

            asyncio.create_task(flip())
            events = await collect(backend, turn())
            self.assertTrue(backend.diagnostics()["loaded"])
            return events

        events = run(scenario())
        self.assertIn("text_delta", [event.kind for event in events])
        self.assertGreater(engine.health_calls, 1)

    def test_ready_timeout_reports_loading_as_the_cause(self) -> None:
        engine = FakeEngine(health_status=503)
        backend = backend_for(engine, ready_timeout_seconds=0.2)
        with self.assertRaises(ResponsesError) as raised:
            run(collect(backend, turn()))
        self.assertEqual(raised.exception.code, "muse_engine_unavailable")
        self.assertIn("mapping", raised.exception.message)

    def test_engine_without_chat_surface_names_the_real_problem(self) -> None:
        engine = FakeEngine(chat_status=404, chat_error={"message": "nope"})
        with self.assertRaises(ResponsesError) as raised:
            run(collect(backend_for(engine), turn()))
        self.assertEqual(raised.exception.code, "muse_engine_unavailable")
        self.assertIn("Chat Completions", raised.exception.message)

    def test_cancel_stops_the_stream_and_reports_it(self) -> None:
        engine = FakeEngine(
            body="".join(_chunk(content=f"token{index} ") for index in range(50))
            + _final()
        )

        async def scenario() -> list[Any]:
            backend = backend_for(engine)
            compiled = turn()
            events: list[Any] = []
            async for event in backend.stream(compiled):
                events.append(event)
                if len(events) == 2:
                    await backend.cancel(compiled.generation_id)
            return events

        events = run(scenario())
        self.assertEqual(events[-1].kind, "finish")
        self.assertEqual(events[-1].finish_reason, "cancelled")
        self.assertLess(len(events), 50)

    def test_cancel_closes_the_connection_not_only_the_response(self) -> None:
        """The regression guard for an orphaned GPU.

        Closing the response alone returns its socket to httpx's pool still
        open. llama.cpp stops only when the client it is writing to goes away,
        so a "cancelled" turn kept a full engine slot decoding to its token
        limit — measured against real weights before this was fixed. Every
        connection a generation opened must be closed by the time it retires.
        """
        engine = FakeEngine(
            body="".join(_chunk(content=f"{index} ") for index in range(200)) + _final()
        )

        async def scenario() -> None:
            backend = backend_for(engine)
            opened: list[httpx.AsyncClient] = []
            make_client = backend._new_client

            def spy() -> httpx.AsyncClient:
                client = make_client()
                opened.append(client)
                return client

            backend._new_client = spy  # type: ignore[method-assign]
            compiled = turn()
            events = 0
            async for _ in backend.stream(compiled):
                events += 1
                if events == 3:
                    await backend.cancel(compiled.generation_id)
            # The pooled client for health/tokenize calls lives on by design;
            # every connection a *generation* opened must be gone.
            generation_clients = [c for c in opened if c is not backend._client]
            self.assertTrue(generation_clients, "the generation opened no connection")
            self.assertTrue(
                all(client.is_closed for client in generation_clients),
                "an engine connection outlived the generation that opened it",
            )
            self.assertFalse(backend._open_streams)

        run(scenario())

    def test_a_completed_turn_also_closes_its_connection(self) -> None:
        engine = FakeEngine(body=_chunk(content="done") + _final())

        async def scenario() -> None:
            backend = backend_for(engine)
            opened: list[httpx.AsyncClient] = []
            make_client = backend._new_client
            def spy() -> httpx.AsyncClient:
                client = make_client()
                opened.append(client)
                return client

            backend._new_client = spy  # type: ignore[method-assign]
            await collect(backend, turn())
            generation_clients = [c for c in opened if c is not backend._client]
            self.assertTrue(generation_clients)
            self.assertTrue(all(client.is_closed for client in generation_clients))

        run(scenario())

    def test_queue_saturation_is_the_same_typed_error_as_mlx(self) -> None:
        engine = FakeEngine(body=_chunk(content="ok") + _final())

        async def scenario() -> None:
            backend = backend_for(engine)
            backend._max_inflight_generations = 1
            first = backend.stream(turn())
            await anext(first)
            with self.assertRaises(ResponsesError) as raised:
                await anext(backend.stream(turn()))
            self.assertEqual(raised.exception.code, "model_queue_saturated")
            self.assertEqual(raised.exception.status_code, 429)
            await first.aclose()

        run(scenario())

    def test_admission_slot_reopens_when_a_consumer_abandons_the_stream(self) -> None:
        engine = FakeEngine(
            body="".join(_chunk(content="x") for _ in range(20)) + _final()
        )

        async def scenario() -> None:
            backend = backend_for(engine)
            stream = backend.stream(turn())
            await anext(stream)
            await stream.aclose()
            self.assertEqual(backend.queue_state()["depth"], 0)
            self.assertTrue(backend.diagnostics()["generation_slot_available"])
            # The slot is genuinely free: a second turn runs to completion.
            events = await collect(backend, turn())
            self.assertEqual(events[-1].kind, "finish")

        run(scenario())

    def test_token_counts_come_from_the_engine_tokenizer(self) -> None:
        engine = FakeEngine(body=_chunk(content="ok") + _final(), tokens=42)
        backend = backend_for(engine)
        estimate = run(backend.count_tokens(turn()))
        self.assertEqual(estimate.input_tokens, 42)
        self.assertIn("/apply-template", [path for path, _ in engine.requests])

    def test_live_slot_metrics_report_prefill_cache_and_decode_counts(self) -> None:
        engine = FakeEngine(
            slots=[
                {
                    "is_processing": True,
                    "n_prompt_tokens": 13271,
                    "n_prompt_tokens_processed": 8192,
                    "n_prompt_tokens_cache": 2048,
                    "next_token": [{"n_decoded": 3}],
                }
            ]
        )

        async def scenario() -> GenerationTiming:
            backend = backend_for(engine)
            timing = GenerationTiming(
                generation_id="gen_metrics",
                queued_at=time.monotonic(),
                admitted_at=time.monotonic(),
                phase="prefill",
            )
            stop = asyncio.Event()
            task = asyncio.create_task(backend._observe_slot(timing, stop))
            await asyncio.sleep(0.05)
            stop.set()
            await task
            return timing

        timing = run(scenario())
        self.assertEqual(timing.prompt_tokens, 13271)
        self.assertEqual(timing.prompt_tokens_processed, 8192)
        self.assertEqual(timing.cached_tokens, 2048)
        self.assertEqual(timing.output_tokens, 3)
        self.assertIsNotNone(timing.live_prefill_tokens_per_second())

    def test_missing_template_endpoint_still_counts_real_tokens(self) -> None:
        engine = FakeEngine(body="", tokens=7, template=False)
        backend = backend_for(engine)
        estimate = run(backend.count_tokens(turn()))
        self.assertEqual(estimate.input_tokens, 7)

    def test_residency_never_reports_disk_size_as_memory(self) -> None:
        engine = FakeEngine(body=_chunk(content="ok") + _final())
        backend = backend_for(engine)
        run(backend.load())
        residency = backend.residency(900)
        self.assertTrue(residency["loaded"])
        self.assertIsNone(residency["free_at"])
        self.assertIsNone(backend.memory_bytes())

    def test_explicit_unload_explains_who_owns_the_weights(self) -> None:
        engine = FakeEngine(body="")
        backend = backend_for(engine)
        with self.assertRaises(ResponsesError) as raised:
            run(backend.unload())
        self.assertEqual(raised.exception.code, "engine_release_not_supported")
        self.assertFalse(run(backend.unload_if_idle(1)))


def _muse_config(tmp: Path, *, engine_url: str | None = ENGINE_URL) -> LagunaConfig:
    models = tmp / "models"
    data = tmp / "data"
    (models / MUSE_GLIMMER_MODEL).mkdir(parents=True, exist_ok=True)
    data.mkdir(parents=True, exist_ok=True)
    return LagunaConfig(
        host="127.0.0.1",
        port=7333,
        backend="llama_cpp",
        api_key="test-key",
        models_dir=models,
        default_model=MUSE_GLIMMER_MODEL,
        model=MUSE_GLIMMER_MODEL,
        revision=None,
        draft_model=None,
        adapter=None,
        external_url=None,
        upstream_api_key=None,
        data_dir=data,
        auto_load=False,
        idle_unload_after_seconds=900,
        context_length=MUSE_CONTEXT_LENGTH,
        started_at=time.time(),
        engine_url=engine_url,
    )


class MuseSidecarSurfaceTests(unittest.TestCase):
    """Both wire surfaces, end to end through the app, on the Muse backend."""

    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory(prefix="laguna-muse-")
        self.engine = FakeEngine(
            body=_chunk(reasoning_content="thinking") + _chunk(content="Muse here") + _final()
        )
        config = _muse_config(Path(self.temp.name))
        self.app = build_app(config)
        # The bound backend is the real one; only its transport is fake.
        self.app.state.responses_service.backend._transport = self.engine.transport()
        self.client_context = TestClient(self.app)
        self.client = self.client_context.__enter__()
        self.headers = {"Authorization": "Bearer test-key"}

    def tearDown(self) -> None:
        self.client_context.__exit__(None, None, None)
        self.temp.cleanup()

    def test_backend_binding_is_the_llama_cpp_backend(self) -> None:
        backend = self.app.state.responses_service.backend
        self.assertIsInstance(backend, LlamaCppChatBackend)
        self.assertNotIsInstance(backend, RemoteResponsesBackend)

    def test_responses_turn_produces_assistant_text(self) -> None:
        response = self.client.post(
            "/v1/responses",
            headers=self.headers,
            json={"model": MUSE_GLIMMER_MODEL, "input": "hi", "store": False},
        )
        self.assertEqual(response.status_code, 200)
        body = response.json()
        self.assertEqual(body["status"], "completed")
        text = [
            part["text"]
            for item in body["output"]
            if item["type"] == "message"
            for part in item["content"]
            if part["type"] == "output_text"
        ]
        self.assertEqual(text, ["Muse here"])
        reasoning = [item for item in body["output"] if item["type"] == "reasoning"]
        self.assertTrue(reasoning)
        self.assertEqual(body["usage"]["input_tokens"], 11)

    def test_responses_stream_and_non_stream_agree(self) -> None:
        request = {"model": MUSE_GLIMMER_MODEL, "input": "hi", "store": False}
        non_stream = self.client.post(
            "/v1/responses", headers=self.headers, json=request
        ).json()
        with self.client.stream(
            "POST", "/v1/responses", headers=self.headers, json={**request, "stream": True}
        ) as response:
            frames = "".join(response.iter_text())
        events = [
            json.loads(line[6:])
            for line in frames.splitlines()
            if line.startswith("data: ") and line[6:] != "[DONE]"
        ]
        streamed = events[-1]["response"]
        self.assertEqual(streamed["status"], "completed")
        self.assertEqual(
            [item["type"] for item in streamed["output"]],
            [item["type"] for item in non_stream["output"]],
        )
        self.assertEqual(streamed["usage"]["output_tokens"], non_stream["usage"]["output_tokens"])

    def test_chat_completions_is_a_peer_not_a_501(self) -> None:
        response = self.client.post(
            "/v1/chat/completions",
            headers=self.headers,
            json={"model": MUSE_GLIMMER_MODEL, "messages": [{"role": "user", "content": "hi"}]},
        )
        self.assertEqual(response.status_code, 200)
        body = response.json()
        self.assertEqual(body["object"], "chat.completion")
        self.assertEqual(body["choices"][0]["message"]["content"], "Muse here")
        self.assertEqual(body["choices"][0]["message"]["reasoning_content"], "thinking")
        self.assertEqual(body["usage"]["prompt_tokens"], 11)

    def test_chat_stream_frames(self) -> None:
        with self.client.stream(
            "POST",
            "/v1/chat/completions",
            headers=self.headers,
            json={
                "model": MUSE_GLIMMER_MODEL,
                "messages": [{"role": "user", "content": "hi"}],
                "stream": True,
            },
        ) as response:
            self.assertEqual(response.status_code, 200)
            frames = "".join(response.iter_text())
        chunks = [
            json.loads(line[6:])
            for line in frames.splitlines()
            if line.startswith("data: ") and line[6:] != "[DONE]"
        ]
        content = "".join(
            chunk["choices"][0]["delta"].get("content", "")
            for chunk in chunks
            if chunk.get("choices")
        )
        self.assertEqual(content, "Muse here")
        self.assertTrue(frames.endswith("data: [DONE]\n\n"))

    def test_legacy_model_id_is_normalized_on_both_surfaces(self) -> None:
        chat = self.client.post(
            "/v1/chat/completions",
            headers=self.headers,
            json={
                "model": MUSE_GLIMMER_LEGACY_MODEL,
                "messages": [{"role": "user", "content": "hi"}],
            },
        )
        self.assertEqual(chat.json()["model"], MUSE_GLIMMER_MODEL)
        responses = self.client.post(
            "/v1/responses",
            headers=self.headers,
            json={"model": MUSE_GLIMMER_LEGACY_MODEL, "input": "hi", "store": False},
        )
        self.assertEqual(responses.json()["model"], MUSE_GLIMMER_MODEL)

    def test_health_advertises_both_surfaces_and_the_engine(self) -> None:
        body = self.client.get("/health", headers=self.headers).json()
        self.assertTrue(body["responsesApi"])
        self.assertTrue(body["chatCompletionsApi"])
        self.assertEqual(body["status"], "ok")
        self.assertEqual(body["backend"], "llama_cpp")
        self.assertEqual(body["defaultModel"], MUSE_GLIMMER_MODEL)
        self.assertEqual(body["engine"]["state"], "ready")
        # Weights this process cannot measure are reported as unknown, never
        # as the checkpoint's size on disk.
        self.assertIsNone(body["memoryBytes"])

    def test_health_fails_closed_when_the_engine_is_gone(self) -> None:
        def refuse(request: httpx.Request) -> httpx.Response:
            raise httpx.ConnectError("connection refused", request=request)

        backend = self.app.state.responses_service.backend
        run(backend.close())
        backend._transport = httpx.MockTransport(refuse)
        body = self.client.get("/health", headers=self.headers).json()
        self.assertFalse(body["responsesApi"])
        self.assertFalse(body["chatCompletionsApi"])
        self.assertEqual(body["status"], "error")
        self.assertIn("No engine is answering", body["detail"])

    def test_model_card_describes_muse_honestly(self) -> None:
        body = self.client.get("/v1/models", headers=self.headers).json()
        item = body["data"][0]
        self.assertEqual(item["id"], MUSE_GLIMMER_MODEL)
        self.assertEqual(item["details"]["family"], "muse_glimmer")
        self.assertEqual(item["details"]["format"], "gguf")
        self.assertEqual(item["context_length"], MUSE_CONTEXT_LENGTH)
        codex = body["models"][0]
        self.assertEqual(codex["context_window"], MUSE_CONTEXT_LENGTH)
        self.assertEqual(
            codex["auto_compact_token_limit"], int(MUSE_CONTEXT_LENGTH * 0.9)
        )
        # Vision is not advertised while no surface lowers an image part to
        # the engine, even though the projector is loaded beside the weights.
        self.assertEqual(codex["input_modalities"], ["text"])

    def test_unload_reports_the_engine_owner_not_a_phantom_generation(self) -> None:
        response = self.client.post("/v1/synth/model/unload", headers=self.headers)
        self.assertEqual(response.status_code, 409)
        self.assertEqual(
            response.json()["error"]["code"], "engine_release_not_supported"
        )

    def test_inference_snapshot_reports_muse_not_laguna(self) -> None:
        self.client.post(
            "/v1/responses",
            headers=self.headers,
            json={"model": MUSE_GLIMMER_MODEL, "input": "hi", "store": False},
        )
        snapshot = self.client.get("/v1/synth/inference", headers=self.headers).json()
        self.assertEqual(snapshot["model"], MUSE_GLIMMER_MODEL)
        self.assertIsNone(snapshot["residentBytes"])
        self.assertEqual(snapshot["queueCapacity"], 9)

    def test_control_plane_reports_a_canonical_state(self) -> None:
        from laguna_daemon.synth_control import CANONICAL_STATES

        status = self.client.get("/v1/synth/status", headers=self.headers).json()
        self.assertIn(status["state"], CANONICAL_STATES)
        self.assertEqual(status["backend"], "llama_cpp")


class MuseIdentityAndBindingTests(unittest.TestCase):
    """One id, one backend — enforced rather than documented."""

    def test_canonical_id_is_the_gguf_spelling(self) -> None:
        self.assertEqual(MUSE_GLIMMER_MODEL, "meta-models/Muse-Glimmer-30B-GGUF")
        self.assertEqual(
            LagunaConfig.normalize_model_id(MUSE_GLIMMER_LEGACY_MODEL),
            MUSE_GLIMMER_MODEL,
        )

    def test_daemon_sources_never_spell_the_legacy_id_as_a_selection(self) -> None:
        package = Path(__file__).parents[1] / "laguna_daemon"
        for path in package.rglob("*.py"):
            if path.name == "config.py":
                continue  # the one place the accepted alias is defined
            with self.subTest(module=path.name):
                self.assertNotIn(
                    "Muse-Glimmer-30B\"", path.read_text(encoding="utf-8")
                )

    def test_muse_never_binds_to_the_responses_passthrough(self) -> None:
        with tempfile.TemporaryDirectory(prefix="laguna-muse-bind-") as tmp:
            config = _muse_config(Path(tmp))
            backend = ResponsesService._make_backend(config)
            self.assertIsInstance(backend, LlamaCppChatBackend)

            external = LagunaConfig(
                **{
                    **{
                        field: getattr(config, field)
                        for field in config.__slots__
                        if field != "started_at"
                    },
                    "backend": "external",
                    "external_url": ENGINE_URL,
                    "started_at": time.time(),
                }
            )
            with self.assertRaises(RuntimeError):
                ResponsesService._make_backend(external)

    def test_muse_selection_resolves_to_the_engine_backend_from_env(self) -> None:
        import os

        with tempfile.TemporaryDirectory(prefix="laguna-muse-env-") as tmp:
            saved = {
                key: os.environ.get(key)
                for key in (
                    "SYNTH_LAGUNA_BACKEND",
                    "SYNTH_LAGUNA_DEFAULT_MODEL",
                    "SYNTH_LAGUNA_ENGINE_URL",
                    "SYNTH_LAGUNA_EXTERNAL_URL",
                    "SYNTH_LAGUNA_DATA_DIR",
                    "SYNTH_LAGUNA_MODELS_DIR",
                    "SYNTH_LAGUNA_CONTEXT_LENGTH",
                )
            }
            try:
                os.environ.update(
                    {
                        "SYNTH_LAGUNA_BACKEND": "external",
                        "SYNTH_LAGUNA_DEFAULT_MODEL": MUSE_GLIMMER_LEGACY_MODEL,
                        "SYNTH_LAGUNA_EXTERNAL_URL": ENGINE_URL,
                        "SYNTH_LAGUNA_DATA_DIR": str(Path(tmp) / "data"),
                        "SYNTH_LAGUNA_MODELS_DIR": str(Path(tmp) / "models"),
                    }
                )
                os.environ.pop("SYNTH_LAGUNA_ENGINE_URL", None)
                os.environ.pop("SYNTH_LAGUNA_CONTEXT_LENGTH", None)
                config = LagunaConfig.from_env()
            finally:
                for key, value in saved.items():
                    if value is None:
                        os.environ.pop(key, None)
                    else:
                        os.environ[key] = value
        # A Desktop build from before this backend existed passed the engine
        # address as an `external` upstream and the pre-GGUF id. Both are
        # migrated rather than obeyed.
        self.assertEqual(config.backend, "llama_cpp")
        self.assertEqual(config.default_model, MUSE_GLIMMER_MODEL)
        self.assertEqual(config.engine_base_url, ENGINE_URL)
        self.assertIsNone(config.external_url)
        self.assertEqual(config.context_length, MUSE_CONTEXT_LENGTH)

    def test_engine_address_is_never_hardcoded_in_the_daemon(self) -> None:
        """The daemon owns no port for a process it does not start.

        The supervisor passes the address in. A literal here would be the
        daemon claiming knowledge of a runtime it must not manage — the exact
        coupling `test_no_second_runtime` exists to prevent.
        """
        source = (
            Path(__file__).parents[1]
            / "laguna_daemon"
            / "responses_api"
            / "backends"
            / "llama_cpp.py"
        ).read_text(encoding="utf-8")
        tree = ast.parse(source)
        for node in ast.walk(tree):
            if isinstance(node, ast.Constant) and isinstance(node.value, str):
                self.assertNotIn("127.0.0.1", node.value)


if __name__ == "__main__":
    unittest.main()
