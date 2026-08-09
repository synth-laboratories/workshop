from __future__ import annotations

import asyncio
import json
from typing import Any, AsyncIterator

from ..capabilities import ModelCapabilities
from ..compiler import compile_turn
from ..ids import new_id
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

    async def capabilities(self, model: str) -> ModelCapabilities:
        return self._capabilities

    async def compile(
        self,
        request: dict[str, Any],
        context_items: list[dict[str, Any]],
        generation_id: str,
    ) -> CompiledTurn:
        return compile_turn(request, context_items, generation_id)

    async def count_tokens(self, turn: CompiledTurn) -> TokenUsageEstimate:
        serialized = json.dumps(turn.messages, ensure_ascii=False)
        return TokenUsageEstimate(max(1, (len(serialized) + 3) // 4))

    async def stream(self, turn: CompiledTurn) -> AsyncIterator[ModelEvent]:
        usage = await self.count_tokens(turn)
        if turn.generation_id in self._cancelled:
            yield ModelEvent(kind="finish", finish_reason="cancelled")
            return
        reasoning = turn.request.get("reasoning")
        if isinstance(reasoning, dict) and reasoning.get("effort") not in {
            None,
            "none",
            "minimal",
        }:
            yield ModelEvent(kind="reasoning_delta", delta="Checked the request contract.")

        last = turn.context_items[-1] if turn.context_items else {}
        last_kind = last.get("type") if isinstance(last, dict) else None
        continuation_outputs = {
            "function_call_output",
            "custom_tool_call_output",
            "shell_call_output",
            "local_shell_call_output",
            "apply_patch_call_output",
            "mcp_call_output",
            "tool_search_output",
        }
        user_prompt = next(
            (
                str(message.get("content") or "")
                for message in reversed(turn.messages)
                if message.get("role") == "user"
            ),
            "",
        )
        suppress_tools = (
            turn.request.get("tool_choice") == "none"
            or "do not call tools" in user_prompt.lower()
        )
        output_tokens = 0
        if turn.bindings and last_kind not in continuation_outputs and not suppress_tools:
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
            elif last_kind in continuation_outputs:
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
            reasoning_tokens=5 if reasoning else 0,
        )
        yield ModelEvent(kind="finish", finish_reason="stop")

    async def cancel(self, generation_id: str) -> None:
        self._cancelled.add(generation_id)

    async def close(self) -> None:
        self._cancelled.clear()
