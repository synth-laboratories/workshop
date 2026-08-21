from __future__ import annotations

import asyncio
import json
import time
from collections import OrderedDict
from typing import Any, AsyncIterator

from ..capabilities import ModelCapabilities
from ..compiler import compile_messages, compile_turn
from ..errors import ResponsesError
from ..ids import new_id
from ..telemetry import GenerationTiming
from .protocol import CompiledTurn, ModelEvent, TokenUsageEstimate


def _schema_example(schema: dict[str, Any]) -> Any:
    if "const" in schema:
        return schema["const"]
    enum = schema.get("enum")
    if isinstance(enum, list) and enum:
        return enum[0]
    kind = schema.get("type")
    if isinstance(kind, list):
        kind = next((entry for entry in kind if entry != "null"), "null")
    if kind == "object" or isinstance(schema.get("properties"), dict):
        properties = schema.get("properties") or {}
        required = set(schema.get("required") or properties.keys())
        return {
            key: _schema_example(value)
            for key, value in properties.items()
            if key in required and isinstance(value, dict)
        }
    if kind == "array":
        items = schema.get("items")
        return [_schema_example(items)] if isinstance(items, dict) else []
    if kind in {"integer", "number"}:
        return schema.get("minimum", 0)
    if kind == "boolean":
        return False
    if kind == "null":
        return None
    return "example"


