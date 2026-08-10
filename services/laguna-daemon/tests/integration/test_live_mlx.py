"""Live integration suite for the local MLX daemon.

Runs against a real daemon serving real weights. Skipped unless
`SYNTH_LAGUNA_LIVE_BASE_URL` is set, so the deterministic suite stays hermetic.

    SYNTH_LAGUNA_LIVE_BASE_URL=http://127.0.0.1:7333 \
    SYNTH_LAGUNA_LIVE_API_KEY=... \
    ./scripts/laguna/test.sh

Or use `scripts/laguna/live_test.sh`, which reads the daemon's key file.

These checks assert *contracts*, not performance numbers: an assertion on
absolute tokens/second would encode one machine's thermal state as a
requirement. Throughput is measured and reported — written to
`SYNTH_LAGUNA_LIVE_REPORT` if set — with only loose sanity floors asserted, so
runs stay comparable over time without becoming flaky.
"""

from __future__ import annotations

import json
import os
import time
import unittest
from pathlib import Path
from typing import Any

import httpx


BASE_URL = (os.getenv("SYNTH_LAGUNA_LIVE_BASE_URL") or "").rstrip("/")
API_KEY = os.getenv("SYNTH_LAGUNA_LIVE_API_KEY") or None
REPORT_PATH = os.getenv("SYNTH_LAGUNA_LIVE_REPORT")
# A cold 24B load plus a long prefill can legitimately take minutes.
TIMEOUT = float(os.getenv("SYNTH_LAGUNA_LIVE_TIMEOUT", "600"))

# Laguna is a reasoning model: it spends its first tokens inside a <think>
# span before emitting any answer at all. Every budget below must therefore
# cover reasoning *plus* the answer. Sizing them like a non-reasoning model
# makes turns terminate with finish_reason "length" (or status "incomplete")
# before the assistant has said anything — which looks exactly like a daemon
# bug and is not one.
ANSWER_TOKENS = 1024
TOOL_TOKENS = 1536
LONG_TOKENS = 4096

# The checkpoint ships no generation_config.json, so the model publishes no
# recommended sampling. 0.7 / 0.95 is the conventional pairing for a reasoning
# model and is chosen deliberately here: greedy decoding (temperature 0) drives
# this model into repetition loops that run to the token cap, which tests the
# sampler rather than whatever the surrounding case is actually about.
TEMPERATURE = 0.7
TOP_P = 0.95

requires_live_daemon = unittest.skipUnless(
    BASE_URL, "set SYNTH_LAGUNA_LIVE_BASE_URL to run the live MLX suite"
)

#: Collected measurements, reported at the end of the run.
MEASUREMENTS: dict[str, Any] = {}


def record(name: str, **values: Any) -> None:
    MEASUREMENTS[name] = values
    rendered = " ".join(f"{key}={value}" for key, value in values.items())
    print(f"    [measured] {name}: {rendered}", flush=True)


def tearDownModule() -> None:
    if not MEASUREMENTS:
        return
    print("\n=== live MLX measurements ===")
    print(json.dumps(MEASUREMENTS, indent=2, sort_keys=True))
    if REPORT_PATH:
        path = Path(REPORT_PATH)
        try:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(
                json.dumps(MEASUREMENTS, indent=2, sort_keys=True), encoding="utf-8"
            )
            print(f"wrote {path}")
        except OSError as error:
            print(f"could not write {path}: {error}")


class LiveDaemonTestCase(unittest.TestCase):
    """Shared plumbing for talking to a real daemon."""

    client: httpx.Client
    model: str

    @classmethod
    def setUpClass(cls) -> None:
        if not BASE_URL:
            raise unittest.SkipTest("no live daemon configured")
        headers = {"Authorization": f"Bearer {API_KEY}"} if API_KEY else {}
        cls.client = httpx.Client(base_url=BASE_URL, headers=headers, timeout=TIMEOUT)
        health = cls.client.get("/health")
        if health.status_code != 200:
            raise unittest.SkipTest(f"daemon not healthy: {health.status_code}")
        cls.model = health.json()["defaultModel"]

    @classmethod
    def tearDownClass(cls) -> None:
        client = getattr(cls, "client", None)
        if client is not None:
            client.close()

    # -- helpers ------------------------------------------------------------

    def chat(self, **body: Any) -> dict[str, Any]:
        response = self.client.post(
            "/v1/chat/completions", json={"model": self.model, **body}
        )
        self.assertEqual(response.status_code, 200, response.text)
        return response.json()

    def responses(self, **body: Any) -> dict[str, Any]:
        response = self.client.post(
            "/v1/responses", json={"model": self.model, **body}
        )
        self.assertEqual(response.status_code, 200, response.text)
        return response.json()

    def stream_lines(self, path: str, body: dict[str, Any]) -> list[str]:
        lines: list[str] = []
        with self.client.stream(
            "POST", path, json={"model": self.model, **body}
        ) as response:
            self.assertEqual(response.status_code, 200)
            for line in response.iter_lines():
                if line:
                    lines.append(line)
        return lines

    def response_events(self, **body: Any) -> list[dict[str, Any]]:
        """Collect the typed events from one real Responses SSE stream."""
        events: list[dict[str, Any]] = []
        for line in self.stream_lines(
            "/v1/responses", {"stream": True, "store": False, **body}
        ):
            if not line.startswith("data: ") or line == "data: [DONE]":
                continue
            events.append(json.loads(line[len("data: ") :]))
        return events

    def inference(self) -> dict[str, Any]:
        response = self.client.get("/v1/synth/inference")
        self.assertEqual(response.status_code, 200)
        return response.json()

    def text_of(self, response: dict[str, Any]) -> str:
        """Concatenate assistant text from a Responses object."""
        parts = []
        for item in response.get("output") or []:
            if item.get("type") != "message":
                continue
            for part in item.get("content") or []:
                if part.get("type") == "output_text":
                    parts.append(part.get("text") or "")
        return "".join(parts)


