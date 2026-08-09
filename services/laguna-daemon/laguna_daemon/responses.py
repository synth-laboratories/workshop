"""Minimal OpenAI Responses API shim over chat/completions.

Codex (0.145+) requires ``wire_api = "responses"``. Laguna engines speak
chat/completions; this module translates enough of Responses for a Codex
agent loop (text + function/tool calls, streaming SSE).
"""

from __future__ import annotations

import json
import time
import uuid
from typing import Any, AsyncIterator, Iterator


def _new_id(prefix: str) -> str:
    return f"{prefix}_{uuid.uuid4().hex}"


def responses_input_to_messages(body: dict[str, Any]) -> list[dict[str, Any]]:
    """Convert Responses ``input`` (+ optional instructions) to chat messages."""
    messages: list[dict[str, Any]] = []
    instructions = body.get("instructions")
    if isinstance(instructions, str) and instructions.strip():
        messages.append({"role": "system", "content": instructions.strip()})

    raw = body.get("input")
    if raw is None:
        return messages
    if isinstance(raw, str):
        messages.append({"role": "user", "content": raw})
        return messages
    if not isinstance(raw, list):
        messages.append({"role": "user", "content": str(raw)})
        return messages

    for item in raw:
        if not isinstance(item, dict):
            messages.append({"role": "user", "content": str(item)})
            continue
        item_type = str(item.get("type") or "")
        role = str(item.get("role") or "")

        if item_type in {"message", ""} or role in {"user", "assistant", "system", "developer"}:
            content = item.get("content")
            text = _content_to_text(content)
            mapped_role = role if role in {"user", "assistant", "system"} else "user"
            if role == "developer":
                mapped_role = "system"
            if text:
                messages.append({"role": mapped_role, "content": text})
            continue

        if item_type in {"function_call", "custom_tool_call"}:
            name = str(item.get("name") or item.get("tool_name") or "tool")
            call_id = str(item.get("call_id") or item.get("id") or _new_id("call"))
            arguments = item.get("arguments")
            if not isinstance(arguments, str):
                arguments = json.dumps(arguments or {})
            messages.append(
                {
                    "role": "assistant",
                    "content": None,
                    "tool_calls": [
                        {
                            "id": call_id,
                            "type": "function",
                            "function": {"name": name, "arguments": arguments},
                        }
                    ],
                }
            )
            continue

        if item_type in {"function_call_output", "custom_tool_call_output", "tool_result"}:
            call_id = str(item.get("call_id") or item.get("id") or _new_id("call"))
            output = item.get("output")
            if output is None:
                output = item.get("content")
            if not isinstance(output, str):
                output = json.dumps(output)
            messages.append(
                {
                    "role": "tool",
                    "tool_call_id": call_id,
                    "content": output,
                }
            )
            continue

        # Fallback: stringify unknown items as user context
        text = _content_to_text(item.get("content")) or json.dumps(item)
        messages.append({"role": "user", "content": text})

    return messages


def _content_to_text(content: Any) -> str:
    if content is None:
        return ""
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        parts: list[str] = []
        for part in content:
            if isinstance(part, str):
                parts.append(part)
            elif isinstance(part, dict):
                if isinstance(part.get("text"), str):
                    parts.append(part["text"])
                elif isinstance(part.get("content"), str):
                    parts.append(part["content"])
                elif part.get("type") in {"input_text", "output_text", "text"}:
                    parts.append(str(part.get("text") or ""))
        return "".join(parts)
    if isinstance(content, dict):
        if isinstance(content.get("text"), str):
            return content["text"]
        return json.dumps(content)
    return str(content)


def responses_tools_to_chat(tools: Any) -> list[dict[str, Any]] | None:
    if not isinstance(tools, list) or not tools:
        return None
    out: list[dict[str, Any]] = []
    for tool in tools:
        if not isinstance(tool, dict):
            continue
        # Already chat-shaped
        if tool.get("type") == "function" and isinstance(tool.get("function"), dict):
            out.append(tool)
            continue
        # Responses-shaped function tool
        if tool.get("type") == "function" or "name" in tool:
            name = str(tool.get("name") or (tool.get("function") or {}).get("name") or "")
            if not name:
                continue
            parameters = tool.get("parameters") or tool.get("input_schema") or {
                "type": "object",
                "properties": {},
            }
            description = str(tool.get("description") or "")
            out.append(
                {
                    "type": "function",
                    "function": {
                        "name": name,
                        "description": description,
                        "parameters": parameters,
                    },
                }
            )
    return out or None


