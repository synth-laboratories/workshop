from __future__ import annotations

import asyncio
import base64
import hashlib
import json
import os
import time
from collections import deque
from copy import deepcopy
from typing import Any, AsyncIterator, Awaitable, Callable

from ..config import LagunaConfig
from .backends import FakeBackend, NativeMlxBackend, RemoteResponsesBackend
from .backends.protocol import ModelBackend
from .compaction import Compactor
from .coordinator import ResponseCoordinator, response_shell
from .errors import ResponsesError
from .events import sse_frame
from .fixtures import FixtureRecorder
from .ids import new_id
from .policies import PolicyError, PolicyRegistry
from .storage import SQLiteResponseStore, StoredResponse
from .validation import normalize_request


def _redact_id(generation_id: str) -> str:
    """Short, stable, non-reversible handle for a generation.

    Telemetry must never carry a complete stable id that could be correlated
    back to stored content, so the monitor gets a truncated digest instead.
    """
    return "sha256:" + hashlib.sha256(generation_id.encode()).hexdigest()[:12]


class ResponsesService:
    CORE_SPEC_VERSION = "2.3.0"
    CORE_SPEC_DATE = "2026-04-24"
    CORE_SPEC_COMMIT = "cd31bc2060a27ee87a05ec97f49c84027eb6c3ba"
    CORE_SCHEMA_SHA256 = "b445f548d7d13da7768c06cab4317c59928a46a35847d4a515b94c75b8294c87"
    OPENAI_SCHEMA_COMMIT = "c309ca176bc22c6075a0c2c2543f2ac4f307c447"
    OPENAI_SCHEMA_SHA256 = "cdabcdfc529b1ec0582009bb2ef7d06b64a66d4f6644e66142305a48f0b7658d"

    def __init__(
        self,
        config: LagunaConfig,
        backend: ModelBackend | None = None,
        settings: Any = None,
    ) -> None:
        from ..settings import SettingsStore

        self.config = config
        self.settings = settings or SettingsStore.load(config)
        self.backend = backend or self._make_backend(config)
        # One shared defaults object: the settings store mutates it, both
        # compile front-ends read it. Explicitly injected test backends get
        # it too, so PUT /v1/synth/settings behaves identically under mock.
        self.backend.sampling_defaults = self.settings.sampling
        self.settings.apply_to_backend(self.backend)
        # Policies are the daemon's selectable model ids: the base weights plus
        # any registered adapter. The backend resolves the request's `model`
        # against this rather than against process-global adapter state.
        self.policies = PolicyRegistry(config.data_dir, config.default_model)
        register = getattr(self.backend, "set_policy_registry", None)
        if callable(register):
            register(self.policies)
        self.store = SQLiteResponseStore(config.data_dir / "responses.sqlite3")
        self.coordinator = ResponseCoordinator(self.backend, self.store)
        self.compactor = Compactor(config.data_dir / "compaction.key")
        self.background_tasks: dict[str, asyncio.Task[None]] = {}
        self.background_slots = asyncio.Semaphore(4)
        self.telemetry: deque[dict[str, Any]] = deque(maxlen=256)
        self.fixtures = FixtureRecorder()

    @staticmethod
    def _make_backend(config: LagunaConfig) -> ModelBackend:
        if config.backend == "mock":
            return FakeBackend(context_length=config.context_length)
        if config.backend == "external":
            return RemoteResponsesBackend(
                config.upstream_url, config.upstream_api_key, config.context_length
            )
        model_path = config.resolve_model_path(config.model) or (config.models_dir / config.model)
        return NativeMlxBackend(
            model_path=model_path,
            adapter_path=config.adapter,
            context_length=config.context_length,
        )

    async def start(self) -> None:
        await self.store.start()

    async def close(self) -> None:
        for task in self.background_tasks.values():
            task.cancel()
        if self.background_tasks:
            await asyncio.gather(*self.background_tasks.values(), return_exceptions=True)
        self.background_tasks.clear()
        await self.backend.close()
        await self.store.close()

    @property
    def idle_unload_after_seconds(self) -> int:
        """Effective idle deadline: runtime settings win over startup config."""
        return int(self.settings.idle_unload_after_seconds)

    async def unload_if_idle(self) -> bool:
        unload = getattr(self.backend, "unload_if_idle", None)
        if unload is None:
            return False
        return bool(await unload(self.idle_unload_after_seconds))

    async def watch_idle(self) -> None:
        """Evict native Responses weights without shutting down the daemon."""
        while True:
            await asyncio.sleep(1.0)
            await self.unload_if_idle()

    async def watch_memory_pressure(self) -> None:
        """Evict idle MLX or terminate this sidecar under sustained emergency."""
        relieve = getattr(self.backend, "relieve_memory_pressure", None)
        if relieve is None:
            while True:
                await asyncio.sleep(5.0)
        emergencies = 0
        while True:
            await asyncio.sleep(1.0)
            outcome = await relieve()
            emergencies = emergencies + 1 if outcome == "active_emergency" else 0
            if emergencies >= 2:
                # Native prefill may be blocked in a worker and unable to honor
                # cooperative cancellation. Immediate sidecar exit is the one
                # bounded way to return its Metal allocation to macOS.
                os._exit(70)

    def residency(self) -> dict[str, Any] | None:
        residency = getattr(self.backend, "residency", None)
        if residency is None:
            return None
        return residency(self.idle_unload_after_seconds)

    def normalize(self, body: Any) -> dict[str, Any]:
        request = normalize_request(body, default_model=self.config.default_model)
        self.require_policy(request.get("model"))
        return request

    def require_policy(self, model: str | None) -> None:
        """Reject an unregistered model id before a response object exists.

        The Responses contract turns a mid-generation failure into a stored
        response with `status: failed` and HTTP 200. An unknown model is a
        request error, not a failed generation, so it is refused up front and
        never becomes a turn the client has to inspect to discover was wrong.
        """
        try:
            self.policies.resolve(model)
        except PolicyError as error:
            raise ResponsesError(
                "model_not_found",
                str(error),
                404,
                error_type="invalid_request_error",
            ) from error

    def capture(self, body: Any, *, transport: str) -> None:
        self.fixtures.capture(body, transport=transport)

    def capture_response(self, body: Any, *, transport: str) -> None:
        self.fixtures.capture(body, transport=transport, kind="response")

    async def create(self, body: Any) -> dict[str, Any]:
        request = self.normalize(body)
        request = self._prepare_compacted_request(request)
        if request["background"]:
            return await self._start_background(request)
        started = time.monotonic()
        response = await self.coordinator.run(request)
        self._record(response, started)
        self.capture_response(response, transport="http")
        return response

    async def stream(
        self,
        body: Any,
        *,
        disconnected: Callable[[], Awaitable[bool]] | None = None,
    ) -> AsyncIterator[bytes]:
        request = self.normalize(body)
        request = self._prepare_compacted_request(request)
        request["stream"] = True
        queue: asyncio.Queue[dict[str, Any] | BaseException | None] = asyncio.Queue()
        started = time.monotonic()

        async def sink(event: dict[str, Any]) -> None:
            await queue.put(event)

        async def run() -> None:
            try:
                response = await self.coordinator.run(request, sink=sink)
                self._record(response, started)
                self.capture_response(response, transport="http-stream")
            except BaseException as exc:
                await queue.put(exc)
            finally:
                await queue.put(None)

        task = asyncio.create_task(run(), name="responses-sse")
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
                    # Large local prompts can spend tens of seconds in MLX
                    # prefill before the first token. SSE comments keep SDK
                    # and Codex idle timers alive without inventing semantic
                    # events or consuming sequence numbers.
                    yield b": keep-alive\n\n"
                    continue
                if entry is None:
                    yield b"data: [DONE]\n\n"
                    break
                if isinstance(entry, BaseException):
                    if isinstance(entry, ResponsesError):
                        event = {
                            "type": "error",
                            "sequence_number": 0,
                            "error": entry.payload()["error"],
                        }
                        yield sse_frame(event)
                        yield b"data: [DONE]\n\n"
                        break
                    raise entry
                yield sse_frame(entry)
        finally:
            if not task.done():
                task.cancel()
                await asyncio.gather(task, return_exceptions=True)

    async def websocket_turn(
        self,
        body: Any,
        connection_cache: dict[str, StoredResponse],
    ) -> AsyncIterator[dict[str, Any]]:
        if not isinstance(body, dict) or body.get("type") != "response.create":
            raise ResponsesError(
                "invalid_websocket_event",
                "WebSocket messages must have type response.create.",
                400,
                "type",
            )
        request_body = {key: value for key, value in body.items() if key != "type"}
        request_body["stream"] = True
        request = self.normalize(request_body)
        request = self._prepare_compacted_request(request)
        queue: asyncio.Queue[dict[str, Any] | BaseException | None] = asyncio.Queue()
        previous_id = request.get("previous_response_id")

        async def sink(event: dict[str, Any]) -> None:
            await queue.put(event)

        async def run() -> None:
            try:
                response = await self.coordinator.run(
                    request,
                    sink=sink,
                    connection_cache=connection_cache,
                )
                self.capture_response(response, transport="websocket")
            except BaseException as exc:
                if previous_id and not request.get("store"):
                    connection_cache.pop(previous_id, None)
                await queue.put(exc)
            finally:
                await queue.put(None)

        task = asyncio.create_task(run(), name="responses-websocket-turn")
        try:
            while True:
                entry = await queue.get()
                if entry is None:
                    break
                if isinstance(entry, ResponsesError):
                    yield {
                        "type": "error",
                        "sequence_number": 0,
                        "error": entry.payload()["error"],
                    }
                    break
                if isinstance(entry, BaseException):
                    raise entry
                yield entry
        finally:
            if not task.done():
                task.cancel()
                await asyncio.gather(task, return_exceptions=True)

    async def compact(self, body: Any) -> dict[str, Any]:
        if not isinstance(body, dict) or not body.get("model"):
            raise ResponsesError("invalid_request", "model is required.", 400, "model")
        request = self.normalize({**body, "stream": False, "store": False})
        items = self._expand_compactions(request["input"])
        turn = await self.backend.compile(request, items, new_id("generation"))
        usage = await self.backend.count_tokens(turn)
        return self.compactor.compact(items, usage.input_tokens)

    def _expand_compactions(self, items: list[dict[str, Any]]) -> list[dict[str, Any]]:
        expanded: list[dict[str, Any]] = []
        for item in items:
            if item.get("type") == "compaction":
                expanded.extend(self.compactor.expand(item))
            else:
                expanded.append(item)
        return expanded

    def _prepare_compacted_request(self, request: dict[str, Any]) -> dict[str, Any]:
        compactions = [item for item in request["input"] if item.get("type") == "compaction"]
        if not compactions:
            return request
        prepared = deepcopy(request)
        prepared["input"] = self._expand_compactions(prepared["input"])
        extension = prepared.setdefault("x_synth", {})
        if isinstance(extension, dict):
            extension["compaction_items"] = compactions
        return prepared

    async def input_tokens(self, body: Any) -> dict[str, Any]:
        request = self.normalize({**body, "stream": False, "background": False})
        context = await self.coordinator.resolve_context(request)
        context = self._expand_compactions(context)
        turn = await self.backend.compile(request, context, new_id("generation"))
        usage = await self.backend.count_tokens(turn)
        return {"object": "response.input_tokens", "input_tokens": usage.input_tokens}

    async def get(self, response_id: str) -> dict[str, Any]:
        stored = await self.store.get(response_id)
        if stored is None:
            raise ResponsesError("response_not_found", "Response not found.", 404, "response_id")
        return stored.response

    async def delete(self, response_id: str) -> dict[str, Any]:
        if not await self.store.delete(response_id):
            raise ResponsesError("response_not_found", "Response not found.", 404, "response_id")
        return {"id": response_id, "object": "response.deleted", "deleted": True}

    async def cancel(self, response_id: str) -> dict[str, Any]:
        task = self.background_tasks.get(response_id)
        if task is not None and not task.done():
            task.cancel()
            await asyncio.gather(task, return_exceptions=True)
        await self.coordinator.cancel(response_id)
        stored = await self.store.get(response_id)
        if stored is None:
            raise ResponsesError("response_not_found", "Response not found.", 404, "response_id")
        response = stored.response
        if response.get("status") not in {"completed", "failed", "incomplete", "cancelled"}:
            response["status"] = "cancelled"
            response["completed_at"] = int(time.time())
            response["error"] = {"code": "cancelled", "message": "Response cancelled."}
            await self.store.put(response, stored.request, stored.context_items)
        return response

    async def input_items(
        self,
        response_id: str,
        *,
        limit: int = 20,
        order: str = "desc",
        after: str | None = None,
    ) -> dict[str, Any]:
        stored = await self.store.get(response_id)
        if stored is None:
            raise ResponsesError("response_not_found", "Response not found.", 404, "response_id")
        limit = min(100, max(1, limit))
        items = list(stored.context_items)
        if order == "desc":
            items.reverse()
        start = self._decode_cursor(after, response_id) if after else 0
        page = items[start : start + limit]
        next_index = start + len(page)
        has_more = next_index < len(items)
        return {
            "object": "list",
            "data": page,
            "first_id": page[0].get("id") if page else None,
            "last_id": page[-1].get("id") if page else None,
            "has_more": has_more,
            "next": self._encode_cursor(next_index, response_id) if has_more else None,
        }

    def _encode_cursor(self, index: int, response_id: str) -> str:
        digest = hashlib.sha256(response_id.encode()).hexdigest()[:12]
        return base64.urlsafe_b64encode(f"{index}:{digest}".encode()).decode().rstrip("=")

    def _decode_cursor(self, cursor: str, response_id: str) -> int:
        try:
            raw = base64.urlsafe_b64decode(cursor + "=" * (-len(cursor) % 4)).decode()
            index_text, digest = raw.split(":", 1)
            expected = hashlib.sha256(response_id.encode()).hexdigest()[:12]
            if digest != expected:
                raise ValueError
            return max(0, int(index_text))
        except (ValueError, UnicodeDecodeError) as exc:
            raise ResponsesError("invalid_cursor", "Invalid pagination cursor.", 400, "after") from exc

    async def _start_background(self, request: dict[str, Any]) -> dict[str, Any]:
        response_id = new_id("response")
        queued = response_shell(request, response_id)
        queued["status"] = "queued"
        await self.store.put(queued, request, request["input"])

        async def runner() -> None:
            try:
                async with self.background_slots:
                    await self.coordinator.run(request, response_id=response_id)
            except asyncio.CancelledError:
                stored = await self.store.get(response_id)
                if stored:
                    cancelled = stored.response
                    cancelled["status"] = "cancelled"
                    cancelled["completed_at"] = int(time.time())
                    cancelled["error"] = {"code": "cancelled", "message": "Response cancelled."}
                    await self.store.put(cancelled, request, stored.context_items)
                raise
            finally:
                self.background_tasks.pop(response_id, None)

        self.background_tasks[response_id] = asyncio.create_task(runner(), name=f"background-{response_id}")
        return queued

    def _record(self, response: dict[str, Any], started: float) -> None:
        usage = response.get("usage") or {}
        self.telemetry.append(
            {
                "response_id": response["id"],
                "model": response["model"],
                "backend": type(self.backend).__name__,
                "latency_ms": round((time.monotonic() - started) * 1000, 3),
                "input_tokens": usage.get("input_tokens", 0),
                "output_tokens": usage.get("output_tokens", 0),
                "item_count": len(response.get("output") or []),
                "status": response["status"],
                "error_code": (response.get("error") or {}).get("code"),
            }
        )

    def inference_snapshot(self) -> dict[str, Any]:
        """Redacted live inference state for the Desktop monitor.

        Sourced entirely from the backend's own timestamps and counters. Any
        metric that was not measured is null so the surface renders
        "Unavailable" rather than a plausible invention. No prompt text,
        reasoning text, tool input or output, credential, file content, or
        complete response id can appear here — the only identifier is a short
        hash of the generation id.
        """
        residency = self.residency()
        resident = bool(residency and residency["loaded"])
        queue = getattr(self.backend, "queue_state", None)
        queue_state = queue() if callable(queue) else {"depth": 0, "capacity": 0}
        active_fn = getattr(self.backend, "active_generation", None)
        active = active_fn() if callable(active_fn) else None
        diagnostics = getattr(self.backend, "diagnostics", None)
        runtime = diagnostics() if callable(diagnostics) else {}

        active_payload: dict[str, Any] | None = None
        if active is not None:
            now = time.monotonic()
            phase = "loading" if runtime.get("loading") else active.phase
            active_payload = {
                "generationId": _redact_id(active.generation_id),
                "phase": phase,
                "queuedAt": round(active.queued_at * 1000, 3),
                "startedAt": (
                    round(active.admitted_at * 1000, 3)
                    if active.admitted_at is not None
                    else None
                ),
                "firstTokenAt": (
                    round(active.first_token_at * 1000, 3)
                    if active.first_token_at is not None
                    else None
                ),
                "lastTokenAt": (
                    round(active.last_token_at * 1000, 3)
                    if active.last_token_at is not None
                    else None
                ),
                "promptTokens": active.prompt_tokens,
                "cachedTokens": active.cached_tokens,
                "outputTokens": active.output_tokens,
                "cacheHitRatio": active.cache_hit_ratio(),
                "prefillTokensPerSecond": active.prefill_tokens_per_second(),
                "decodeTokensPerSecond": active.decode_tokens_per_second(),
                "elapsedMs": active.elapsed_ms(now),
            }

        return {
            "model": self.config.default_model,
            "resident": resident,
            # Real allocator bytes, or null when the backend cannot measure
            # them. Summing the weights' on-disk size would report a fact about
            # the filesystem under a name that promises a fact about memory.
            "residentBytes": self.memory_bytes(),
            "queueDepth": queue_state["depth"],
            "queueCapacity": queue_state["capacity"],
            "active": active_payload,
            "rolling": self.coordinator.runner.telemetry.snapshot(),
            # Per-policy decode speed, so a picker can show what a LoRA costs
            # instead of implying every model id runs at one rate.
            "policies": self.coordinator.runner.telemetry.policy_snapshot(
                self.policies.default_model
            ),
            # Apple GPU utilization counters are not reliably available, and
            # deriving one from process CPU would be a fabrication.
            "gpuUtilization": None,
        }

    def memory_bytes(self) -> int | None:
        measure = getattr(self.backend, "memory_bytes", None)
        return measure() if callable(measure) else None

    def prefill_histogram(self) -> dict[str, Any]:
        """Rolling prompt-size buckets, served by the control surface only:
        the legacy inference payload's field set is pinned by its tests."""
        return self.coordinator.runner.telemetry.prefill_histogram()

    async def unload_now(self) -> bool:
        """Release weights on request, honoring the same eviction guard."""
        unload = getattr(self.backend, "unload", None)
        if unload is not None:
            return bool(await unload())
        diagnostics = getattr(self.backend, "diagnostics", None)
        runtime = diagnostics() if callable(diagnostics) else {}
        if runtime.get("inflight_generations"):
            return False
        release = getattr(self.backend, "_release_model_memory", None)
        if release is None:
            return False
        await release()
        return True

    async def health(self) -> dict[str, Any]:
        capabilities = await self.backend.capabilities(self.config.default_model)
        result = {
            "engine": "native",
            "backend": type(self.backend).__name__,
            "openResponses": {
                "date": self.CORE_SPEC_DATE,
                "version": self.CORE_SPEC_VERSION,
                "commit": self.CORE_SPEC_COMMIT,
                "schemaSha256": self.CORE_SCHEMA_SHA256,
            },
            "openaiExtension": {
                "commit": self.OPENAI_SCHEMA_COMMIT,
                "schemaSha256": self.OPENAI_SCHEMA_SHA256,
            },
            "capabilities": capabilities.json(),
            "conformance": {"portableScenarios": 17, "status": "implemented"},
        }
        diagnostics = getattr(self.backend, "diagnostics", None)
        if callable(diagnostics):
            result["runtime"] = diagnostics()
        return result