@requires_live_daemon
class LiveArchitectureTests(LiveDaemonTestCase):
    """One daemon, one runtime, both surfaces."""

    def test_health_reports_a_single_native_runtime(self) -> None:
        health = self.client.get("/health").json()
        self.assertEqual(health["status"], "ok")
        self.assertIs(health["responsesApi"], True)
        self.assertIs(health["chatCompletionsApi"], True)
        # The legacy second-engine switch must be gone from the live build.
        self.assertNotIn("responsesEngine", health)
        self.assertEqual(health["responses"]["engine"], "native")
        self.assertEqual(health["responses"]["backend"], "NativeMlxBackend")

    def test_no_second_local_server_is_listening(self) -> None:
        """An MLX selection is self-contained: nothing on the legacy port.

        Scoped to this module's live MLX daemon. A GGUF selection does have a
        supervisor-owned engine on that port, which the daemon drives as a
        backend and never proxies to; `tests/test_muse_llama_cpp.py` covers it.
        """
        with self.assertRaises(httpx.ConnectError):
            httpx.get("http://127.0.0.1:7334/health", timeout=3)

    def test_models_advertises_the_codex_envelope(self) -> None:
        models = self.client.get("/v1/models").json()
        self.assertEqual(models["object"], "list")
        self.assertEqual(models["data"][0]["id"], self.model)
        self.assertTrue(models["models"], "Codex model envelope is missing")


@requires_live_daemon
class LiveResponsesTests(LiveDaemonTestCase):
    def test_non_stream_completes_against_real_weights(self) -> None:
        started = time.monotonic()
        body = self.responses(
            input="Reply with exactly the word: pong", store=False, max_output_tokens=ANSWER_TOKENS
        )
        elapsed = time.monotonic() - started
        self.assertEqual(body["status"], "completed")
        self.assertTrue(self.text_of(body).strip())
        self.assertGreater(body["usage"]["output_tokens"], 0)
        record(
            "responses_non_stream",
            latency_s=round(elapsed, 3),
            input_tokens=body["usage"]["input_tokens"],
            output_tokens=body["usage"]["output_tokens"],
        )

    def test_stream_and_non_stream_agree(self) -> None:
        prompt = "Name the first three prime numbers, digits only."
        direct = self.responses(
            input=prompt, store=False, temperature=TEMPERATURE, top_p=TOP_P, max_output_tokens=ANSWER_TOKENS
        )
        lines = self.stream_lines(
            "/v1/responses",
            {"input": prompt, "stream": True, "store": False, "temperature": TEMPERATURE, "top_p": TOP_P,
             "max_output_tokens": ANSWER_TOKENS},
        )
        completed = [
            json.loads(line[len("data: ") :])
            for line in lines
            if line.startswith("data: ") and line != "data: [DONE]"
        ]
        final = next(
            event["response"]
            for event in completed
            if event.get("type") == "response.completed"
        )
        self.assertEqual(final["status"], "completed")
        # Greedy decoding makes the two paths comparable; the invariant under
        # test is that the streamed final object equals the direct one in shape
        # and accounting, not that a sampled model repeats itself.
        self.assertEqual(
            set(final["usage"]), set(direct["usage"]),
            "streamed and direct usage shapes diverged",
        )
        self.assertGreater(final["usage"]["output_tokens"], 0)

    def test_previous_response_id_continues_a_thread(self) -> None:
        first = self.responses(input="Remember the number 41.", max_output_tokens=ANSWER_TOKENS)
        second = self.responses(
            input="What number did I ask you to remember? Digits only.",
            previous_response_id=first["id"],
            max_output_tokens=ANSWER_TOKENS,
        )
        self.assertEqual(second["status"], "completed")
        self.assertEqual(second["previous_response_id"], first["id"])

    def test_function_tool_round_trip(self) -> None:
        tools = [
            {
                "type": "function",
                "name": "get_weather",
                "description": "Look up the weather for a city.",
                "parameters": {
                    "type": "object",
                    "properties": {"city": {"type": "string"}},
                    "required": ["city"],
                    "additionalProperties": False,
                },
            }
        ]
        body = self.responses(
            input="What is the weather in Paris? Use the tool.",
            tools=tools,
            tool_choice="required",
            store=False,
            max_output_tokens=TOOL_TOKENS,
        )
        calls = [
            item for item in body["output"] if item.get("type") == "function_call"
        ]
        self.assertTrue(calls, f"model did not call the tool: {body['output']}")
        self.assertEqual(calls[0]["name"], "get_weather")
        json.loads(calls[0]["arguments"])

    def test_custom_tool_preserves_raw_input(self) -> None:
        """The Responses-only tool kind that Chat cannot represent."""
        body = self.responses(
            input="Invoke the echo tool with the text: hello",
            tools=[
                {
                    "type": "custom",
                    "name": "echo",
                    "description": "Echo raw text back verbatim.",
                }
            ],
            tool_choice="required",
            store=False,
            max_output_tokens=TOOL_TOKENS,
        )
        calls = [
            item for item in body["output"] if item.get("type") == "custom_tool_call"
        ]
        self.assertTrue(calls, f"no custom_tool_call in {body['output']}")
        self.assertEqual(calls[0]["name"], "echo")
        self.assertIsInstance(calls[0]["input"], str)

    def test_structured_output_is_grammar_constrained(self) -> None:
        schema = {
            "type": "object",
            "properties": {"city": {"type": "string"}, "population": {"type": "integer"}},
            "required": ["city", "population"],
            "additionalProperties": False,
        }
        body = self.responses(
            input="Give me a city and its population.",
            text={"format": {"type": "json_schema", "name": "city", "schema": schema}},
            store=False,
            max_output_tokens=TOOL_TOKENS,
        )
        parsed = json.loads(self.text_of(body))
        self.assertIn("city", parsed)
        self.assertIsInstance(parsed["population"], int)