def build_chat_body_from_responses(body: dict[str, Any], *, default_model: str) -> dict[str, Any]:
    model = body.get("model") or default_model
    aliases = {
        "laguna-xs-2.1",
        "synth/Laguna-XS-2.1",
        "synth/Laguna-XS-2.1-NVFP4",
    }
    if model in aliases:
        model = default_model
    chat: dict[str, Any] = {
        "model": model,
        "messages": responses_input_to_messages(body),
        "stream": bool(body.get("stream")),
    }
    tools = responses_tools_to_chat(body.get("tools"))
    if tools:
        chat["tools"] = tools
        if body.get("tool_choice") is not None:
            chat["tool_choice"] = body["tool_choice"]
    for key in ("temperature", "top_p", "max_tokens", "max_output_tokens"):
        if key in body and body[key] is not None:
            if key == "max_output_tokens":
                chat["max_tokens"] = body[key]
            else:
                chat[key] = body[key]
    return chat


def chat_message_to_response_output(
    message: dict[str, Any],
    *,
    response_id: str,
) -> list[dict[str, Any]]:
    output: list[dict[str, Any]] = []
    tool_calls = message.get("tool_calls") or []
    if isinstance(tool_calls, list):
        for call in tool_calls:
            if not isinstance(call, dict):
                continue
            fn = call.get("function") if isinstance(call.get("function"), dict) else {}
            call_id = str(call.get("id") or _new_id("call"))
            output.append(
                {
                    "type": "function_call",
                    "id": call_id,
                    "call_id": call_id,
                    "name": str(fn.get("name") or "tool"),
                    "arguments": str(fn.get("arguments") or "{}"),
                    "status": "completed",
                }
            )
    content = message.get("content")
    if isinstance(content, str) and content:
        output.append(
            {
                "type": "message",
                "id": _new_id("msg"),
                "role": "assistant",
                "status": "completed",
                "content": [{"type": "output_text", "text": content}],
            }
        )
    if not output:
        output.append(
            {
                "type": "message",
                "id": _new_id("msg"),
                "role": "assistant",
                "status": "completed",
                "content": [{"type": "output_text", "text": ""}],
            }
        )
    return output


def wrap_chat_as_response(
    chat_json: dict[str, Any],
    *,
    model: str,
) -> dict[str, Any]:
    response_id = _new_id("resp")
    created = int(time.time())
    choices = chat_json.get("choices") or []
    message: dict[str, Any] = {}
    if choices and isinstance(choices[0], dict):
        message = choices[0].get("message") or {}
    usage = chat_json.get("usage") or {}
    return {
        "id": response_id,
        "object": "response",
        "created_at": created,
        "status": "completed",
        "model": model,
        "output": chat_message_to_response_output(message, response_id=response_id),
        "usage": {
            "input_tokens": usage.get("prompt_tokens") or 0,
            "output_tokens": usage.get("completion_tokens") or 0,
            "total_tokens": usage.get("total_tokens") or 0,
        },
    }


def mock_response_payload(model: str, prompt: str) -> dict[str, Any]:
    text = f"Synth Laguna Responses shim (mock). Received: {prompt[:240]}"
    response_id = _new_id("resp")
    return {
        "id": response_id,
        "object": "response",
        "created_at": int(time.time()),
        "status": "completed",
        "model": model,
        "output": [
            {
                "type": "message",
                "id": _new_id("msg"),
                "role": "assistant",
                "status": "completed",
                "content": [{"type": "output_text", "text": text}],
            }
        ],
        "usage": {
            "input_tokens": max(1, len(prompt.split())),
            "output_tokens": max(1, len(text.split())),
            "total_tokens": max(2, len(prompt.split()) + len(text.split())),
        },
    }


def iter_response_sse_from_text(
    *,
    model: str,
    text: str,
    response_id: str | None = None,
) -> Iterator[dict[str, Any]]:
    """Emit Responses SSE event objects for a plain text completion."""
    rid = response_id or _new_id("resp")
    msg_id = _new_id("msg")
    created = int(time.time())
    yield {
        "type": "response.created",
        "response": {
            "id": rid,
            "object": "response",
            "created_at": created,
            "status": "in_progress",
            "model": model,
            "output": [],
        },
    }
    yield {
        "type": "response.output_item.added",
        "output_index": 0,
        "item": {
            "type": "message",
            "id": msg_id,
            "role": "assistant",
            "status": "in_progress",
            "content": [],
        },
    }
    yield {
        "type": "response.content_part.added",
        "output_index": 0,
        "content_index": 0,
        "part": {"type": "output_text", "text": ""},
    }
    # Stream by words for mock / non-streaming upstreams
    words = text.split(" ")
    pieces = [words[0]] + [f" {w}" for w in words[1:]] if words else [""]
    for piece in pieces:
        if not piece:
            continue
        yield {
            "type": "response.output_text.delta",
            "output_index": 0,
            "content_index": 0,
            "delta": piece,
        }
    yield {
        "type": "response.output_text.done",
        "output_index": 0,
        "content_index": 0,
        "text": text,
    }
    yield {
        "type": "response.output_item.done",
        "output_index": 0,
        "item": {
            "type": "message",
            "id": msg_id,
            "role": "assistant",
            "status": "completed",
            "content": [{"type": "output_text", "text": text}],
        },
    }
    yield {
        "type": "response.completed",
        "response": {
            "id": rid,
            "object": "response",
            "created_at": created,
            "status": "completed",
            "model": model,
            "output": [
                {
                    "type": "message",
                    "id": msg_id,
                    "role": "assistant",
                    "status": "completed",
                    "content": [{"type": "output_text", "text": text}],
                }
            ],
        },
    }


