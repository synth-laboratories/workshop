from __future__ import annotations

import json
from typing import Any, AsyncIterator

import httpx

from ..capabilities import ModelCapabilities
from ..compiler import compile_turn
from ..errors import ResponsesError
from .protocol import CompiledTurn, ModelEvent, TokenUsageEstimate


def _passthrough_body(turn: CompiledTurn) -> dict[str, Any]:
    """Build the upstream request body for a native Responses passthrough.

    The upstream gateway behind this backend is itself a stateless native
    Responses passthrough: it holds no `previous_response_id` session store
    and must not be asked to keep one. `turn.context_items` is the
    coordinator's fully resolved conversation — any `previous_response_id`
    history already flattened in as ordinary `message`/`function_call`/
    `function_call_output` items — so it, not the client's original short
    `input`, is what travels upstream as `input`. `previous_response_id` is
    dropped (the id belongs to this daemon's local id space, not the
    upstream's, and is meaningless there) and `store` is forced to `False`:
    this backend never asks the upstream to retain anything, and retains
    nothing itself.
    """
    body = {**turn.request, "input": turn.context_items, "store": False}
    body.pop("previous_response_id", None)
    return body


class RemoteResponsesBackend:
    """Native Responses passthrough adapter; Chat Completions is never used."""

    def __init__(self, base_url: str, api_key: str | None, context_length: int) -> None:
        self.base_url = base_url.rstrip("/")
        self.api_key = api_key
        self._capabilities = ModelCapabilities(context_length=context_length)

    async def capabilities(self, model: str) -> ModelCapabilities:
        return self._capabilities

    async def compile(
        self,
        request: dict[str, Any],
        context_items: list[dict[str, Any]],
        generation_id: str,
    ) -> CompiledTurn:
        return compile_turn(request, context_items, generation_id)

    async def compile_messages(self, **kwargs: Any) -> CompiledTurn:
        raise ResponsesError(
            "chat_requires_local_backend",
            "This daemon is configured with a remote native Responses provider. "
            "The remote passthrough forwards the original Responses body upstream, "
            "so it cannot serve the Chat Completions surface without fabricating "
            "one. Use the native Responses surface, or configure the local MLX "
            "backend.",
            501,
            error_type="server_error",
        )

    async def count_tokens(self, turn: CompiledTurn) -> TokenUsageEstimate:
        headers = {"Authorization": f"Bearer {self.api_key}"} if self.api_key else None
        async with httpx.AsyncClient(timeout=30) as client:
            response = await client.post(
                f"{self.base_url}/v1/responses/input_tokens",
                json=_passthrough_body(turn),
                headers=headers,
            )
        if response.status_code == 404:
            raise ResponsesError(
                "native_responses_unavailable",
                "The configured remote backend has no native /v1/responses surface.",
                502,
                error_type="server_error",
            )
        response.raise_for_status()
        return TokenUsageEstimate(int(response.json().get("input_tokens") or 0))

    async def stream(self, turn: CompiledTurn) -> AsyncIterator[ModelEvent]:
        headers = {"Authorization": f"Bearer {self.api_key}"} if self.api_key else None
        body = {**_passthrough_body(turn), "stream": False}
        async with httpx.AsyncClient(timeout=300) as client:
            response = await client.post(
                f"{self.base_url}/v1/responses", json=body, headers=headers
            )
        if response.status_code == 404:
            raise ResponsesError(
                "native_responses_unavailable",
                "The configured remote backend has no native /v1/responses surface; Chat fallback is prohibited.",
                502,
                error_type="server_error",
            )
        if response.status_code >= 400:
            raise ResponsesError(
                "remote_responses_error",
                response.text[:800],
                502,
                error_type="server_error",
            )
        payload = response.json()
        for item in payload.get("output") or []:
            kind = item.get("type")
            if kind == "message":
                for part in item.get("content") or []:
                    if part.get("type") == "output_text":
                        yield ModelEvent(kind="text_delta", delta=str(part.get("text") or ""))
            elif kind == "reasoning":
                for part in item.get("summary") or []:
                    yield ModelEvent(kind="reasoning_delta", delta=str(part.get("text") or ""))
            elif kind in {
                "function_call",
                "custom_tool_call",
                "tool_search_call",
                "shell_call",
                "apply_patch_call",
                "mcp_call",
            }:
                yield ModelEvent(
                    kind=kind,
                    name=item.get("name"),
                    call_id=item.get("call_id"),
                    arguments=item.get("arguments"),
                    input=item.get("input"),
                    item=item,
                )
        usage = payload.get("usage") or {}
        yield ModelEvent(
            kind="usage",
            input_tokens=usage.get("input_tokens"),
            output_tokens=usage.get("output_tokens"),
            reasoning_tokens=((usage.get("output_tokens_details") or {}).get("reasoning_tokens") or 0),
        )
        yield ModelEvent(kind="finish", finish_reason="stop")

    async def cancel(self, generation_id: str) -> None:
        return None

    async def close(self) -> None:
        return None