@requires_live_daemon
class LiveReasoningMatrixTests(LiveDaemonTestCase):
    """Exercise reasoning as a sidecar contract, including tool-bearing turns.

    Codex advertises tools on nearly every local turn. A text-only reasoning
    check can therefore be green while the real desktop never receives a
    reasoning token. Keep both axes in this live suite.
    """

    TOOL = {
        "type": "function",
        "name": "inspect_directory",
        "description": "List entries in a directory.",
        "parameters": {
            "type": "object",
            "properties": {"path": {"type": "string"}},
            "required": ["path"],
            "additionalProperties": False,
        },
    }

    @staticmethod
    def _reasoning_deltas(events: list[dict[str, Any]]) -> list[str]:
        return [
            str(event.get("delta") or "")
            for event in events
            if event.get("type") == "response.reasoning_summary_text.delta"
        ]

    def test_reasoning_effort_matrix_streams_what_the_model_was_asked_for(self) -> None:
        prompt = (
            "Work this out carefully and explain the result: compute 12345 "
            "times 6789. Do not call a tool unless it is needed."
        )
        # Measured on real weights (2026-08-09): Laguna's willingness to open
        # a reasoning span on a tool-bearing turn is conditioned on the
        # system header's substance. With a substantive system prompt the
        # first-token P(</think>) is ~0.07-0.12 (reasons); with a terse
        # directive-only header it is ~0.80 (skips); with no system message
        # at all the template's default persona leaves it a ~0.35 coin flip.
        # Codex always sends real instructions, so the matrix models that
        # shape rather than gating a release on a coin flip.
        instructions = "You are a coding agent running in a terminal."
        cases = (
            ("none_without_tools", "none", [], False),
            ("high_without_tools", "high", [], True),
            # `max` was the desktop's legacy label. The sidecar accepts it as
            # a compatibility alias for high rather than rejecting a cold turn.
            ("max_alias_without_tools", "max", [], True),
            # This is the real Codex shape: tools are advertised even when the
            # answer itself does not need one.
            ("high_with_tools", "high", [self.TOOL], True),
        )
        # Thinking-enabled turns keep the span *open*; whether the model uses
        # it is a per-sample draw (P(skip) ≈ 0.07-0.35 depending on header).
        # `none` must therefore be exact on every sample, while `high` cases
        # get a small bounded retry so a legitimate skip-draw cannot fail the
        # release gate. Systematic suppression still fails: three consecutive
        # skips has probability under a few percent even at the worst
        # measured header, and exactly zero when streaming itself is broken.
        ATTEMPTS = 3
        results: dict[str, Any] = {}
        for name, effort, tools, expect_reasoning in cases:
            with self.subTest(case=name):
                attempts_used = 0
                deltas: list[str] = []
                event_types: list[str] = []
                for attempt in range(ATTEMPTS if expect_reasoning else 1):
                    attempts_used = attempt + 1
                    events = self.response_events(
                        input=prompt,
                        instructions=instructions,
                        reasoning={"effort": effort, "summary": "auto"},
                        tools=tools,
                        temperature=TEMPERATURE,
                        top_p=TOP_P,
                        max_output_tokens=768,
                    )
                    deltas = self._reasoning_deltas(events)
                    event_types = [str(event.get("type") or "") for event in events]
                    self.assertTrue(
                        any(kind in {"response.completed", "response.incomplete"} for kind in event_types),
                        f"{name} never emitted a terminal response event",
                    )
                    if not expect_reasoning or deltas:
                        break
                results[name] = {
                    "reasoning_frames": len(deltas),
                    "reasoning_chars": sum(map(len, deltas)),
                    "attempts": attempts_used,
                    "terminal": event_types[-1] if event_types else None,
                }
                if expect_reasoning:
                    self.assertGreater(
                        len(deltas),
                        0,
                        f"{name} requested reasoning but streamed none in "
                        f"{attempts_used} attempts",
                    )
                else:
                    self.assertEqual(deltas, [], f"{name} unexpectedly streamed reasoning")
        record("reasoning_effort_tool_matrix", **results)


