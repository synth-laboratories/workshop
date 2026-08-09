from __future__ import annotations

import json
from copy import deepcopy
from typing import Any, Awaitable, Callable

from .backends.protocol import ModelEvent, ToolBinding
from .errors import ResponsesError
from .ids import new_id
from .validation import _validate_json_value, validate_structured_output


EventSink = Callable[[dict[str, Any]], Awaitable[None]]


def sse_frame(event: dict[str, Any]) -> bytes:
    payload = json.dumps(event, ensure_ascii=False, separators=(",", ":"))
    return f"event: {event['type']}\ndata: {payload}\n\n".encode()


class ResponseEventAssembler:
    def __init__(
        self,
        response: dict[str, Any],
        bindings: dict[str, ToolBinding],
        sink: EventSink | None,
    ) -> None:
        self.response = response
        self.bindings = bindings
        self.sink = sink
        self.sequence = 0
        self.output: list[dict[str, Any]] = response["output"]
        self._message: dict[str, Any] | None = None
        self._reasoning: dict[str, Any] | None = None
        self._text = ""
        self._reasoning_text = ""
        self._usage = {
            "input_tokens": 0,
            "input_tokens_details": {"cached_tokens": 0},
            "output_tokens": 0,
            "output_tokens_details": {"reasoning_tokens": 0},
            "total_tokens": 0,
        }

    async def emit(self, event_type: str, **fields: Any) -> None:
        event = {"type": event_type, "sequence_number": self.sequence, **fields}
        self.sequence += 1
        if self.sink is not None:
            await self.sink(event)

    async def start(self) -> None:
        await self.emit("response.created", response=deepcopy(self.response))
        await self.emit("response.in_progress", response=deepcopy(self.response))

    async def consume(self, model_event: ModelEvent) -> str | None:
        kind = model_event.kind
        if kind == "reasoning_delta":
            await self._append_reasoning(model_event.delta)
        elif kind == "text_delta":
            await self._append_text(model_event.delta)
        elif kind in {
            "function_call",
            "custom_tool_call",
            "tool_search_call",
            "mcp_call",
            "shell_call",
            "apply_patch_call",
        }:
            await self._finish_reasoning()
            await self._finish_message()
            await self._add_tool_call(model_event)
        elif kind == "usage":
            self._usage = {
                "input_tokens": int(model_event.input_tokens or 0),
                "input_tokens_details": {
                    "cached_tokens": int(model_event.metadata.get("cached_tokens") or 0)
                },
                "output_tokens": int(model_event.output_tokens or 0),
                "output_tokens_details": {
                    "reasoning_tokens": int(model_event.reasoning_tokens or 0)
                },
                "total_tokens": int(model_event.input_tokens or 0)
                + int(model_event.output_tokens or 0),
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

    async def _append_reasoning(self, delta: str) -> None:
        if self._reasoning is None:
            item = {
                "id": new_id("reasoning"),
                "type": "reasoning",
                "summary": [],
                "content": [],
                "encrypted_content": None,
            }
            self._reasoning = item
            self.output.append(item)
            await self.emit(
                "response.output_item.added",
                output_index=len(self.output) - 1,
                item=deepcopy(item),
            )
            await self.emit(
                "response.reasoning_summary_part.added",
                item_id=item["id"],
                output_index=len(self.output) - 1,
                summary_index=0,
                part={"type": "summary_text", "text": ""},
            )
        self._reasoning_text += delta
        await self.emit(
            "response.reasoning_summary_text.delta",
            item_id=self._reasoning["id"],
            output_index=self.output.index(self._reasoning),
            summary_index=0,
            delta=delta,
        )

    async def _finish_reasoning(self) -> None:
        if self._reasoning is None:
            return
        item = self._reasoning
        index = self.output.index(item)
        part = {"type": "summary_text", "text": self._reasoning_text}
        item["summary"] = [part]
        await self.emit(
            "response.reasoning_summary_text.done",
            item_id=item["id"],
            output_index=index,
            summary_index=0,
            text=self._reasoning_text,
        )
        await self.emit(
            "response.reasoning_summary_part.done",
            item_id=item["id"],
            output_index=index,
            summary_index=0,
            part=deepcopy(part),
        )
        await self.emit("response.output_item.done", output_index=index, item=deepcopy(item))
        self._reasoning = None

    async def _append_text(self, delta: str) -> None:
        await self._finish_reasoning()
        if self._message is None:
            item = {
                "id": new_id("message"),
                "type": "message",
                "status": "in_progress",
                "role": "assistant",
                "phase": "final_answer",
                "content": [],
            }
            self._message = item
            self.output.append(item)
            index = len(self.output) - 1
            await self.emit("response.output_item.added", output_index=index, item=deepcopy(item))
            await self.emit(
                "response.content_part.added",
                item_id=item["id"],
                output_index=index,
                content_index=0,
                part={
                    "type": "output_text",
                    "text": "",
                    "annotations": [],
                    "logprobs": [],
                },
            )
        self._text += delta
        await self.emit(
            "response.output_text.delta",
            item_id=self._message["id"],
            output_index=self.output.index(self._message),
            content_index=0,
            delta=delta,
            logprobs=[],
        )

    async def _finish_message(self) -> None:
        if self._message is None:
            return
        item = self._message
        index = self.output.index(item)
        part = {
            "type": "output_text",
            "text": self._text,
            "annotations": [],
            "logprobs": [],
        }
        item["content"] = [part]
        item["status"] = "completed"
        await self.emit(
            "response.output_text.done",
            item_id=item["id"],
            output_index=index,
            content_index=0,
            text=self._text,
            logprobs=[],
        )
        await self.emit(
            "response.content_part.done",
            item_id=item["id"],
            output_index=index,
            content_index=0,
            part=deepcopy(part),
        )
        await self.emit("response.output_item.done", output_index=index, item=deepcopy(item))
        self._message = None

    async def _add_tool_call(self, event: ModelEvent) -> None:
        name = str(event.name or "")
        binding = next(
            (
                candidate
                for candidate in self.bindings.values()
                if candidate.original_name == name
                and candidate.namespace == event.namespace
            ),
            None,
        )
        call_id = event.call_id or new_id("call")
        if event.kind == "custom_tool_call":
            item = {
                "id": new_id("custom_tool_call"),
                "type": "custom_tool_call",
                "status": "in_progress",
                "call_id": call_id,
                "name": name,
                "input": "",
            }
            delta_type = "response.custom_tool_call_input.delta"
            done_type = "response.custom_tool_call_input.done"
            field = "input"
            value = event.input or ""
        else:
            item_type = {
                "function_call": "function_call",
                "tool_search_call": "tool_search_call",
                "mcp_call": "mcp_call",
                "shell_call": "shell_call",
                "apply_patch_call": "apply_patch_call",
            }[event.kind]
            item = {
                "id": new_id(item_type),
                "type": item_type,
                "status": "in_progress",
                "call_id": call_id,
                "name": name,
                "arguments": "",
            }
            value = event.arguments or "{}"
            if binding and binding.kind == "function" and binding.schema:
                try:
                    parsed = json.loads(value)
                except ValueError as exc:
                    raise ResponsesError(
                        "invalid_tool_arguments",
                        f"Tool {name!r} returned invalid JSON arguments.",
                        422,
                        error_type="model_error",
                    ) from exc
                _validate_json_value(parsed, binding.schema, path=f"tool.{name}")
            prefix = {
                "function_call": "response.function_call_arguments",
                "tool_search_call": "response.tool_search_call.arguments",
                "mcp_call": "response.mcp_call.arguments",
                "shell_call": "response.shell_call.arguments",
                "apply_patch_call": "response.apply_patch_call.arguments",
            }[event.kind]
            delta_type = f"{prefix}.delta"
            done_type = f"{prefix}.done"
            field = "arguments"
        if event.namespace:
            item["namespace"] = event.namespace
        self.output.append(item)
        index = len(self.output) - 1
        await self.emit("response.output_item.added", output_index=index, item=deepcopy(item))
        await self.emit(
            delta_type,
            item_id=item["id"],
            output_index=index,
            delta=value,
        )
        await self.emit(
            done_type,
            item_id=item["id"],
            output_index=index,
            **{field: value},
        )
        item[field] = value
        item["status"] = "completed"
        await self.emit("response.output_item.done", output_index=index, item=deepcopy(item))

    async def complete(self, finish_reason: str) -> dict[str, Any]:
        await self._finish_reasoning()
        await self._finish_message()
        self.response["usage"] = self._usage
        self.response["completed_at"] = self.response["created_at"]
        if finish_reason in {"length", "max_output_tokens"}:
            self.response["status"] = "incomplete"
            self.response["incomplete_details"] = {"reason": "max_output_tokens"}
            await self.emit("response.incomplete", response=deepcopy(self.response))
        elif finish_reason == "cancelled":
            self.response["status"] = "cancelled"
            self.response["error"] = {"code": "cancelled", "message": "Response cancelled."}
            await self.emit("response.failed", response=deepcopy(self.response))
        else:
            validate_structured_output(self._text, self.response["text"].get("format"))
            self.response["status"] = "completed"
            await self.emit("response.completed", response=deepcopy(self.response))
        return self.response

    async def fail(self, error: ResponsesError) -> dict[str, Any]:
        await self._finish_reasoning()
        await self._finish_message()
        self.response["status"] = "failed"
        self.response["completed_at"] = self.response["created_at"]
        self.response["error"] = {"code": error.code, "message": error.message}
        self.response["usage"] = self._usage
        await self.emit(
            "error",
            error={
                "type": error.error_type,
                "code": error.code,
                "message": error.message,
                "param": error.param,
            },
        )
        await self.emit("response.failed", response=deepcopy(self.response))
        return self.response
