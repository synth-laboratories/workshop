from __future__ import annotations

import asyncio
import time
from copy import deepcopy
from typing import Any

from .backends.protocol import ModelBackend
from .capabilities import validate_capabilities
from .errors import ResponsesError
from .events import EventSink, ResponseEventAssembler
from .ids import new_id
from .runner import TurnRunner
from .storage import SQLiteResponseStore, StoredResponse
from .validation import validate_tool_outputs


def response_shell(request: dict[str, Any], response_id: str | None = None) -> dict[str, Any]:
    text = request.get("text") if isinstance(request.get("text"), dict) else {}
    format_spec = text.get("format") if isinstance(text, dict) else None
    if not isinstance(format_spec, dict):
        format_spec = {"type": "text"}
    reasoning = request.get("reasoning") if isinstance(request.get("reasoning"), dict) else None
    response_tools = deepcopy(request.get("tools") or [])
    for tool in response_tools:
        if tool.get("type") == "function":
            tool.setdefault("strict", False)
    return {
        "id": response_id or new_id("response"),
        "object": "response",
        "created_at": int(time.time()),
        "completed_at": None,
        "status": "in_progress",
        "incomplete_details": None,
        "model": request["model"],
        "previous_response_id": request.get("previous_response_id"),
        "instructions": request.get("instructions"),
        "output": [],
        "error": None,
        "tools": response_tools,
        "tool_choice": deepcopy(request.get("tool_choice") or "auto"),
        "truncation": request.get("truncation", "disabled"),
        "parallel_tool_calls": request.get("parallel_tool_calls", True),
        "text": {"format": deepcopy(format_spec), **({"verbosity": text["verbosity"]} if "verbosity" in text else {})},
        "top_p": float(request.get("top_p", 1.0)),
        "presence_penalty": float(request.get("presence_penalty", 0.0)),
        "frequency_penalty": float(request.get("frequency_penalty", 0.0)),
        "top_logprobs": int(request.get("top_logprobs", 0)),
        "temperature": float(request.get("temperature", 1.0)),
        "reasoning": deepcopy(reasoning),
        "usage": None,
        "max_output_tokens": request.get("max_output_tokens"),
        "max_tool_calls": request.get("max_tool_calls"),
        "store": request.get("store", True),
        "background": request.get("background", False),
        "service_tier": "default",
        "metadata": deepcopy(request.get("metadata") or {}),
        "safety_identifier": request.get("safety_identifier"),
        "prompt_cache_key": request.get("prompt_cache_key"),
    }


class ResponseCoordinator:
    """The Responses-specific half of running a turn.

    Everything here is about Responses semantics — the item/context graph,
    `previous_response_id`, truncation of items, and persistence. The neutral
    execution of a compiled turn lives in `TurnRunner`, which the Chat surface
    drives the same way.
    """

    def __init__(self, backend: ModelBackend, store: SQLiteResponseStore) -> None:
        self.backend = backend
        self.store = store
        self.runner = TurnRunner(backend)

    @property
    def active(self) -> dict[str, tuple[str, asyncio.Task[Any] | None]]:
        """In-flight responses. The runner owns the registry; this is the view
        the service and its tests have always read."""
        return self.runner.active

    async def resolve_context(
        self,
        request: dict[str, Any],
        *,
        connection_cache: dict[str, StoredResponse] | None = None,
    ) -> list[dict[str, Any]]:
        previous_id = request.get("previous_response_id")
        context: list[dict[str, Any]] = []
        if previous_id:
            stored = connection_cache.get(previous_id) if connection_cache else None
            if stored is None:
                stored = await self.store.get(previous_id)
            if stored is None:
                raise ResponsesError(
                    "previous_response_not_found",
                    f"Previous response {previous_id!r} was not found.",
                    404,
                    "previous_response_id",
                )
            context.extend(deepcopy(stored.context_items))
            context.extend(deepcopy(stored.response.get("output") or []))
        context.extend(deepcopy(request["input"]))
        validate_tool_outputs(context)
        return context

    async def run(
        self,
        request: dict[str, Any],
        *,
        sink: EventSink | None = None,
        response_id: str | None = None,
        connection_cache: dict[str, StoredResponse] | None = None,
    ) -> dict[str, Any]:
        response = response_shell(request, response_id)
        generation_id = new_id("generation")
        assembler: ResponseEventAssembler | None = None
        context: list[dict[str, Any]] = []
        async with self.runner.slot(response["id"], generation_id):
            try:
                capabilities = await self.backend.capabilities(request["model"])
                validate_capabilities(request, capabilities)
                context = await self.resolve_context(request, connection_cache=connection_cache)
                turn = await self.backend.compile(request, context, generation_id)
                estimate = await self.backend.count_tokens(turn)
                if estimate.input_tokens > capabilities.context_length:
                    if request.get("truncation") != "auto":
                        raise ResponsesError(
                            "context_length_exceeded",
                            f"Compiled input has {estimate.input_tokens} tokens; model limit is {capabilities.context_length}.",
                            400,
                            "input",
                        )
                    context, turn = await self._truncate(request, context, generation_id, capabilities.context_length)
                assembler = ResponseEventAssembler(response, turn.bindings, sink)
                final = await self.runner.drive(turn, assembler)
            except ResponsesError as error:
                if assembler is None:
                    raise
                final = await assembler.fail(error)
        if request.get("store"):
            await self.store.put(final, request, context)
        elif connection_cache is not None and final["status"] == "completed":
            connection_cache[final["id"]] = StoredResponse(final, request, context)
        return final

    async def _truncate(
        self,
        request: dict[str, Any],
        context: list[dict[str, Any]],
        generation_id: str,
        limit: int,
    ) -> tuple[list[dict[str, Any]], Any]:
        trimmed = list(context)
        removed: list[str] = []
        while len(trimmed) > 1:
            candidate = trimmed.pop(0)
            call_id = candidate.get("call_id")
            if candidate.get("type", "").endswith("_call") and call_id:
                paired = next((item for item in trimmed if item.get("call_id") == call_id and item.get("type", "").endswith("_output")), None)
                if paired is not None:
                    trimmed.remove(paired)
                    removed.append(str(paired.get("id") or call_id))
            elif candidate.get("type", "").endswith("_output") and call_id:
                call = next((item for item in trimmed if item.get("call_id") == call_id and item.get("type", "").endswith("_call")), None)
                if call is not None:
                    trimmed.remove(call)
                    removed.append(str(call.get("id") or call_id))
            removed.append(str(candidate.get("id") or "unknown"))
            turn = await self.backend.compile(request, trimmed, generation_id)
            if (await self.backend.count_tokens(turn)).input_tokens <= limit:
                extension = request.setdefault("x_synth", {})
                if isinstance(extension, dict):
                    extension["truncated_item_ids"] = removed
                return trimmed, turn
        raise ResponsesError(
            "context_length_exceeded",
            "The request cannot fit the model context window without dropping required items.",
            400,
            "input",
        )

    async def cancel(self, response_id: str) -> bool:
        if not self.runner.is_active(response_id):
            stored = await self.store.get(response_id)
            return bool(stored and stored.response.get("status") == "cancelled")
        return await self.runner.cancel(response_id)