def sse_event(payload: dict[str, Any]) -> bytes:
    event_type = str(payload.get("type") or "message")
    data = json.dumps(payload, separators=(",", ":"))
    return f"event: {event_type}\ndata: {data}\n\n".encode()


async def translate_chat_sse_to_responses(
    lines: AsyncIterator[bytes],
    *,
    model: str,
) -> AsyncIterator[bytes]:
    """Convert chat.completion.chunk SSE bytes into Responses SSE events."""
    rid = _new_id("resp")
    msg_id = _new_id("msg")
    created = int(time.time())
    started = False
    content_started = False
    text_parts: list[str] = []
    tool_calls: dict[int, dict[str, Any]] = {}

    async for raw in lines:
        for line in raw.decode("utf-8", errors="replace").splitlines():
            line = line.strip()
            if not line.startswith("data:"):
                continue
            data = line[5:].strip()
            if not data or data == "[DONE]":
                continue
            try:
                frame = json.loads(data)
            except json.JSONDecodeError:
                continue
            if not started:
                started = True
                yield sse_event(
                    {
                        "type": "response.created",
                        "response": {
                            "id": rid,
                            "object": "response",
                            "created_at": created,
                            "status": "in_progress",
                            "model": model,
                            "output": [],
                        },
                    }
                )
            choices = frame.get("choices") or []
            if not choices:
                continue
            delta = choices[0].get("delta") or {}
            finish = choices[0].get("finish_reason")

            # Tool call deltas
            for tc in delta.get("tool_calls") or []:
                if not isinstance(tc, dict):
                    continue
                idx = int(tc.get("index") or 0)
                slot = tool_calls.setdefault(
                    idx,
                    {
                        "id": str(tc.get("id") or _new_id("call")),
                        "name": "",
                        "arguments": "",
                    },
                )
                if tc.get("id"):
                    slot["id"] = str(tc["id"])
                fn = tc.get("function") or {}
                if fn.get("name"):
                    slot["name"] = str(fn["name"])
                if fn.get("arguments"):
                    slot["arguments"] += str(fn["arguments"])

            content = delta.get("content")
            if isinstance(content, str) and content:
                if not content_started:
                    content_started = True
                    yield sse_event(
                        {
                            "type": "response.output_item.added",
                            "output_index": 0,
                            "item": {
                                "type": "message",
                                "id": msg_id,
                                "role": "assistant",
                                "status": "in_progress",
                                "content": [],
                            },
                        }
                    )
                    yield sse_event(
                        {
                            "type": "response.content_part.added",
                            "output_index": 0,
                            "content_index": 0,
                            "part": {"type": "output_text", "text": ""},
                        }
                    )
                text_parts.append(content)
                yield sse_event(
                    {
                        "type": "response.output_text.delta",
                        "output_index": 0,
                        "content_index": 0,
                        "delta": content,
                    }
                )

            if finish:
                break

    full_text = "".join(text_parts)
    output_items: list[dict[str, Any]] = []

    if tool_calls:
        for idx, slot in sorted(tool_calls.items()):
            item = {
                "type": "function_call",
                "id": slot["id"],
                "call_id": slot["id"],
                "name": slot["name"] or "tool",
                "arguments": slot["arguments"] or "{}",
                "status": "completed",
            }
            output_items.append(item)
            yield sse_event(
                {
                    "type": "response.output_item.added",
                    "output_index": len(output_items) - 1,
                    "item": {**item, "status": "in_progress"},
                }
            )
            yield sse_event(
                {
                    "type": "response.output_item.done",
                    "output_index": len(output_items) - 1,
                    "item": item,
                }
            )

    if content_started or full_text or not tool_calls:
        if content_started:
            yield sse_event(
                {
                    "type": "response.output_text.done",
                    "output_index": 0,
                    "content_index": 0,
                    "text": full_text,
                }
            )
            yield sse_event(
                {
                    "type": "response.output_item.done",
                    "output_index": 0,
                    "item": {
                        "type": "message",
                        "id": msg_id,
                        "role": "assistant",
                        "status": "completed",
                        "content": [{"type": "output_text", "text": full_text}],
                    },
                }
            )
        output_items.insert(
            0,
            {
                "type": "message",
                "id": msg_id,
                "role": "assistant",
                "status": "completed",
                "content": [{"type": "output_text", "text": full_text}],
            },
        )

    if not started:
        for event in iter_response_sse_from_text(model=model, text="", response_id=rid):
            yield sse_event(event)
        return

    yield sse_event(
        {
            "type": "response.completed",
            "response": {
                "id": rid,
                "object": "response",
                "created_at": created,
                "status": "completed",
                "model": model,
                "output": output_items,
            },
        }
    )
