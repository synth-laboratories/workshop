from __future__ import annotations

import json
from typing import Any

from ..responses_api.backends.protocol import CompiledTurn, ModelBackend
from ..responses_api.capabilities import SamplingDefaults
from .validation import TEXT_PART_TYPES, validate_chat_request


def _content_text(content: Any) -> str:
    if isinstance(content, str):
        return content
    if not isinstance(content, list):
        return ""
    pieces = [
        str(part.get("text") or "")
        for part in content
        if isinstance(part, dict) and part.get("type") in TEXT_PART_TYPES
    ]
    return "\n".join(piece for piece in pieces if piece)


def normalize_messages(messages: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Bring Chat messages to the prompt-compiler's message shape.

    This is close to a pass-through by design: the compiler's internal message
    form *is* the chat form, so the only work is flattening content parts,
    folding `developer` onto `system`, and normalizing tool-call arguments.
    Nothing here converts to or from a Responses item.
    """
    normalized: list[dict[str, Any]] = []
    for message in messages:
        role = message.get("role")
        if role == "developer":
            role = "system"
        entry: dict[str, Any] = {
            "role": role,
            "content": _content_text(message.get("content")),
        }
        if role == "tool":
            entry["tool_call_id"] = message.get("tool_call_id")
        if role == "assistant":
            # Poolside's preserved-thinking contract: prior assistant
            # reasoning must round-trip so the template can re-render it
            # inside <think>...</think>. Dropping it here is the documented
            # way to make the model stop reasoning after the first tool
            # boundary. `reasoning_content` is our own response field name;
            # `reasoning` is vLLM's spelling, accepted for compatibility.
            reasoning = message.get("reasoning_content")
            if not isinstance(reasoning, str) or not reasoning:
                reasoning = message.get("reasoning")
            if isinstance(reasoning, str) and reasoning:
                entry["reasoning_content"] = reasoning
        tool_calls = message.get("tool_calls")
        if role == "assistant" and isinstance(tool_calls, list) and tool_calls:
            entry["tool_calls"] = [
                {
                    "id": call.get("id"),
                    "type": "function",
                    "function": {
                        "name": (call.get("function") or {}).get("name"),
                        # The compiler's message form carries decoded arguments;
                        # Chat sends them as a JSON string on the wire.
                        "arguments": _decode_arguments(
                            (call.get("function") or {}).get("arguments")
                        ),
                    },
                }
                for call in tool_calls
                if isinstance(call, dict)
            ]
        normalized.append(entry)
    return normalized


def _decode_arguments(raw: Any) -> Any:
    if isinstance(raw, str):
        try:
            return json.loads(raw)
        except ValueError:
            return {"input": raw}
    return raw if isinstance(raw, dict) else {}


def flatten_tools(tools: Any) -> list[dict[str, Any]]:
    """Chat nests a function under `function`; the binder takes the flat form."""
    if not isinstance(tools, list):
        return []
    flattened: list[dict[str, Any]] = []
    for tool in tools:
        function = tool.get("function") or {}
        flattened.append(
            {
                "type": "function",
                "name": function.get("name"),
                "description": function.get("description") or "",
                "parameters": function.get("parameters")
                if isinstance(function.get("parameters"), dict)
                else {"type": "object", "properties": {}},
            }
        )
    return flattened


def normalize_tool_choice(tool_choice: Any) -> Any:
    if isinstance(tool_choice, dict):
        return {"name": (tool_choice.get("function") or {}).get("name")}
    return tool_choice if tool_choice is not None else "auto"


def _structured_format(response_format: Any) -> dict[str, Any] | None:
    if not isinstance(response_format, dict):
        return None
    kind = response_format.get("type")
    if kind == "json_object":
        return {"type": "json_object"}
    if kind == "json_schema":
        schema_spec = response_format.get("json_schema")
        schema = (
            schema_spec.get("schema")
            if isinstance(schema_spec, dict)
            else response_format.get("schema")
        )
        return {"type": "json_schema", "schema": schema}
    return None


async def compile_chat_turn(
    body: dict[str, Any],
    *,
    backend: ModelBackend,
    generation_id: str,
    default_model: str,
) -> CompiledTurn:
    """Compile a validated Chat request straight onto the neutral core.

    The request never becomes a Responses object on the way. Chat is a peer
    front-end, so it reaches `compile_messages` with exactly the arguments the
    core defines.
    """
    validate_chat_request(body)
    max_tokens = body.get("max_completion_tokens") or body.get("max_tokens")
    effort = body.get("reasoning_effort")
    cache_key = body.get("prompt_cache_key")
    # Same fallback rule as the Responses surface: explicit request value,
    # then the settings store's live defaults on the shared backend, then
    # the historical built-ins.
    sampling = getattr(backend, "sampling_defaults", None) or SamplingDefaults()
    return await backend.compile_messages(
        messages=normalize_messages(body["messages"]),
        model=str(body.get("model") or default_model),
        generation_id=generation_id,
        tools=flatten_tools(body.get("tools")),
        tool_choice=normalize_tool_choice(body.get("tool_choice")),
        max_output_tokens=int(max_tokens or sampling.max_output_tokens),
        temperature=float(body.get("temperature", sampling.temperature)),
        top_p=float(body.get("top_p", sampling.top_p)),
        top_k=int(body.get("top_k") or sampling.top_k),
        # Chat's `reasoning_effort` is absent by default; absence means the
        # configured default (thinking on unless settings say otherwise).
        # Unsupported spellings were rejected by validate_chat_request.
        enable_thinking=(effort or sampling.reasoning_effort) != "none",
        structured_format=_structured_format(body.get("response_format")),
        prompt_cache_key=cache_key if isinstance(cache_key, str) and cache_key else None,
    )