@requires_live_daemon
class LiveRefactorContractTests(LiveDaemonTestCase):
    """Driver contracts for the 2026-08-09 refactor.

    Each test states a contract the reference stacks (Poolside model card,
    vLLM/SGLang poolside_v1, mlx-lm server) agree on. They are written before
    the implementation on purpose; a red case here is the work list.
    """

    TOOL = {
        "type": "function",
        "name": "inspect_directory",
        "description": "List entries in a directory.",
        "parameters": {
            "type": "object",
            "properties": {"path": {"type": "string"}},
            "required": ["path"],
            "additionalProperties": False,
        },
    }

    def test_answer_text_streams_incrementally_when_tools_are_advertised(self) -> None:
        """A tool-advertised turn that ends in prose must stream that prose.

        Codex advertises tools on nearly every turn; buffering the whole answer
        until generation finishes silently degrades the ambient client to
        non-streaming. Contract: many `response.output_text.delta` frames
        spread over real time, exactly like the tool-free path.
        """
        arrivals: list[float] = []
        started = time.monotonic()
        with self.client.stream(
            "POST",
            "/v1/responses",
            json={
                "model": self.model,
                "input": (
                    "Without calling any tool, write a short paragraph of about "
                    "80 words describing what a filesystem inode stores."
                ),
                "tools": [self.TOOL],
                "reasoning": {"effort": "high", "summary": "auto"},
                "stream": True,
                "store": False,
                "temperature": TEMPERATURE,
                "top_p": TOP_P,
                "max_output_tokens": TOOL_TOKENS,
            },
        ) as response:
            self.assertEqual(response.status_code, 200)
            for line in response.iter_lines():
                if not line.startswith("data: ") or line == "data: [DONE]":
                    continue
                event = json.loads(line[len("data: ") :])
                if event.get("type") == "response.output_text.delta" and event.get("delta"):
                    arrivals.append(time.monotonic() - started)
        self.assertGreater(
            len(arrivals), 5,
            f"expected many incremental answer frames with tools advertised, got "
            f"{len(arrivals)} — the answer was buffered until generation finished",
        )
        span = arrivals[-1] - arrivals[0]
        self.assertGreater(
            span, 0.05,
            "all answer frames arrived at once, so the answer was buffered",
        )
        record(
            "tool_advertised_answer_streaming",
            frames=len(arrivals), span_s=round(span, 3),
        )

    def test_chat_tool_loop_round_trips_reasoning_content(self) -> None:
        """Poolside: preserved thinking is the contract for multi-step tools.

        The model card requires `reasoning_content` from prior assistant
        messages to be sent back and re-rendered inside <think>...</think>;
        dropping it is the documented cause of the model not reasoning in
        follow-up steps. The Chat surface must therefore carry an inbound
        assistant `reasoning_content` through to the template, and a
        tool-bearing first turn must produce reasoning to preserve at all.
        """
        tools = [
            {
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "Look up the weather for a city.",
                    "parameters": {
                        "type": "object",
                        "properties": {"city": {"type": "string"}},
                        "required": ["city"],
                        "additionalProperties": False,
                    },
                },
            }
        ]
        first = self.chat(
            messages=[
                {
                    "role": "user",
                    "content": "Think about which city I most likely mean, then "
                    "look up the weather: I'm at the Eiffel Tower.",
                }
            ],
            tools=tools,
            temperature=TEMPERATURE,
            top_p=TOP_P,
            max_tokens=TOOL_TOKENS,
        )
        message = first["choices"][0]["message"]
        self.assertTrue(
            (message.get("reasoning_content") or "").strip(),
            "tool-bearing first turn produced no reasoning_content — the "
            "preserved-thinking contract has nothing to preserve",
        )
        calls = message.get("tool_calls") or []
        self.assertTrue(calls, f"first turn made no tool call: {message}")

        second = self.chat(
            messages=[
                {
                    "role": "user",
                    "content": "Think about which city I most likely mean, then "
                    "look up the weather: I'm at the Eiffel Tower.",
                },
                {
                    "role": "assistant",
                    "content": message.get("content"),
                    "reasoning_content": message.get("reasoning_content"),
                    "tool_calls": calls,
                },
                {
                    "role": "tool",
                    "tool_call_id": calls[0]["id"],
                    "content": '{"temp_c": 19, "sky": "overcast"}',
                },
            ],
            tools=tools,
            temperature=TEMPERATURE,
            top_p=TOP_P,
            max_tokens=TOOL_TOKENS,
        )
        final = second["choices"][0]["message"]
        self.assertTrue((final.get("content") or "").strip())
        record(
            "chat_tool_loop_reasoning",
            first_reasoning_chars=len(message.get("reasoning_content") or ""),
            second_reasoning_chars=len(final.get("reasoning_content") or ""),
        )

    def test_reasoning_tokens_are_counted_not_estimated(self) -> None:
        """usage.output_tokens_details.reasoning_tokens ≤ output_tokens, always."""
        body = self.responses(
            input="Work out 47 times 83 carefully, then give the product.",
            reasoning={"effort": "high", "summary": "auto"},
            store=False,
            temperature=TEMPERATURE,
            top_p=TOP_P,
            max_output_tokens=TOOL_TOKENS,
        )
        usage = body["usage"]
        reasoning_tokens = usage["output_tokens_details"]["reasoning_tokens"]
        self.assertGreater(reasoning_tokens, 0, "high effort produced no counted reasoning")
        self.assertLessEqual(
            reasoning_tokens,
            usage["output_tokens"],
            f"reasoning_tokens {reasoning_tokens} exceeds output_tokens "
            f"{usage['output_tokens']} — an estimate, not a count",
        )

    def test_unsupported_reasoning_efforts_get_a_typed_error(self) -> None:
        """`none` and `high` are real; `max` is a documented alias; the rest lie."""
        for effort in ("low", "medium", "xhigh", "minimal"):
            with self.subTest(effort=effort):
                response = self.client.post(
                    "/v1/responses",
                    json={
                        "model": self.model,
                        "input": "hi",
                        "reasoning": {"effort": effort},
                        "store": False,
                    },
                )
                self.assertEqual(response.status_code, 400, response.text)
                self.assertEqual(
                    response.json()["error"]["code"], "unsupported_reasoning_effort"
                )

    def test_bare_responses_request_defaults_to_thinking_on(self) -> None:
        """/v1/models advertises default high; a request with no `reasoning`
        object must honor that default rather than silently disabling thinking.
        """
        body = self.responses(
            input="What is 23 + 54? Reply with the number only.",
            store=False,
            temperature=TEMPERATURE,
            top_p=TOP_P,
            max_output_tokens=ANSWER_TOKENS,
        )
        reasoning_items = [
            item for item in body["output"] if item.get("type") == "reasoning"
        ]
        self.assertTrue(
            reasoning_items,
            "a bare request silently lost the model's default thinking mode",
        )

    def test_model_card_sampling_is_accepted_end_to_end(self) -> None:
        """generation_config.json: temperature=1.0, top_k=20, top_p=1.0."""
        body = self.responses(
            input="Reply with exactly the word: calibrated",
            reasoning={"effort": "none"},
            store=False,
            temperature=1.0,
            top_p=1.0,
            top_k=20,
            max_output_tokens=ANSWER_TOKENS,
        )
        self.assertEqual(body["status"], "completed")
        self.assertTrue(self.text_of(body).strip())
        chat_body = self.chat(
            messages=[{"role": "user", "content": "Reply with exactly: calibrated"}],
            reasoning_effort="none",
            temperature=1.0,
            top_p=1.0,
            top_k=20,
            max_tokens=ANSWER_TOKENS,
        )
        self.assertTrue((chat_body["choices"][0]["message"]["content"] or "").strip())


