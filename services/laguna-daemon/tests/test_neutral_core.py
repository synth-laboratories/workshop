from __future__ import annotations

import asyncio
import inspect
import unittest
from pathlib import Path

from laguna_daemon.responses_api.backends.fake import FakeBackend
from laguna_daemon.responses_api.backends.protocol import CompiledTurn
from laguna_daemon.responses_api.capabilities import DEFAULT_MAX_OUTPUT_TOKENS
from laguna_daemon.responses_api.compiler import (
    compile_messages,
    compile_turn,
    items_to_messages,
)
from laguna_daemon.responses_api.errors import ResponsesError
from laguna_daemon.responses_api.runner import TurnRunner
from laguna_daemon.responses_api.validation import normalize_request


# Item kinds and envelope concepts that belong to the Responses wire surface.
# The neutral core must not know any of them, or Chat becomes a second-class
# citizen expressed in Responses terms.
RESPONSES_ONLY_TOKENS = (
    "previous_response_id",
    "response_shell",
    "function_call_output",
    "custom_tool_call_output",
    "apply_patch_call_output",
    "tool_search_output",
    "input_text",
    "output_text",
    "instructions",
)


class NeutralCoreContractTests(unittest.TestCase):
    """The shared core must stay free of any single protocol's concepts."""

    def test_runner_module_has_no_responses_concepts(self) -> None:
        source = Path(inspect.getfile(TurnRunner)).read_text(encoding="utf-8")
        for token in RESPONSES_ONLY_TOKENS:
            self.assertNotIn(
                token,
                source,
                msg=f"runner.py leaked the Responses-only concept {token!r}",
            )

    def test_compile_messages_has_no_responses_concepts(self) -> None:
        source = inspect.getsource(compile_messages)
        for token in RESPONSES_ONLY_TOKENS:
            self.assertNotIn(
                token,
                source,
                msg=f"compile_messages leaked the Responses-only concept {token!r}",
            )

    def test_compile_messages_needs_no_responses_request(self) -> None:
        """A Chat turn must compile without fabricating a Responses request."""
        turn = compile_messages(
            [{"role": "user", "content": "hello"}],
            model="test-model",
            generation_id="gen_1",
        )
        self.assertIsInstance(turn, CompiledTurn)
        self.assertEqual(turn.request, {})
        self.assertEqual(turn.context_items, [])
        self.assertEqual(turn.model, "test-model")
        self.assertEqual(turn.messages[-1]["content"], "hello")
        self.assertEqual(turn.max_output_tokens, DEFAULT_MAX_OUTPUT_TOKENS)

    def test_responses_default_allows_complete_coding_tool_calls(self) -> None:
        turn = compile_turn({"model": "m", "input": []}, [], "gen_default")
        self.assertEqual(turn.max_output_tokens, DEFAULT_MAX_OUTPUT_TOKENS)

    def test_responses_front_end_delegates_to_the_neutral_core(self) -> None:
        """compile_turn must be a thin front-end, not a parallel implementation."""
        request = {
            "model": "test-model",
            "input": [],
            "temperature": 0.5,
            "top_p": 0.9,
            "max_output_tokens": 64,
            "prompt_cache_key": "abc",
            "reasoning": {"effort": "high"},
        }
        items = [
            {
                "type": "message",
                "role": "user",
                "content": [{"type": "input_text", "text": "hi"}],
            }
        ]
        turn = compile_turn(request, items, "gen_2")
        self.assertEqual(turn.temperature, 0.5)
        self.assertEqual(turn.top_p, 0.9)
        self.assertEqual(turn.max_output_tokens, 64)
        self.assertEqual(turn.prompt_cache_key, "abc")
        self.assertTrue(turn.enable_thinking)
        # Provenance is retained for the remote passthrough backend.
        self.assertIs(turn.request, request)
        self.assertIs(turn.context_items, items)

    def test_absent_reasoning_defaults_to_thinking_on(self) -> None:
        """/v1/models advertises default high; a bare request must honor it."""
        turn = compile_turn({"model": "m", "input": []}, [], "gen_default_think")
        self.assertTrue(turn.enable_thinking)

    def test_unsupported_reasoning_efforts_are_a_typed_error(self) -> None:
        """One thinking mode exists; other spellings must not silently run it."""
        for effort in ("low", "medium", "xhigh", "minimal"):
            with self.subTest(effort=effort):
                with self.assertRaises(ResponsesError) as caught:
                    normalize_request(
                        {"model": "m", "input": [], "reasoning": {"effort": effort}},
                        default_model="m",
                    )
                self.assertEqual(caught.exception.code, "unsupported_reasoning_effort")

    def test_top_k_flows_from_request_to_compiled_turn(self) -> None:
        request = normalize_request(
            {"model": "m", "input": [], "top_k": 20}, default_model="m"
        )
        turn = compile_turn(request, [], "gen_top_k")
        self.assertEqual(turn.top_k, 20)

    def test_top_k_is_validated(self) -> None:
        for bad in ("twenty", -1, 10_000, True):
            with self.subTest(top_k=bad):
                with self.assertRaises(ResponsesError):
                    normalize_request(
                        {"model": "m", "input": [], "top_k": bad}, default_model="m"
                    )

    def test_tool_directives_merge_into_the_callers_system_message(self) -> None:
        """Laguna's template promotes messages[0] into the header <system>
        block. Tool directives must extend the caller's system prompt, never
        displace it from that header.
        """
        messages = [
            {"role": "system", "content": "You are Codex."},
            {"role": "user", "content": "hi"},
        ]
        turn = compile_messages(
            messages,
            model="m",
            generation_id="gen_directives",
            tools=[{"type": "function", "name": "do_it", "parameters": {"type": "object"}}],
        )
        head = turn.messages[0]
        self.assertEqual(head["role"], "system")
        self.assertTrue(head["content"].startswith("You are Codex."))
        self.assertIn("only callable tool names", head["content"])
        self.assertEqual(
            sum(1 for message in turn.messages if message["role"] == "system"), 1
        )

    def test_tool_directives_without_a_system_message_add_exactly_one(self) -> None:
        turn = compile_messages(
            [{"role": "user", "content": "hi"}],
            model="m",
            generation_id="gen_directives_bare",
            tools=[{"type": "function", "name": "do_it", "parameters": {"type": "object"}}],
            tool_choice="required",
        )
        systems = [m for m in turn.messages if m["role"] == "system"]
        self.assertEqual(len(systems), 1)
        self.assertIn("only callable tool names", systems[0]["content"])
        self.assertIn("You must call the tool", systems[0]["content"])

    def test_structured_output_renders_with_thinking_closed(self) -> None:
        """A JSON grammar owns every sampled token; an open <think> span in
        the prompt could never be closed."""
        turn = compile_messages(
            [{"role": "user", "content": "hi"}],
            model="m",
            generation_id="gen_structured",
            structured_format={"type": "json_object"},
            enable_thinking=True,
        )
        self.assertFalse(turn.enable_thinking)

    def test_reasoning_effort_none_disables_thinking(self) -> None:
        turn = compile_turn(
            {"model": "m", "input": [], "reasoning": {"effort": "none"}}, [], "gen_3"
        )
        self.assertFalse(turn.enable_thinking)

    def test_legacy_max_reasoning_normalizes_to_supported_high(self) -> None:
        request = normalize_request(
            {"model": "m", "input": [], "reasoning": {"effort": "max"}},
            default_model="m",
        )
        self.assertEqual(request["reasoning"]["effort"], "high")

    def test_responses_history_preserves_reasoning_for_the_next_assistant_item(self) -> None:
        messages = items_to_messages(
            {},
            [
                {
                    "type": "reasoning",
                    "summary": [
                        {"type": "summary_text", "text": "Inspect before editing."}
                    ],
                },
                {
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "read_file",
                    "arguments": '{"path":"README.md"}',
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "contents",
                },
                {
                    "type": "message",
                    "role": "user",
                    "content": [{"type": "input_text", "text": "continue"}],
                },
            ],
        )
        assistant = next(message for message in messages if message["role"] == "assistant")
        self.assertEqual(assistant["reasoning_content"], "Inspect before editing.")
        self.assertEqual(assistant["tool_calls"][0]["function"]["name"], "read_file")

    def test_prompt_cache_key_is_a_neutral_field(self) -> None:
        """The backend reads the dataclass field, not a Responses request dict."""
        turn = compile_messages(
            [{"role": "user", "content": "hi"}],
            model="m",
            generation_id="gen_4",
            prompt_cache_key="cache-1",
        )
        self.assertEqual(turn.prompt_cache_key, "cache-1")


