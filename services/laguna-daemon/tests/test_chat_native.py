from __future__ import annotations

import json
import tempfile
import time
import unittest
from pathlib import Path

from fastapi.testclient import TestClient

from laguna_daemon.app import build_app
from laguna_daemon.config import LagunaConfig


MODEL = "poolside/Laguna-XS-2.1-NVFP4-mlx"

WEATHER_TOOL = {
    "type": "function",
    "function": {
        "name": "get.Current-Weather",
        "description": "Look up the weather.",
        "parameters": {
            "type": "object",
            "properties": {"city": {"type": "string"}},
            "required": ["city"],
            "additionalProperties": False,
        },
    },
}


def _config(tmp: Path) -> LagunaConfig:
    models = tmp / "models"
    models.mkdir(parents=True, exist_ok=True)
    data = tmp / "data"
    data.mkdir(parents=True, exist_ok=True)
    return LagunaConfig(
        host="127.0.0.1",
        port=7333,
        backend="mock",
        api_key=None,
        models_dir=models,
        default_model=MODEL,
        model=MODEL,
        revision=None,
        draft_model=None,
        adapter=None,
        external_url=None,
        upstream_api_key=None,
        data_dir=data,
        auto_load=True,
        idle_unload_after_seconds=900,
        context_length=262144,
        started_at=time.time(),
    )


def parse_chunks(body: str) -> list[dict]:
    chunks = []
    for line in body.splitlines():
        if not line.startswith("data: "):
            continue
        payload = line[len("data: ") :]
        if payload == "[DONE]":
            continue
        chunks.append(json.loads(payload))
    return chunks


def reassemble(chunks: list[dict]) -> dict:
    """Rebuild the final message from a chunk stream, the way a client would."""
    content = ""
    reasoning = ""
    tool_calls: dict[int, dict] = {}
    finish_reason = None
    usage = None
    for chunk in chunks:
        if chunk.get("usage") is not None:
            usage = chunk["usage"]
        for choice in chunk.get("choices") or []:
            delta = choice.get("delta") or {}
            content += delta.get("content") or ""
            reasoning += delta.get("reasoning_content") or ""
            for call in delta.get("tool_calls") or []:
                slot = tool_calls.setdefault(
                    call["index"],
                    {"id": call.get("id"), "type": "function", "function": {"name": "", "arguments": ""}},
                )
                function = call.get("function") or {}
                if function.get("name"):
                    slot["function"]["name"] = function["name"]
                slot["function"]["arguments"] += function.get("arguments") or ""
            if choice.get("finish_reason"):
                finish_reason = choice["finish_reason"]
    return {
        "content": content or None,
        "reasoning_content": reasoning,
        "tool_calls": [tool_calls[key] for key in sorted(tool_calls)],
        "finish_reason": finish_reason,
        "usage": usage,
    }


class ChatSurfaceTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory(prefix="synth-chat-")
        self.addCleanup(self.temp.cleanup)
        self.client = TestClient(build_app(_config(Path(self.temp.name))))

    def chat(self, **body):
        return self.client.post("/v1/chat/completions", json={"model": MODEL, **body})

    # -- shape --------------------------------------------------------------

    def test_non_stream_returns_a_chat_completion(self) -> None:
        response = self.chat(messages=[{"role": "user", "content": "hello"}])
        self.assertEqual(response.status_code, 200)
        body = response.json()
        self.assertEqual(body["object"], "chat.completion")
        self.assertTrue(body["id"].startswith("chatcmpl"))
        self.assertEqual(body["model"], MODEL)
        choice = body["choices"][0]
        self.assertEqual(choice["index"], 0)
        self.assertEqual(choice["message"]["role"], "assistant")
        self.assertEqual(choice["finish_reason"], "stop")
        self.assertIn("prompt_tokens", body["usage"])
        self.assertIn("cached_tokens", body["usage"]["prompt_tokens_details"])

    def test_stream_emits_chat_completion_chunks(self) -> None:
        response = self.chat(
            messages=[{"role": "user", "content": "hello"}], stream=True
        )
        self.assertEqual(response.status_code, 200)
        chunks = parse_chunks(response.text)
        self.assertTrue(chunks)
        self.assertTrue(all(c["object"] == "chat.completion.chunk" for c in chunks))
        self.assertEqual(chunks[0]["choices"][0]["delta"]["role"], "assistant")
        self.assertIn("[DONE]", response.text)

    def test_reasoning_is_separated_from_content(self) -> None:
        """Hidden model reasoning must never leak into assistant content."""
        body = self.chat(messages=[{"role": "user", "content": "hello"}]).json()
        message = body["choices"][0]["message"]
        self.assertIn("reasoning_content", message)
        self.assertNotIn("Checked the request contract", message["content"] or "")

    # -- the equivalence guarantee -----------------------------------------

    def test_stream_and_non_stream_reconstruct_the_same_result(self) -> None:
        messages = [{"role": "user", "content": "hello there"}]
        direct = self.chat(messages=messages).json()
        streamed = reassemble(
            parse_chunks(
                self.chat(
                    messages=messages,
                    stream=True,
                    stream_options={"include_usage": True},
                ).text
            )
        )
        message = direct["choices"][0]["message"]
        self.assertEqual(streamed["content"], message["content"])
        self.assertEqual(
            streamed["reasoning_content"], message.get("reasoning_content", "")
        )
        self.assertEqual(streamed["finish_reason"], direct["choices"][0]["finish_reason"])
        self.assertEqual(streamed["usage"], direct["usage"])

    def test_stream_and_non_stream_agree_on_tool_calls(self) -> None:
        messages = [{"role": "user", "content": "weather in Paris?"}]
        direct = self.chat(messages=messages, tools=[WEATHER_TOOL]).json()
        streamed = reassemble(
            parse_chunks(
                self.chat(messages=messages, tools=[WEATHER_TOOL], stream=True).text
            )
        )
        expected = direct["choices"][0]["message"]["tool_calls"]
        self.assertEqual(len(streamed["tool_calls"]), len(expected))
        self.assertEqual(
            streamed["tool_calls"][0]["function"]["name"],
            expected[0]["function"]["name"],
        )
        self.assertEqual(
            streamed["tool_calls"][0]["function"]["arguments"],
            expected[0]["function"]["arguments"],
        )
        self.assertEqual(streamed["finish_reason"], "tool_calls")

    # -- tool calling -------------------------------------------------------

    def test_tool_call_restores_the_callers_original_name(self) -> None:
        """The model sees a sanitized name; the caller must get theirs back."""
        body = self.chat(
            messages=[{"role": "user", "content": "weather?"}], tools=[WEATHER_TOOL]
        ).json()
        choice = body["choices"][0]
        self.assertEqual(choice["finish_reason"], "tool_calls")
        call = choice["message"]["tool_calls"][0]
        self.assertEqual(call["type"], "function")
        self.assertEqual(call["function"]["name"], "get.Current-Weather")
        self.assertIsInstance(call["function"]["arguments"], str)
        json.loads(call["function"]["arguments"])

    def test_tool_output_continues_the_conversation(self) -> None:
        first = self.chat(
            messages=[{"role": "user", "content": "weather?"}], tools=[WEATHER_TOOL]
        ).json()
        call = first["choices"][0]["message"]["tool_calls"][0]
        second = self.chat(
            messages=[
                {"role": "user", "content": "weather?"},
                {
                    "role": "assistant",
                    "content": None,
                    "tool_calls": [call],
                },
                {
                    "role": "tool",
                    "tool_call_id": call["id"],
                    "content": '{"temp_c": 17}',
                },
            ],
            tools=[WEATHER_TOOL],
        ).json()
        choice = second["choices"][0]
        self.assertEqual(choice["finish_reason"], "stop")
        self.assertIsNone(choice["message"].get("tool_calls"))
        self.assertTrue(choice["message"]["content"])

    def test_tool_choice_none_suppresses_tools(self) -> None:
        body = self.chat(
            messages=[{"role": "user", "content": "weather?"}],
            tools=[WEATHER_TOOL],
            tool_choice="none",
        ).json()
        choice = body["choices"][0]
        self.assertEqual(choice["finish_reason"], "stop")
        self.assertIsNone(choice["message"].get("tool_calls"))

    def test_named_tool_choice_selects_that_tool(self) -> None:
        body = self.chat(
            messages=[{"role": "user", "content": "weather?"}],
            tools=[WEATHER_TOOL],
            tool_choice={"type": "function", "function": {"name": "get.Current-Weather"}},
        ).json()
        call = body["choices"][0]["message"]["tool_calls"][0]
        self.assertEqual(call["function"]["name"], "get.Current-Weather")

    def test_named_tool_choice_for_an_unknown_tool_is_rejected(self) -> None:
        response = self.chat(
            messages=[{"role": "user", "content": "weather?"}],
            tools=[WEATHER_TOOL],
            tool_choice={"type": "function", "function": {"name": "nope"}},
        )
        self.assertEqual(response.status_code, 400)

    # -- structured output --------------------------------------------------

    def test_json_object_response_format(self) -> None:
        body = self.chat(
            messages=[{"role": "user", "content": "json please"}],
            response_format={"type": "json_object"},
        ).json()
        json.loads(body["choices"][0]["message"]["content"])

    def test_json_schema_response_format(self) -> None:
        schema = {
            "type": "object",
            "properties": {"city": {"type": "string"}},
            "required": ["city"],
            "additionalProperties": False,
        }
        body = self.chat(
            messages=[{"role": "user", "content": "json please"}],
            response_format={
                "type": "json_schema",
                "json_schema": {"name": "city", "schema": schema},
            },
        ).json()
        parsed = json.loads(body["choices"][0]["message"]["content"])
        self.assertIn("city", parsed)

    # -- explicit rejection of what cannot be honored -----------------------

    def test_unsupported_fields_are_rejected_with_stable_codes(self) -> None:
        cases = {
            "n": 2,
            "logprobs": True,
            "top_logprobs": 5,
            "logit_bias": {"1": 1},
            "seed": 7,
            "modalities": ["audio"],
            "prediction": {"type": "content", "content": "x"},
            "web_search_options": {},
            "stop": ["\n"],
            "presence_penalty": 0.5,
            "frequency_penalty": 0.5,
            "store": True,
            "service_tier": "flex",
        }
        for field, value in cases.items():
            with self.subTest(field=field):
                response = self.chat(
                    messages=[{"role": "user", "content": "hi"}], **{field: value}
                )
                self.assertEqual(response.status_code, 400)
                error = response.json()["error"]
                self.assertEqual(error["code"], "unsupported_chat_field")
                self.assertEqual(error["param"], field)

    def test_streaming_requests_are_validated_before_the_stream_opens(self) -> None:
        """A bad streaming request must fail with a real status, not a frame.

        The frame iterator is lazy, so validation has to be awaited at the
        boundary; otherwise nothing runs until the first pull, by which point
        200 has already been sent and the error can only be smuggled inside
        the stream.
        """
        for field, value in (("logprobs", True), ("n", 2), ("stop", ["\n"])):
            with self.subTest(field=field):
                response = self.chat(
                    messages=[{"role": "user", "content": "hi"}],
                    stream=True,
                    **{field: value},
                )
                self.assertEqual(response.status_code, 400)
                self.assertEqual(
                    response.headers["content-type"].split(";")[0], "application/json"
                )
                self.assertEqual(
                    response.json()["error"]["code"], "unsupported_chat_field"
                )

    def test_malformed_streaming_requests_also_fail_with_a_status(self) -> None:
        response = self.client.post(
            "/v1/chat/completions",
            json={"model": MODEL, "messages": [], "stream": True},
        )
        self.assertEqual(response.status_code, 400)
        self.assertEqual(response.json()["error"]["code"], "invalid_request")

    def test_zero_penalties_are_accepted(self) -> None:
        """Rejection is about fields that would change output, not their defaults."""
        response = self.chat(
            messages=[{"role": "user", "content": "hi"}],
            presence_penalty=0,
            frequency_penalty=0,
        )
        self.assertEqual(response.status_code, 200)

    def test_responses_only_tool_kinds_are_rejected(self) -> None:
        for kind in ("custom", "namespace", "mcp", "shell", "apply_patch"):
            with self.subTest(kind=kind):
                response = self.chat(
                    messages=[{"role": "user", "content": "hi"}],
                    tools=[{"type": kind, "name": "x"}],
                )
                self.assertEqual(response.status_code, 400)
                self.assertEqual(
                    response.json()["error"]["code"], "unsupported_chat_field"
                )

    def test_non_text_content_parts_are_rejected(self) -> None:
        response = self.chat(
            messages=[
                {
                    "role": "user",
                    "content": [
                        {"type": "image_url", "image_url": {"url": "http://x/y.png"}}
                    ],
                }
            ]
        )
        self.assertEqual(response.status_code, 400)
        self.assertEqual(response.json()["error"]["code"], "unsupported_chat_field")

    def test_malformed_requests_are_rejected(self) -> None:
        for body in (
            {"messages": []},
            {"messages": [{"role": "wizard", "content": "hi"}]},
            {"messages": [{"role": "tool", "content": "x"}]},
            {"messages": [{"role": "user"}]},
        ):
            with self.subTest(body=body):
                response = self.client.post(
                    "/v1/chat/completions", json={"model": MODEL, **body}
                )
                self.assertEqual(response.status_code, 400)
                self.assertEqual(response.json()["error"]["code"], "invalid_request")

    def test_text_content_parts_are_accepted(self) -> None:
        response = self.chat(
            messages=[{"role": "user", "content": [{"type": "text", "text": "hi"}]}]
        )
        self.assertEqual(response.status_code, 200)

    def test_model_aliases_resolve_to_the_served_model(self) -> None:
        body = self.client.post(
            "/v1/chat/completions",
            json={
                "model": "laguna-xs-2.1",
                "messages": [{"role": "user", "content": "hi"}],
            },
        ).json()
        self.assertEqual(body["model"], MODEL)

    # -- shared runtime -----------------------------------------------------

    def test_chat_is_stateless_and_creates_no_stored_response(self) -> None:
        """Chat must not write to the Responses store."""
        body = self.chat(messages=[{"role": "user", "content": "hi"}]).json()
        self.assertEqual(self.client.get(f"/v1/responses/{body['id']}").status_code, 404)

    def test_both_surfaces_share_one_runner(self) -> None:
        """Shared admission is the whole point of the neutral core."""
        app = self.client.app
        self.assertIs(
            app.state.chat_service.runner,
            app.state.responses_service.coordinator.runner,
        )
        self.assertIs(
            app.state.chat_service.backend, app.state.responses_service.backend
        )


if __name__ == "__main__":
    unittest.main()