@requires_live_daemon
class LiveChatTests(LiveDaemonTestCase):
    def test_non_stream_completes(self) -> None:
        started = time.monotonic()
        body = self.chat(
            messages=[{"role": "user", "content": "Reply with exactly: pong"}],
            max_tokens=ANSWER_TOKENS,
        )
        elapsed = time.monotonic() - started
        self.assertEqual(body["object"], "chat.completion")
        message = body["choices"][0]["message"]
        self.assertEqual(message["role"], "assistant")
        self.assertTrue((message["content"] or "").strip())
        self.assertGreater(body["usage"]["completion_tokens"], 0)
        record(
            "chat_non_stream",
            latency_s=round(elapsed, 3),
            prompt_tokens=body["usage"]["prompt_tokens"],
            completion_tokens=body["usage"]["completion_tokens"],
        )

    def test_stream_reassembles_to_the_same_shape(self) -> None:
        messages = [{"role": "user", "content": "Count from 1 to 5, digits only."}]
        lines = self.stream_lines(
            "/v1/chat/completions",
            {
                "messages": messages,
                "stream": True,
                "temperature": TEMPERATURE, "top_p": TOP_P,
                "max_tokens": ANSWER_TOKENS,
                "stream_options": {"include_usage": True},
            },
        )
        self.assertIn("data: [DONE]", lines)
        chunks = [
            json.loads(line[len("data: ") :])
            for line in lines
            if line.startswith("data: ") and line != "data: [DONE]"
        ]
        self.assertTrue(chunks)
        self.assertTrue(all(c["object"] == "chat.completion.chunk" for c in chunks))
        content = "".join(
            (choice.get("delta") or {}).get("content") or ""
            for chunk in chunks
            for choice in chunk.get("choices") or []
        )
        self.assertTrue(content.strip(), "stream produced no assistant content")
        finish = [
            choice["finish_reason"]
            for chunk in chunks
            for choice in chunk.get("choices") or []
            if choice.get("finish_reason")
        ]
        # "length" is a truthful terminal reason when a cap is reached; the
        # contract is that a terminal reason arrives at all, and is a real one.
        self.assertIn(finish[-1], {"stop", "length", "tool_calls"})
        usage = [chunk["usage"] for chunk in chunks if chunk.get("usage")]
        self.assertTrue(usage, "include_usage did not produce a usage chunk")
        self.assertGreater(usage[-1]["completion_tokens"], 0)

    def test_streaming_is_actually_incremental(self) -> None:
        """Frames must arrive as the model produces them.

        Regression coverage for a measured defect: a 12.7-second generation
        arrived as a single 1280-character frame, because the backend withheld
        every chunk until a `</think>` marker that this checkpoint often never
        emits. Streaming had silently degraded to non-streaming, and asserting
        only on the reassembled text could not tell the difference.
        """
        arrivals: list[tuple[float, int]] = []
        started = time.monotonic()
        with self.client.stream(
            "POST",
            "/v1/chat/completions",
            json={
                "model": self.model,
                "messages": [
                    {"role": "user", "content": "Count slowly from 1 to 20, one per line."}
                ],
                "stream": True,
                "max_tokens": ANSWER_TOKENS,
                "temperature": TEMPERATURE,
                "top_p": TOP_P,
            },
        ) as response:
            self.assertEqual(response.status_code, 200)
            for line in response.iter_lines():
                if not line.startswith("data: ") or line == "data: [DONE]":
                    continue
                event = json.loads(line[len("data: ") :])
                for choice in event.get("choices") or []:
                    delta = choice.get("delta") or {}
                    text = delta.get("content") or delta.get("reasoning_content") or ""
                    if text:
                        arrivals.append((time.monotonic() - started, len(text)))

        self.assertGreater(
            len(arrivals), 5,
            f"expected many incremental frames, got {len(arrivals)} — "
            "the response was buffered rather than streamed",
        )
        span = arrivals[-1][0] - arrivals[0][0]
        self.assertGreater(
            span, 0.05,
            "every frame arrived at effectively the same instant, so the "
            "generation was buffered and flushed at the end",
        )
        record(
            "streaming_incrementality",
            frames=len(arrivals),
            first_frame_s=round(arrivals[0][0], 3),
            last_frame_s=round(arrivals[-1][0], 3),
            span_s=round(span, 3),
        )

    def test_reasoning_never_leaks_into_content(self) -> None:
        body = self.chat(
            messages=[{"role": "user", "content": "What is 17 + 25?"}], max_tokens=ANSWER_TOKENS
        )
        message = body["choices"][0]["message"]
        content = message["content"] or ""
        self.assertNotIn("<think>", content)
        self.assertNotIn("</think>", content)

    def test_tool_call_and_continuation(self) -> None:
        tools = [
            {
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "Look up the weather for a city.",
                    "parameters": {
                        "type": "object",
                        "properties": {"city": {"type": "string"}},
                        "required": ["city"],
                        "additionalProperties": False,
                    },
                },
            }
        ]
        first = self.chat(
            messages=[{"role": "user", "content": "Weather in Paris? Use the tool."}],
            tools=tools,
            tool_choice="required",
            max_tokens=TOOL_TOKENS,
        )
        choice = first["choices"][0]
        self.assertEqual(choice["finish_reason"], "tool_calls")
        call = choice["message"]["tool_calls"][0]
        self.assertEqual(call["function"]["name"], "get_weather")
        json.loads(call["function"]["arguments"])

        second = self.chat(
            messages=[
                {"role": "user", "content": "Weather in Paris? Use the tool."},
                {"role": "assistant", "content": None, "tool_calls": [call]},
                {
                    "role": "tool",
                    "tool_call_id": call["id"],
                    "content": '{"temp_c": 17, "sky": "clear"}',
                },
            ],
            tools=tools,
            max_tokens=TOOL_TOKENS,
        )
        self.assertEqual(second["choices"][0]["finish_reason"], "stop")
        self.assertTrue((second["choices"][0]["message"]["content"] or "").strip())

    def test_json_schema_response_format(self) -> None:
        schema = {
            "type": "object",
            "properties": {"answer": {"type": "integer"}},
            "required": ["answer"],
            "additionalProperties": False,
        }
        body = self.chat(
            messages=[{"role": "user", "content": "What is 6 times 7?"}],
            response_format={
                "type": "json_schema",
                "json_schema": {"name": "answer", "schema": schema},
            },
            max_tokens=TOOL_TOKENS,
        )
        parsed = json.loads(body["choices"][0]["message"]["content"])
        self.assertIsInstance(parsed["answer"], int)

    def test_unsupported_fields_are_rejected_by_the_live_daemon(self) -> None:
        response = self.client.post(
            "/v1/chat/completions",
            json={
                "model": self.model,
                "messages": [{"role": "user", "content": "hi"}],
                "logprobs": True,
            },
        )
        self.assertEqual(response.status_code, 400)
        self.assertEqual(response.json()["error"]["code"], "unsupported_chat_field")


