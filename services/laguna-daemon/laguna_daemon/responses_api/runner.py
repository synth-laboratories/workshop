from __future__ import annotations

import asyncio
import time
from contextlib import aclosing, asynccontextmanager
from typing import Any, AsyncIterator, Protocol

from .backends.protocol import CompiledTurn, ModelBackend, ModelEvent
from .errors import ResponsesError
from .telemetry import GenerationTiming, InferenceTelemetry


class TurnRenderer(Protocol):
    """Turns canonical `ModelEvent`s into one wire protocol's objects.

    Each wire surface supplies its own renderer. `ResponseEventAssembler` builds
    Responses items and semantic SSE events; `ChatEventAssembler` builds Chat
    completion chunks. Neither is defined in terms of the other, and the runner
    below does not know which one it is driving.
    """

    async def start(self) -> None: ...

    async def consume(self, model_event: ModelEvent) -> str | None: ...

    async def complete(self, finish_reason: str) -> Any: ...

    async def fail(self, error: ResponsesError) -> Any: ...


class TurnRunner:
    """Protocol-neutral execution of one compiled turn.

    This owns the parts of running a generation that every wire surface needs
    identically: the in-flight registry used for cancellation, propagating a
    cancel to the backend, and driving the model stream at the ownership
    boundary where the scarce generation slot must be released.
    """

    def __init__(
        self, backend: ModelBackend, telemetry: InferenceTelemetry | None = None
    ) -> None:
        self.backend = backend
        self.telemetry = telemetry or InferenceTelemetry()
        self.active: dict[str, tuple[str, asyncio.Task[Any] | None]] = {}

    @asynccontextmanager
    async def slot(self, key: str, generation_id: str) -> AsyncIterator[None]:
        """Hold the in-flight registration for `key` across a whole turn.

        Registration deliberately spans compilation as well as generation: a
        cancel that arrives while a large Codex prompt is still compiling must
        find the entry and stop the work, not fall through and let it run on.
        """
        self.active[key] = (generation_id, asyncio.current_task())
        try:
            yield
        except asyncio.CancelledError:
            await self.backend.cancel(generation_id)
            raise
        finally:
            self.active.pop(key, None)

    async def drive(self, turn: CompiledTurn, renderer: TurnRenderer) -> Any:
        """Stream one compiled turn through a renderer and return its result."""
        started = time.monotonic()
        await renderer.start()
        finish_reason = "stop"
        # A backend stream owns scarce generation resources (the native MLX
        # implementation holds the single GPU admission slot). An exception in
        # the renderer does not automatically close an async iterator, which
        # can strand that slot after the worker itself has already gone idle.
        # Always close the stream at this ownership boundary.
        try:
            async with aclosing(self.backend.stream(turn)) as model_events:
                async for model_event in model_events:
                    observed = await renderer.consume(model_event)
                    if observed is not None:
                        finish_reason = observed
        except asyncio.CancelledError:
            self.telemetry.record_cancelled()
            raise
        except BaseException:
            self.telemetry.record_failed()
            raise
        latency_ms = round((time.monotonic() - started) * 1000, 3)
        if finish_reason == "cancelled":
            self.telemetry.record_cancelled()
        else:
            self.telemetry.record_completed(
                self.generation_timing(turn.generation_id), latency_ms
            )
        return await renderer.complete(finish_reason)

    def generation_timing(self, generation_id: str) -> GenerationTiming | None:
        """Real per-generation timings, when the backend measures them."""
        metrics = getattr(self.backend, "generation_metrics", None)
        if metrics is None:
            return None
        return metrics(generation_id)

    def is_active(self, key: str) -> bool:
        return key in self.active

    async def cancel(self, key: str) -> bool:
        """Cancel an in-flight turn. Returns False when it is not running here."""
        entry = self.active.get(key)
        if entry is None:
            return False
        generation_id, task = entry
        await self.backend.cancel(generation_id)
        if task is not None:
            task.cancel()
        return True
