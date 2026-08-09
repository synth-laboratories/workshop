from __future__ import annotations

import json
from typing import Any

from pydantic import ValidationError

from .errors import ResponsesError, invalid
from .ids import ensure_id


TOP_LEVEL_FIELDS = {
    "model",
    "input",
    "previous_response_id",
    "include",
    "tools",
    "tool_choice",
    "metadata",
    "text",
    "temperature",
    "top_p",
    "presence_penalty",
    "frequency_penalty",
    "parallel_tool_calls",
    "stream",
    "stream_options",
    "background",
    "max_output_tokens",
    "max_tool_calls",
    "reasoning",
    "safety_identifier",
    "prompt_cache_key",
    "truncation",
    "instructions",
    "store",
    "service_tier",
    "top_logprobs",
    "x_synth",
    # Codex app-server transport metadata. It is accepted as an opaque
    # extension, never compiled into prompts or copied to responses.
    "client_metadata",
}

EXTENSION_TOOL_TYPES = {
    "custom",
    "namespace",
    "tool_search",
    "mcp",
    "shell",
    "local_shell",
    "apply_patch",
    "web_search",
}
EXTENSION_ITEM_TYPES = {
    "custom_tool_call",
    "custom_tool_call_output",
    "tool_search_call",
    "tool_search_output",
    "mcp_list_tools",
    "mcp_approval_request",
    "mcp_approval_response",
    "mcp_call",
    "mcp_call_output",
    "shell_call",
    "shell_call_output",
    "local_shell_call",
    "local_shell_call_output",
    "apply_patch_call",
    "apply_patch_call_output",
    "compaction",
}


def _normalize_content(role: str, content: Any) -> list[dict[str, Any]]:
    if isinstance(content, str):
        kind = "output_text" if role == "assistant" else "input_text"
        part: dict[str, Any] = {"type": kind, "text": content}
        if kind == "output_text":
            part["annotations"] = []
        return [part]
    if not isinstance(content, list):
        raise invalid("Message content must be a string or an array.", param="input")
    result: list[dict[str, Any]] = []
    for index, part in enumerate(content):
        if not isinstance(part, dict) or not isinstance(part.get("type"), str):
            raise invalid("Content parts must be typed objects.", param=f"input.content[{index}]")
        result.append(dict(part))
    return result


def _normalize_item(item: Any, index: int) -> dict[str, Any]:
    if not isinstance(item, dict):
        raise invalid("Input items must be objects.", param=f"input[{index}]")
    normalized = dict(item)
    kind = normalized.get("type")
    if not isinstance(kind, str):
        if normalized.get("role"):
            kind = "message"
            normalized["type"] = kind
        else:
            raise invalid("Input item type is required.", param=f"input[{index}].type")
    if kind == "message":
        role = normalized.get("role")
        if role not in {"system", "developer", "user", "assistant"}:
            raise invalid("Unsupported message role.", param=f"input[{index}].role")
        normalized["id"] = ensure_id(normalized.get("id"), "message")
        normalized["content"] = _normalize_content(str(role), normalized.get("content", ""))
        normalized.setdefault("status", "completed")
        if role != "assistant" and "phase" in normalized:
            raise invalid("Only assistant messages may set phase.", param=f"input[{index}].phase")
    elif kind in {
        "function_call",
        "custom_tool_call",
        "tool_search_call",
        "mcp_call",
        "shell_call",
        "local_shell_call",
        "apply_patch_call",
    }:
        normalized["id"] = ensure_id(normalized.get("id"), kind)
        normalized["call_id"] = ensure_id(normalized.get("call_id"), "call")
        normalized.setdefault("status", "completed")
    elif kind.endswith("_output") or kind in {"mcp_approval_response", "tool_search_output"}:
        normalized["id"] = ensure_id(normalized.get("id"), kind)
        if not isinstance(normalized.get("call_id"), str):
            raise invalid("Tool output call_id is required.", param=f"input[{index}].call_id")
    elif kind == "reasoning":
        normalized["id"] = ensure_id(normalized.get("id"), "reasoning")
        normalized.setdefault("summary", [])
    elif kind == "compaction":
        normalized["id"] = ensure_id(normalized.get("id"), "compaction")
        if not isinstance(normalized.get("encrypted_content"), str):
            raise invalid(
                "Compaction encrypted_content is required.",
                param=f"input[{index}].encrypted_content",
            )
    elif kind == "item_reference":
        if not isinstance(normalized.get("id"), str):
            raise invalid("Item reference id is required.", param=f"input[{index}].id")
    else:
        raise invalid(f"Unsupported input item type {kind!r}.", param=f"input[{index}].type")
    return normalized


def normalize_request(body: Any, *, default_model: str) -> dict[str, Any]:
    if not isinstance(body, dict):
        raise invalid("Request body must be a JSON object.")
    unknown = sorted(set(body) - TOP_LEVEL_FIELDS)
    if unknown:
        raise invalid(f"Unknown request field {unknown[0]!r}.", param=unknown[0])
    request = dict(body)
    request["model"] = str(request.get("model") or default_model)
    if not request["model"]:
        raise invalid("model is required.", param="model")
    raw_input = request.get("input", "")
    if isinstance(raw_input, str):
        raw_input = [{"type": "message", "role": "user", "content": raw_input}]
    if not isinstance(raw_input, list):
        raise invalid("input must be a string or an array.", param="input")
    request["input"] = [_normalize_item(item, index) for index, item in enumerate(raw_input)]
    request["tools"] = list(request.get("tools") or [])
    request["stream"] = bool(request.get("stream", False))
    request["store"] = bool(request.get("store", True))
    request["background"] = bool(request.get("background", False))
    request["parallel_tool_calls"] = bool(request.get("parallel_tool_calls", True))
    request["truncation"] = request.get("truncation") or "disabled"
    if request["truncation"] not in {"disabled", "auto"}:
        raise invalid("truncation must be disabled or auto.", param="truncation")
    service_tier = request.get("service_tier") or "default"
    if service_tier not in {"default", "auto"}:
        raise ResponsesError(
            "unsupported_service_tier",
            "Local Laguna supports the standard/default service tier only.",
            400,
            "service_tier",
        )
    request["service_tier"] = "default"
    if request["background"] and request["stream"]:
        raise invalid("background and stream cannot both be true.", param="background")
    previous = request.get("previous_response_id")
    if previous is not None and not isinstance(previous, str):
        raise invalid("previous_response_id must be a string.", param="previous_response_id")
    if not all(isinstance(tool, dict) for tool in request["tools"]):
        raise invalid("tools must contain objects.", param="tools")
    client_metadata = request.get("client_metadata")
    if client_metadata is not None and not isinstance(client_metadata, dict):
        raise invalid("client_metadata must be an object.", param="client_metadata")
    _validate_with_generated_core(request)
    return request