@requires_live_daemon
class LivePromptCacheTests(LiveDaemonTestCase):
    """A warm prefix must actually be reused, and be visible in telemetry."""

    def _long_prefix(self) -> str:
        # Long enough that prefill dominates and a cache hit is unmistakable.
        return (
            "You are reviewing a codebase. Here are the conventions:\n"
            + "\n".join(f"- Rule {index}: keep functions small and pure." for index in range(400))
        )

    def test_repeated_prefix_is_cached_and_faster(self) -> None:
        cache_key = f"live-cache-{int(time.time())}"
        prefix = self._long_prefix()

        def ask(question: str) -> tuple[dict[str, Any], float]:
            started = time.monotonic()
            body = self.responses(
                input=f"{prefix}\n\nQuestion: {question}",
                store=False,
                prompt_cache_key=cache_key,
                # This is a prefill/cache test, not an answer-quality test.
                # The minimum legal 16-token generation keeps total wall time
                # dominated by prefill instead of stochastic reasoning length.
                max_output_tokens=16,
                temperature=TEMPERATURE, top_p=TOP_P,
            )
            return body, time.monotonic() - started

        cold, cold_elapsed = ask("Reply with the word one.")
        warm, warm_elapsed = ask("Reply with the word two.")

        cold_cached = cold["usage"]["input_tokens_details"]["cached_tokens"]
        warm_cached = warm["usage"]["input_tokens_details"]["cached_tokens"]
        record(
            "prompt_cache",
            prompt_tokens=warm["usage"]["input_tokens"],
            cold_cached_tokens=cold_cached,
            warm_cached_tokens=warm_cached,
            cold_latency_s=round(cold_elapsed, 3),
            warm_latency_s=round(warm_elapsed, 3),
            speedup=round(cold_elapsed / warm_elapsed, 3) if warm_elapsed else None,
        )

        self.assertGreater(
            warm_cached, 0, "second request with the same prefix reused no cache"
        )
        self.assertGreater(
            warm_cached, cold_cached, "cache did not grow across identical prefixes"
        )
        # Reusing thousands of prefill tokens must not be slower than redoing
        # them. Deliberately loose: this is a regression guard, not a benchmark.
        self.assertLess(
            warm_elapsed,
            cold_elapsed * 1.5,
            f"warm request was not faster (cold={cold_elapsed:.2f}s warm={warm_elapsed:.2f}s)",
        )

    def test_distinct_cache_keys_do_not_share_state(self) -> None:
        prefix = self._long_prefix()
        first = self.responses(
            input=f"{prefix}\n\nQuestion: reply one.",
            store=False,
            prompt_cache_key=f"live-a-{int(time.time())}",
            max_output_tokens=ANSWER_TOKENS,
        )
        second = self.responses(
            input=f"{prefix}\n\nQuestion: reply two.",
            store=False,
            prompt_cache_key=f"live-b-{int(time.time())}",
            max_output_tokens=ANSWER_TOKENS,
        )
        self.assertEqual(
            second["usage"]["input_tokens_details"]["cached_tokens"],
            0,
            "a fresh cache key reused another key's prefix",
        )
        self.assertEqual(first["status"], "completed")


