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
    """A protocol-neutral unit of work handed to a backend.

    Everything a backend needs to generate lives in the neutral fields. The
    `request`/`context_items` pair is Responses-only provenance retained for the
    remote passthrough backend, which forwards the original body upstream; a
    Chat turn leaves them empty rather than fabricating a Responses request.
    """

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
    # 0 disables top-k, matching the MLX sampler's own convention. Poolside's
    # generation_config.json recommends 20 for this checkpoint.
    top_k: int = 0
    enable_thinking: bool = True
    structured_format: dict[str, Any] | None = None
    prompt_cache_key: str | None = None


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

    async def compile_messages(self, **kwargs: Any) -> CompiledTurn:
        """Compile an already-neutral turn.

        The peer of `compile` for wire surfaces that are not Responses. Keyword
        arguments are those of `compiler.compile_messages`; the backend supplies
        its own prompt builder and ensures any weights it needs are loaded.
        """
        ...

    async def count_tokens(self, turn: CompiledTurn) -> TokenUsageEstimate: ...

    def stream(self, turn: CompiledTurn) -> AsyncIterator[ModelEvent]: ...

    async def cancel(self, generation_id: str) -> None: ...

    async def close(self) -> None: ...
