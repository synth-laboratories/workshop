from __future__ import annotations

import asyncio
from typing import Any, AsyncIterator, Awaitable, Callable

from ..config import LagunaConfig
from ..responses_api.errors import ResponsesError
from ..responses_api.ids import new_id
from ..responses_api.service import ResponsesService
from .front import compile_chat_turn
from .renderer import ChatEventAssembler, chat_sse_frame


MODEL_ALIASES = {
    "laguna-xs-2.1",
    "synth/Laguna-XS-2.1",
    "synth/Laguna-XS-2.1-NVFP4",
}


class ChatService:
    """The Chat Completions wire surface.

    A peer of `ResponsesService`, not a client of it. Both compile onto the same
    neutral core and execute on the same `TurnRunner`, which is what makes model
    residency, the single GPU admission slot, cancellation, and token accounting
    shared rather than duplicated. Chat is stateless by protocol, so unlike
    Responses it owns no store and no continuation state.
    """

    def __init__(self, config: LagunaConfig, responses: ResponsesService) -> None:
        self.config = config
        self.responses = responses

    @property
    def backend(self) -> Any:
        return self.responses.backend

    @property
    def runner(self) -> Any:
        return self.responses.coordinator.runner

    def normalize(self, body: Any) -> dict[str, Any]:
        if not isinstance(body, dict):
            raise ResponsesError("invalid_request", "The request body must be a JSON object.", 400)
        requested = body.get("model") or self.config.default_model
        if requested in MODEL_ALIASES:
            requested = self.config.default_model
        # Chat is a peer surface, so a policy pin has to be honoured — and
        # refused — here exactly as it is on Responses.
        policies = getattr(self.responses, "policies", None)
        if policies is not None:
            from ..responses_api.policies import PolicyError

            try:
                policies.resolve(requested)
            except PolicyError as error:
                raise ResponsesError(
                    "model_not_found", str(error), 404, error_type="invalid_request_error"
                ) from error
        return {**body, "model": requested}

    async def _prepare(self, body: dict[str, Any]) -> tuple[Any, ChatEventAssembler]:
        generation_id = new_id("generation")
        capabilities = await self.backend.capabilities(body["model"])
        turn = await compile_chat_turn(
            body,
            backend=self.backend,
            generation_id=generation_id,
            default_model=self.config.default_model,
        )
        estimate = await self.backend.count_tokens(turn)
        if estimate.input_tokens > capabilities.context_length:
            # Chat has no `truncation: auto` equivalent, so there is no
            # sanctioned way to silently drop the caller's messages.
            raise ResponsesError(
                "context_length_exceeded",
                f"Compiled input has {estimate.input_tokens} tokens; model limit "
                f"is {capabilities.context_length}.",
                400,
                "messages",
            )
        stream_options = body.get("stream_options") or {}
        assembler = ChatEventAssembler(
            model=turn.model,
            include_usage=bool(
                isinstance(stream_options, dict) and stream_options.get("include_usage")
            ),
        )
        return turn, assembler

    async def create(self, body: Any) -> dict[str, Any]:
        request = self.normalize(body)
        turn, assembler = await self._prepare(request)
        async with self.runner.slot(assembler.id, turn.generation_id):
            return await self.runner.drive(turn, assembler)

    async def open_stream(
        self,
        body: Any,
        *,
        disconnected: Callable[[], Awaitable[bool]] | None = None,
    ) -> AsyncIterator[bytes]:
        """Validate and compile eagerly, then hand back the frame iterator.

        Awaiting the preparation here is what makes a bad streaming request fail
        with a real HTTP error. If preparation happened inside the generator it
        would not run until the first frame was pulled, by which point the
        status line is already sent and a 400 can only be smuggled into the
        stream — which the Responses surface does not do either.
        """
        request = self.normalize(body)
        turn, assembler = await self._prepare(request)
        return self._stream_prepared(turn, assembler, disconnected)

    async def _stream_prepared(
        self,
        turn: Any,
        assembler: ChatEventAssembler,
        disconnected: Callable[[], Awaitable[bool]] | None,
    ) -> AsyncIterator[bytes]:
        queue: asyncio.Queue[dict[str, Any] | BaseException | None] = asyncio.Queue()

        async def sink(chunk: dict[str, Any]) -> None:
            await queue.put(chunk)

        assembler.sink = sink

        async def run() -> None:
            try:
                async with self.runner.slot(assembler.id, turn.generation_id):
                    await self.runner.drive(turn, assembler)
            except BaseException as exc:
                await queue.put(exc)
            finally:
                await queue.put(None)

        task = asyncio.create_task(run(), name="chat-sse")
        try:
            while True:
                if disconnected is not None and await disconnected():
                    task.cancel()
                    await asyncio.gather(task, return_exceptions=True)
                    break
                try:
                    entry = await asyncio.wait_for(
                        queue.get(), timeout=0.25 if disconnected is not None else 5.0
                    )
                except TimeoutError:
                    if disconnected is not None:
                        continue
                    # A large local prompt can spend tens of seconds in MLX
                    # prefill before the first token. SSE comments keep SDK
                    # idle timers alive without emitting a spurious chunk.
                    yield b": keep-alive\n\n"
                    continue
                if entry is None:
                    yield b"data: [DONE]\n\n"
                    break
                if isinstance(entry, BaseException):
                    if isinstance(entry, ResponsesError):
                        yield chat_sse_frame(entry.payload())
                        yield b"data: [DONE]\n\n"
                        break
                    raise entry
                yield chat_sse_frame(entry)
        finally:
            if not task.done():
                task.cancel()
                await asyncio.gather(task, return_exceptions=True)