@requires_live_daemon
class LiveThroughputTests(LiveDaemonTestCase):
    """Measure and report; assert only sanity floors."""

    def test_decode_throughput_is_measured_and_sane(self) -> None:
        prompt = "Write a short paragraph about the sea."
        first_token_at: float | None = None
        chunks = 0
        started = time.monotonic()
        with self.client.stream(
            "POST",
            "/v1/chat/completions",
            json={
                "model": self.model,
                "messages": [{"role": "user", "content": prompt}],
                "stream": True,
                "max_tokens": 200,
                "stream_options": {"include_usage": True},
            },
        ) as response:
            self.assertEqual(response.status_code, 200)
            usage: dict[str, Any] | None = None
            for line in response.iter_lines():
                if not line.startswith("data: ") or line == "data: [DONE]":
                    continue
                event = json.loads(line[len("data: ") :])
                if event.get("usage"):
                    usage = event["usage"]
                for choice in event.get("choices") or []:
                    delta = choice.get("delta") or {}
                    if delta.get("content") or delta.get("reasoning_content"):
                        if first_token_at is None:
                            first_token_at = time.monotonic()
                        chunks += 1
        finished = time.monotonic()

        self.assertIsNotNone(first_token_at, "no reasoning or answer tokens were streamed")
        ttft = first_token_at - started
        decode_seconds = finished - first_token_at
        output_tokens = (usage or {}).get("completion_tokens") or chunks
        decode_tps = output_tokens / decode_seconds if decode_seconds > 0 else None
        # Client-side decode timing is only as good as the client's flushing.
        # If the transport delivers several frames at once, the arithmetic here
        # reports an impossible rate. Record it as client-observed and record
        # the daemon's own figure beside it, which is measured at the token
        # source and is the number to trust.
        daemon = self.inference()["rolling"]
        record(
            "streaming_throughput",
            client_ttft_s=round(ttft, 3),
            client_decode_s=round(decode_seconds, 3),
            output_tokens=output_tokens,
            client_decode_tps=round(decode_tps, 2) if decode_tps else None,
            daemon_decode_tps_p50=daemon["decodeTpsP50"],
            daemon_ttft_p50_ms=daemon["ttftP50Ms"],
        )

        # Sanity floors only: a real Apple-silicon decode is far above this,
        # but pinning a true number would encode one machine's thermal state.
        self.assertGreater(output_tokens, 0)
        self.assertLess(ttft, TIMEOUT, "first token never arrived within the timeout")
        if decode_tps is not None:
            self.assertGreater(decode_tps, 0.5, "decode throughput collapsed")

    def test_daemon_telemetry_matches_observed_work(self) -> None:
        before = self.inference()["rolling"]
        self.chat(messages=[{"role": "user", "content": "hi"}], max_tokens=ANSWER_TOKENS)
        after = self.inference()

        self.assertEqual(
            after["rolling"]["requestsCompleted"],
            before["requestsCompleted"] + 1,
        )
        self.assertTrue(after["resident"], "weights are not resident after a turn")
        self.assertGreater(after["residentBytes"], 0)
        self.assertIsNotNone(after["rolling"]["latencyP50Ms"])
        record(
            "daemon_rolling",
            ttft_p50_ms=after["rolling"]["ttftP50Ms"],
            decode_tps_p50=after["rolling"]["decodeTpsP50"],
            latency_p50_ms=after["rolling"]["latencyP50Ms"],
            resident_gb=round(after["residentBytes"] / 1024**3, 2),
        )

    def test_metrics_endpoint_reflects_real_work(self) -> None:
        self.chat(messages=[{"role": "user", "content": "hi"}], max_tokens=ANSWER_TOKENS)
        body = self.client.get("/metrics").text
        self.assertIn('laguna_requests_total{outcome="completed"}', body)
        self.assertIn("laguna_model_resident 1", body)