class FakeBackend:
    """Deterministic protocol backend; it never loads model weights."""

    def __init__(self, *, context_length: int = 262_144) -> None:
        self._capabilities = ModelCapabilities(
            images=True,
            files=True,
            video=True,
            audio=False,
            hosted_mcp=False,
            context_length=context_length,
        )
        self._cancelled: set[str] = set()
        self._generations: dict[str, GenerationTiming] = {}
        self._recent_generations: OrderedDict[str, GenerationTiming] = OrderedDict()
        self._loaded = False
        self._last_used_at = time.time()
        self.adapter_path = None

    async def capabilities(self, model: str) -> ModelCapabilities:
        return self._capabilities

    async def load(self) -> None:
        """Explicit residency request, mirroring the native backend."""
        self._loaded = True
        self._last_used_at = time.time()

    async def set_adapter(self, adapter_path: str | None) -> None:
        self.adapter_path = adapter_path

    def set_policy_registry(self, registry: Any) -> None:
        self._policies = registry

    async def _ensure_policy(self, model: str | None) -> None:
        registry = getattr(self, "_policies", None)
        if registry is None:
            return
        from ..policies import PolicyError

        try:
            policy = registry.resolve(model)
        except PolicyError as error:
            raise ResponsesError(
                "model_not_found",
                str(error),
                404,
                error_type="invalid_request_error",
            ) from error
        self.attached_policy = policy.model_id
        self.adapter_path = None if policy.is_base else str(policy.adapter_path)
        self._loaded = False

    async def compile(
        self,
        request: dict[str, Any],
        context_items: list[dict[str, Any]],
        generation_id: str,
    ) -> CompiledTurn:
        return compile_turn(
            request,
            context_items,
            generation_id,
            defaults=getattr(self, "sampling_defaults", None),
        )

    async def compile_messages(self, **kwargs: Any) -> CompiledTurn:
        return compile_messages(**kwargs)

    async def count_tokens(self, turn: CompiledTurn) -> TokenUsageEstimate:
        serialized = json.dumps(turn.messages, ensure_ascii=False)
        return TokenUsageEstimate(max(1, (len(serialized) + 3) // 4))

    async def stream(self, turn: CompiledTurn) -> AsyncIterator[ModelEvent]:
        await self._ensure_policy(turn.model)
        usage = await self.count_tokens(turn)
        self._loaded = True
        self._last_used_at = time.time()
        timing = GenerationTiming(
            generation_id=turn.generation_id, queued_at=time.monotonic()
        )
        timing.admitted_at = timing.queued_at
        timing.compiled_at = time.monotonic()
        timing.prompt_tokens = usage.input_tokens
        timing.phase = "prefill"
        self._generations[turn.generation_id] = timing
        try:
            async for event in self._generate(turn, usage, timing):
                if event.kind in {"text_delta", "reasoning_delta"} or event.kind.endswith("_call"):
                    if timing.first_token_at is None:
                        timing.first_token_at = time.monotonic()
                        timing.phase = "decode"
                    timing.last_token_at = time.monotonic()
                if event.kind == "usage":
                    timing.output_tokens = int(event.output_tokens or 0)
                    timing.cached_tokens = int(event.metadata.get("cached_tokens") or 0)
                yield event
        finally:
            timing.completed_at = time.monotonic()
            timing.phase = "complete"
            self._generations.pop(turn.generation_id, None)
            self._recent_generations[turn.generation_id] = timing
            while len(self._recent_generations) > 32:
                self._recent_generations.popitem(last=False)

    async def _generate(
        self, turn: CompiledTurn, usage: TokenUsageEstimate, timing: GenerationTiming
    ) -> AsyncIterator[ModelEvent]:
        if turn.generation_id in self._cancelled:
            yield ModelEvent(kind="finish", finish_reason="cancelled")
            return
        if turn.enable_thinking:
            yield ModelEvent(kind="reasoning_delta", delta="Checked the request contract.")

        # A turn that already carries tool output must answer, not call again.
        # `items_to_messages` lowers every Responses `*_call_output` item to a
        # `role: "tool"` message and Chat sends that role directly, so the last
        # message role is the protocol-neutral form of this check.
        last_message = turn.messages[-1] if turn.messages else {}
        continues_tool_output = (
            isinstance(last_message, dict) and last_message.get("role") == "tool"
        )
        user_prompt = next(
            (
                str(message.get("content") or "")
                for message in reversed(turn.messages)
                if message.get("role") == "user"
            ),
            "",
        )
        # `tool_choice: "none"` already empties `bindings` during compilation, so
        # only the prompt-driven suppression needs checking here.
        suppress_tools = "do not call tools" in user_prompt.lower()
        output_tokens = 0
        if turn.bindings and not continues_tool_output and not suppress_tools:
            binding = next(iter(turn.bindings.values()))
            call_id = new_id("call")
            if binding.kind == "custom":
                if binding.original_name == "mcp__synth_containers":
                    raw = json.dumps(
                        {
                            "method": "container_run_and_visualize",
                            "count": 2,
                            "base_url": "http://127.0.0.1:8098",
                            "name": "Craftax Rust",
                        },
                        separators=(",", ":"),
                    )
                else:
                    raw = "example"
                output_tokens = max(1, (len(raw) + 3) // 4)
                yield ModelEvent(
                    kind="custom_tool_call",
                    name=binding.original_name,
                    namespace=binding.namespace,
                    call_id=call_id,
                    input=raw,
                )
            elif binding.kind == "tool_search":
                arguments = json.dumps(
                    {"query": "matching tools", "namespace": binding.original_name},
                    separators=(",", ":"),
                )
                output_tokens = max(1, (len(arguments) + 3) // 4)
                yield ModelEvent(
                    kind="tool_search_call",
                    name=binding.original_name,
                    call_id=call_id,
                    arguments=arguments,
                )
            elif binding.kind in {"shell", "local_shell"}:
                yield ModelEvent(
                    kind="shell_call",
                    name=binding.original_name,
                    call_id=call_id,
                    arguments=json.dumps({"command": "pwd"}),
                )
                output_tokens = 4
            elif binding.kind == "apply_patch":
                yield ModelEvent(
                    kind="apply_patch_call",
                    name=binding.original_name,
                    call_id=call_id,
                    arguments=json.dumps({"patch": "*** Begin Patch\n*** End Patch"}),
                )
                output_tokens = 8
            elif binding.kind == "mcp":
                yield ModelEvent(
                    kind="mcp_call",
                    name=binding.original_name,
                    call_id=call_id,
                    arguments=json.dumps({"name": "example", "arguments": {}}),
                )
                output_tokens = 6
            else:
                arguments = json.dumps(
                    _schema_example(binding.schema or {"type": "object"}),
                    separators=(",", ":"),
                )
                output_tokens = max(1, (len(arguments) + 3) // 4)
                yield ModelEvent(
                    kind="function_call",
                    name=binding.original_name,
                    namespace=binding.namespace,
                    call_id=call_id,
                    arguments=arguments,
                )
        else:
            format_spec = turn.structured_format or {}
            if format_spec.get("type") == "json_schema":
                schema = format_spec.get("schema")
                text = json.dumps(
                    _schema_example(schema if isinstance(schema, dict) else {}),
                    separators=(",", ":"),
                )
            elif format_spec.get("type") == "json_object":
                text = '{"result":"ok"}'
            elif continues_tool_output:
                text = "Tool output received."
            else:
                text = f"Synth Laguna native mock. Received: {user_prompt[:240]}"
            for token in text.split(" "):
                if turn.generation_id in self._cancelled:
                    yield ModelEvent(kind="finish", finish_reason="cancelled")
                    return
                delta = token if output_tokens == 0 else f" {token}"
                output_tokens += 1
                yield ModelEvent(kind="text_delta", delta=delta)
                delay = ((turn.request.get("x_synth") or {}).get("fake_delay_ms") or 0)
                await asyncio.sleep(float(delay) / 1000)
        yield ModelEvent(
            kind="usage",
            input_tokens=usage.input_tokens,
            output_tokens=output_tokens,
            reasoning_tokens=5 if turn.enable_thinking else 0,
        )
        yield ModelEvent(kind="finish", finish_reason="stop")

    async def cancel(self, generation_id: str) -> None:
        self._cancelled.add(generation_id)

    async def close(self) -> None:
        self._cancelled.clear()

    # -- telemetry surface, mirroring the native backend ---------------------

    def diagnostics(self) -> dict[str, Any]:
        return {
            "loaded": self._loaded,
            "loading": False,
            "inflight_generations": len(self._generations),
            "max_inflight_generations": 9,
            "generation_slot_available": not self._generations,
            "queued_generations": 0,
            "generation_phases": {
                key: timing.phase for key, timing in self._generations.items()
            },
        }

    def memory_bytes(self) -> int | None:
        """The deterministic backend holds no weights, so it holds no memory."""
        return 0

    def generation_metrics(self, generation_id: str) -> GenerationTiming | None:
        timing = self._generations.get(generation_id)
        if timing is not None:
            return timing
        return self._recent_generations.get(generation_id)

    def active_generation(self) -> GenerationTiming | None:
        return next(iter(self._generations.values()), None)

    def queue_state(self) -> dict[str, int]:
        return {"depth": len(self._generations), "capacity": 9}

    def residency(self, idle_unload_after_seconds: int) -> dict[str, Any]:
        last_used_at = int(self._last_used_at * 1000)
        return {
            "loaded": self._loaded,
            "idle_seconds": max(0, int(time.time() - self._last_used_at)),
            "last_used_at": last_used_at,
            "free_at": (
                last_used_at + idle_unload_after_seconds * 1000
                if self._loaded and idle_unload_after_seconds > 0
                else None
            ),
        }

    async def unload_if_idle(self, idle_unload_after_seconds: int) -> bool:
        if idle_unload_after_seconds <= 0 or not self._loaded:
            return False
        if time.time() - self._last_used_at < idle_unload_after_seconds:
            return False
        if self._generations:
            return False
        self._loaded = False
        return True

    async def unload(self) -> bool:
        if self._generations:
            return False
        self._loaded = False
        return True
