from __future__ import annotations

import json
import time
from typing import Any, Awaitable, Callable

from ..responses_api.backends.protocol import ModelEvent
from ..responses_api.errors import ResponsesError
from ..responses_api.ids import new_id


ChunkSink = Callable[[dict[str, Any]], Awaitable[None]]

#: Model finish reasons mapped onto the Chat vocabulary.
_FINISH_REASONS = {
    "stop": "stop",
    "length": "length",
    "tool_call": "tool_calls",
    "tool_calls": "tool_calls",
    # Chat has no cancelled reason. A cancelled turn stops early, so report the
    # truthful shape a client can act on rather than inventing a new value.
    "cancelled": "stop",
}


def chat_sse_frame(payload: dict[str, Any]) -> bytes:
    body = json.dumps(payload, ensure_ascii=False, separators=(",", ":"))
    return f"data: {body}\n\n".encode()


class ChatEventAssembler:
    """Renders canonical `ModelEvent`s as Chat Completions objects.

    The peer of `ResponseEventAssembler`. Both consume the same model event
    stream from the same runner; neither is derived from the other. Streaming
    and non-streaming go through this one object so the final text, tool calls,
    finish reason, and usage are the same by construction rather than by two
    implementations agreeing.
    """

    def __init__(
        self,
        *,
        model: str,
        sink: ChunkSink | None = None,
        include_usage: bool = False,
    ) -> None:
        self.id = new_id("chatcmpl")
        self.model = model
        self.sink = sink
        self.include_usage = include_usage
        self.created = int(time.time())
        self._role_sent = False
        self._content = ""
        self._reasoning = ""
        self._tool_calls: list[dict[str, Any]] = []
        self._usage = {
            "prompt_tokens": 0,
            "completion_tokens": 0,
            "total_tokens": 0,
            "prompt_tokens_details": {"cached_tokens": 0},
            "completion_tokens_details": {"reasoning_tokens": 0},
        }

    # -- emission -----------------------------------------------------------

    async def _emit(self, delta: dict[str, Any], finish_reason: str | None = None) -> None:
        if self.sink is None:
            return
        await self.sink(
            {
                "id": self.id,
                "object": "chat.completion.chunk",
                "created": self.created,
                "model": self.model,
                "choices": [
                    {"index": 0, "delta": delta, "finish_reason": finish_reason}
                ],
            }
        )

    async def _ensure_role(self) -> None:
        if self._role_sent:
            return
        self._role_sent = True
        await self._emit({"role": "assistant"})

    # -- TurnRenderer protocol ---------------------------------------------

    async def start(self) -> None:
        await self._ensure_role()

    async def consume(self, model_event: ModelEvent) -> str | None:
        kind = model_event.kind
        if kind == "text_delta":
            self._content += model_event.delta
            await self._ensure_role()
            await self._emit({"content": model_event.delta})
        elif kind == "reasoning_delta":
            self._reasoning += model_event.delta
            await self._ensure_role()
            await self._emit({"reasoning_content": model_event.delta})
        elif kind == "function_call":
            await self._add_tool_call(model_event)
        elif kind in {
            "custom_tool_call",
            "tool_search_call",
            "mcp_call",
            "shell_call",
            "apply_patch_call",
        }:
            # Chat validation only ever binds function tools, so reaching here
            # means a binding was built that this surface cannot represent.
            # Failing loudly beats silently degrading it to a function call.
            raise ResponsesError(
                "unrepresentable_tool_call",
                f"The model produced a {kind!r} that the Chat surface cannot "
                "represent. Use the native Responses surface for this tool kind.",
                500,
                error_type="server_error",
            )
        elif kind == "usage":
            self._usage = {
                "prompt_tokens": int(model_event.input_tokens or 0),
                "completion_tokens": int(model_event.output_tokens or 0),
                "total_tokens": int(model_event.input_tokens or 0)
                + int(model_event.output_tokens or 0),
                "prompt_tokens_details": {
                    "cached_tokens": int(model_event.metadata.get("cached_tokens") or 0)
                },
                "completion_tokens_details": {
                    "reasoning_tokens": int(model_event.reasoning_tokens or 0)
                },
            }
        elif kind == "finish":
            return model_event.finish_reason or "stop"
        elif kind == "error":
            raise ResponsesError(
                model_event.error_code or "model_error",
                model_event.error_message or "The model backend failed.",
                500,
                error_type="model_error",
            )
        return None

    async def _add_tool_call(self, model_event: ModelEvent) -> None:
        index = len(self._tool_calls)
        arguments = model_event.arguments
        if not isinstance(arguments, str):
            arguments = json.dumps(arguments or {}, separators=(",", ":"))
        call = {
            "index": index,
            "id": model_event.call_id or new_id("call"),
            "type": "function",
            # `name` is already the caller's original tool name: the backend
            # restores it from the immutable ToolBinding before emitting.
            "function": {"name": model_event.name, "arguments": arguments},
        }
        self._tool_calls.append(call)
        await self._ensure_role()
        await self._emit({"tool_calls": [call]})

    async def complete(self, finish_reason: str) -> dict[str, Any]:
        resolved = _FINISH_REASONS.get(finish_reason, "stop")
        if self._tool_calls:
            resolved = "tool_calls"
        await self._emit({}, finish_reason=resolved)
        if self.include_usage and self.sink is not None:
            await self.sink(
                {
                    "id": self.id,
                    "object": "chat.completion.chunk",
                    "created": self.created,
                    "model": self.model,
                    "choices": [],
                    "usage": dict(self._usage),
                }
            )
        return self.final(resolved)

    async def fail(self, error: ResponsesError) -> dict[str, Any]:
        # A failed turn has no Chat object to return; the caller renders the
        # error envelope. Re-raising keeps that decision at the HTTP boundary.
        raise error

    # -- final object -------------------------------------------------------

    def final(self, finish_reason: str) -> dict[str, Any]:
        message: dict[str, Any] = {"role": "assistant", "content": self._content or None}
        if self._reasoning:
            message["reasoning_content"] = self._reasoning
        if self._tool_calls:
            message["tool_calls"] = [
                {
                    "id": call["id"],
                    "type": "function",
                    "function": dict(call["function"]),
                }
                for call in self._tool_calls
            ]
        return {
            "id": self.id,
            "object": "chat.completion",
            "created": self.created,
            "model": self.model,
            "choices": [
                {"index": 0, "message": message, "finish_reason": finish_reason}
            ],
            "usage": dict(self._usage),
        }