def _validate_with_generated_core(request: dict[str, Any]) -> None:
    extension = any(tool.get("type") in EXTENSION_TOOL_TYPES for tool in request["tools"])
    extension = extension or any(item.get("type") in EXTENSION_ITEM_TYPES for item in request["input"])
    if extension:
        return
    try:
        from openresponses_types import CreateResponseBody

        CreateResponseBody.model_validate(request)
    except ImportError:
        return
    except ValidationError as exc:
        issue = exc.errors(include_url=False)[0]
        param = ".".join(str(part) for part in issue.get("loc") or ()) or None
        raise invalid(str(issue.get("msg") or "Invalid OpenResponses request."), param=param) from exc


def validate_tool_outputs(context_items: list[dict[str, Any]]) -> None:
    calls: dict[str, str] = {}
    consumed: set[str] = set()
    for index, item in enumerate(context_items):
        kind = item.get("type")
        call_id = item.get("call_id")
        if kind in {
            "function_call",
            "custom_tool_call",
            "tool_search_call",
            "mcp_call",
            "shell_call",
            "local_shell_call",
            "apply_patch_call",
        } and isinstance(call_id, str):
            calls[call_id] = kind
        if kind in {
            "function_call_output",
            "custom_tool_call_output",
            "tool_search_output",
            "mcp_call_output",
            "shell_call_output",
            "local_shell_call_output",
            "apply_patch_call_output",
        }:
            if not isinstance(call_id, str) or call_id not in calls:
                raise ResponsesError(
                    "tool_output_without_call",
                    f"No matching tool call exists for output call_id {call_id!r}.",
                    400,
                    f"input[{index}].call_id",
                )
            if call_id in consumed:
                raise ResponsesError(
                    "duplicate_tool_output",
                    f"Tool call {call_id!r} already has an output.",
                    400,
                    f"input[{index}].call_id",
                )
            consumed.add(call_id)


def validate_structured_output(text: str, format_spec: dict[str, Any] | None) -> None:
    if not format_spec or format_spec.get("type") == "text":
        return
    try:
        value = json.loads(text)
    except ValueError as exc:
        raise ResponsesError(
            "invalid_structured_output",
            "The model output was not valid JSON.",
            422,
            error_type="model_error",
        ) from exc
    if format_spec.get("type") != "json_schema":
        if not isinstance(value, dict):
            raise ResponsesError(
                "invalid_structured_output",
                "json_object output must be a JSON object.",
                422,
                error_type="model_error",
            )
        return
    schema = format_spec.get("schema")
    if not isinstance(schema, dict):
        raise invalid("json_schema format requires schema.", param="text.format.schema")
    _validate_json_value(value, schema, path="$")


def _validate_json_value(value: Any, schema: dict[str, Any], *, path: str) -> None:
    expected = schema.get("type")
    if isinstance(expected, list):
        if value is None and "null" in expected:
            return
        expected = next((entry for entry in expected if entry != "null"), None)
    valid_type = {
        "object": isinstance(value, dict),
        "array": isinstance(value, list),
        "string": isinstance(value, str),
        "integer": isinstance(value, int) and not isinstance(value, bool),
        "number": isinstance(value, (int, float)) and not isinstance(value, bool),
        "boolean": isinstance(value, bool),
        "null": value is None,
        None: True,
    }.get(expected, True)
    if not valid_type:
        raise ResponsesError(
            "invalid_structured_output",
            f"Structured output value at {path} does not match type {expected!r}.",
            422,
            error_type="model_error",
        )
    if "enum" in schema and value not in schema["enum"]:
        raise ResponsesError(
            "invalid_structured_output",
            f"Structured output value at {path} is outside the allowed enum.",
            422,
            error_type="model_error",
        )
    if isinstance(value, dict):
        properties = schema.get("properties") or {}
        for key in schema.get("required") or []:
            if key not in value:
                raise ResponsesError(
                    "invalid_structured_output",
                    f"Structured output is missing required property {path}.{key}.",
                    422,
                    error_type="model_error",
                )
        if schema.get("additionalProperties") is False:
            extra = set(value) - set(properties)
            if extra:
                raise ResponsesError(
                    "invalid_structured_output",
                    f"Structured output contains unknown property {path}.{sorted(extra)[0]}.",
                    422,
                    error_type="model_error",
                )
        for key, child in value.items():
            child_schema = properties.get(key)
            if isinstance(child_schema, dict):
                _validate_json_value(child, child_schema, path=f"{path}.{key}")
    if isinstance(value, list) and isinstance(schema.get("items"), dict):
        for index, child in enumerate(value):
            _validate_json_value(child, schema["items"], path=f"{path}[{index}]")