class TurnRunnerTests(unittest.IsolatedAsyncioTestCase):
    class _Renderer:
        def __init__(self) -> None:
            self.started = False
            self.events: list[str] = []
            self.finish_reason: str | None = None
            self.failed: Exception | None = None

        async def start(self) -> None:
            self.started = True

        async def consume(self, model_event):  # type: ignore[no-untyped-def]
            self.events.append(model_event.kind)
            return model_event.finish_reason if model_event.kind == "finish" else None

        async def complete(self, finish_reason: str):  # type: ignore[no-untyped-def]
            self.finish_reason = finish_reason
            return {"status": "completed", "finish_reason": finish_reason}

        async def fail(self, error):  # type: ignore[no-untyped-def]
            self.failed = error
            return {"status": "failed"}

    async def test_drive_streams_any_renderer(self) -> None:
        backend = FakeBackend()
        runner = TurnRunner(backend)
        turn = compile_messages(
            [{"role": "user", "content": "hello"}],
            model="m",
            generation_id="gen_drive",
        )
        renderer = self._Renderer()
        result = await runner.drive(turn, renderer)
        self.assertTrue(renderer.started)
        self.assertEqual(result["status"], "completed")
        self.assertIn("text_delta", renderer.events)
        self.assertIn("usage", renderer.events)

    async def test_slot_registers_before_generation_and_clears_after(self) -> None:
        """Registration must span compilation so a cancel during a long prompt
        compile is still able to find and stop the work."""
        runner = TurnRunner(FakeBackend())
        self.assertFalse(runner.is_active("resp_1"))
        async with runner.slot("resp_1", "gen_5"):
            self.assertTrue(runner.is_active("resp_1"))
        self.assertFalse(runner.is_active("resp_1"))

    async def test_cancel_propagates_to_backend_and_task(self) -> None:
        runner = TurnRunner(FakeBackend())
        entered = asyncio.Event()

        async def work() -> None:
            async with runner.slot("resp_2", "gen_6"):
                entered.set()
                await asyncio.sleep(30)

        task = asyncio.create_task(work())
        await entered.wait()
        self.assertTrue(await runner.cancel("resp_2"))
        with self.assertRaises(asyncio.CancelledError):
            await task
        self.assertFalse(runner.is_active("resp_2"))

    async def test_cancel_of_unknown_key_is_false(self) -> None:
        runner = TurnRunner(FakeBackend())
        self.assertFalse(await runner.cancel("nope"))

    async def test_slot_releases_registration_on_error(self) -> None:
        runner = TurnRunner(FakeBackend())
        with self.assertRaises(ValueError):
            async with runner.slot("resp_3", "gen_7"):
                raise ValueError("boom")
        self.assertFalse(runner.is_active("resp_3"))


if __name__ == "__main__":
    unittest.main()
