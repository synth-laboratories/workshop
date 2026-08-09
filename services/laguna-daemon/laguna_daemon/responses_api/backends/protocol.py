from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, AsyncIterator, Protocol

from ..capabilities import ModelCapabilities


@dataclass(frozen=True, slots=True)
class ToolBinding:
    model_name: str
    original_name: str
    kind: str
    schema: dict[str, Any] | None = None
    format: dict[str, Any] | None = None
    namespace: str | None = None
    caller: str = "client"
    authorization: str = "client_delegated"


@dataclass(frozen=True, slots=True)
class CompiledTurn:
    generation_id: str
    model: str
    request: dict[str, Any]
    context_items: list[dict[str, Any]]
    messages: list[dict[str, Any]]
    prompt: str | list[int] | None
    tools: list[dict[str, Any]]
    bindings: dict[str, ToolBinding]
    max_output_tokens: int
    temperature: float
    top_p: float
    structured_format: dict[str, Any] | None = None


@dataclass(frozen=True, slots=True)
class TokenUsageEstimate:
    input_tokens: int
    cached_tokens: int = 0


@dataclass(frozen=True, slots=True)
class ModelEvent:
    kind: str
    delta: str = ""
    name: str | None = None
    namespace: str | None = None
    call_id: str | None = None
    arguments: str | None = None
    input: str | None = None
    item: dict[str, Any] | None = None
    input_tokens: int | None = None
    output_tokens: int | None = None
    reasoning_tokens: int = 0
    finish_reason: str | None = None
    error_code: str | None = None
    error_message: str | None = None
    metadata: dict[str, Any] = field(default_factory=dict)


class ModelBackend(Protocol):
    async def capabilities(self, model: str) -> ModelCapabilities: ...

    async def compile(
        self,
        request: dict[str, Any],
        context_items: list[dict[str, Any]],
        generation_id: str,
    ) -> CompiledTurn: ...

    async def count_tokens(self, turn: CompiledTurn) -> TokenUsageEstimate: ...

    def stream(self, turn: CompiledTurn) -> AsyncIterator[ModelEvent]: ...

    async def cancel(self, generation_id: str) -> None: ...

    async def close(self) -> None: ...