@requires_live_daemon
class LiveConcurrencyAndCancellationTests(LiveDaemonTestCase):
    def test_client_disconnect_frees_the_generation_slot(self) -> None:
        """An abandoned stream must not strand the single GPU slot."""
        with self.client.stream(
            "POST",
            "/v1/chat/completions",
            json={
                "model": self.model,
                "messages": [
                    {"role": "user", "content": "Write a very long essay about the sea."}
                ],
                "stream": True,
                "max_tokens": 2000,
            },
        ) as response:
            self.assertEqual(response.status_code, 200)
            for line in response.iter_lines():
                if line.startswith("data: "):
                    break  # Walk away mid-generation.

        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            runtime = self.client.get("/health").json()["responses"].get("runtime") or {}
            if (
                runtime.get("inflight_generations") == 0
                and runtime.get("queued_generations") == 0
                and runtime.get("generation_slot_available") is True
            ):
                break
            time.sleep(0.5)
        else:
            self.fail("the generation slot was never released after disconnect")

        # The daemon must still serve the next request normally.
        follow_up = self.chat(
            messages=[{"role": "user", "content": "Reply with: ok"}], max_tokens=ANSWER_TOKENS
        )
        self.assertTrue((follow_up["choices"][0]["message"]["content"] or "").strip())

    def test_responses_cancel_endpoint_stops_a_background_turn(self) -> None:
        created = self.responses(
            input="Write an extremely long essay about the sea.",
            background=True,
            max_output_tokens=LONG_TOKENS,
        )
        self.assertIn(created["status"], {"queued", "in_progress"})
        cancelled = self.client.post(f"/v1/responses/{created['id']}/cancel")
        self.assertEqual(cancelled.status_code, 200)
        self.assertEqual(cancelled.json()["status"], "cancelled")

    def test_concurrent_requests_are_serialized_not_dropped(self) -> None:
        """One GPU slot, but every admitted request must still complete."""
        import concurrent.futures

        def ask(index: int) -> dict[str, Any]:
            return self.chat(
                messages=[{"role": "user", "content": f"Reply with the number {index}."}],
                max_tokens=ANSWER_TOKENS,
            )

        started = time.monotonic()
        with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
            results = list(pool.map(ask, range(3)))
        elapsed = time.monotonic() - started

        self.assertEqual(len(results), 3)
        for body in results:
            self.assertTrue((body["choices"][0]["message"]["content"] or "").strip())
        record("concurrency_3", total_s=round(elapsed, 3))


@requires_live_daemon
class LiveResidencyTests(LiveDaemonTestCase):
    """Load, release, and reload without ever restarting the daemon."""

    def test_unload_then_reload_on_the_next_prompt(self) -> None:
        self.chat(messages=[{"role": "user", "content": "hi"}], max_tokens=ANSWER_TOKENS)
        self.assertTrue(self.inference()["resident"])

        unload = self.client.post("/v1/synth/model/unload")
        self.assertEqual(unload.status_code, 200, unload.text)
        self.assertTrue(unload.json()["unloaded"])

        after_unload = self.inference()
        self.assertFalse(after_unload["resident"])
        self.assertEqual(after_unload["residentBytes"], 0)
        # Aggregates survive eviction; only residency is released.
        self.assertGreater(after_unload["rolling"]["requestsCompleted"], 0)

        # The daemon is still alive and reloads on demand.
        health = self.client.get("/health")
        self.assertEqual(health.status_code, 200)
        self.assertIsNone(health.json()["loadedModel"])

        started = time.monotonic()
        reloaded = self.chat(
            messages=[{"role": "user", "content": "Reply with: ok"}], max_tokens=ANSWER_TOKENS
        )
        record("cold_reload_s", seconds=round(time.monotonic() - started, 3))
        self.assertTrue((reloaded["choices"][0]["message"]["content"] or "").strip())
        self.assertTrue(self.inference()["resident"])


if __name__ == "__main__":
    unittest.main()
