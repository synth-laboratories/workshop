from __future__ import annotations

import asyncio
import json
import time
import uuid
from contextlib import asynccontextmanager
from typing import Any, AsyncIterator, Callable

import httpx
from fastapi import FastAPI, Request, WebSocket, WebSocketDisconnect
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import JSONResponse, StreamingResponse
from starlette.middleware.base import BaseHTTPMiddleware

from .config import LagunaConfig
from .manager import LagunaProcessManager
from .responses_api import ResponsesService
from .responses_api.errors import ResponsesError
from .responses import (
    build_chat_body_from_responses,
    iter_response_sse_from_text,
    mock_response_payload,
    responses_input_to_messages,
    response_tool_types,
    sse_event,
    translate_chat_sse_to_responses,
    wrap_chat_as_response,
)


def _sse(payload: dict[str, Any]) -> bytes:
    return f"data: {json.dumps(payload, separators=(',', ':'))}\n\n".encode()


def _openai_error(status: int, message: str) -> JSONResponse:
    return JSONResponse(
        status_code=status,
        content={
            "error": {
                "code": str(status),
                "type": "invalid_request_error",
                "message": message,
            }
        },
    )


def _responses_error(error: ResponsesError) -> JSONResponse:
    return JSONResponse(status_code=error.status_code, content=error.payload())


def _finish_request(response: Any, manager: LagunaProcessManager) -> Any:
    """Release the eviction guard after a normal response or after streaming ends."""
    if isinstance(response, StreamingResponse):
        body_iterator = response.body_iterator

        async def tracked_body() -> AsyncIterator[bytes | str]:
            try:
                async for chunk in body_iterator:
                    yield chunk
            finally:
                manager.end_request()

        response.body_iterator = tracked_body()
    else:
        manager.end_request()
    return response


def _mock_stream(prompt: str) -> list[str]:
    text = (
        "Synth Laguna sidecar (mock). "
        f"Received: {prompt[:240]}"
    )
    words = text.split(" ")
    return [words[0]] + [f" {w}" for w in words[1:]]


class BearerAuthMiddleware(BaseHTTPMiddleware):
    def __init__(self, app: Any, api_key: str | None) -> None:
        super().__init__(app)
        self.api_key = api_key

    async def dispatch(self, request: Request, call_next: Callable):
        if self.api_key is None:
            return await call_next(request)
        # Match Poolside: health still requires auth in practice on their binary
        # for /v1/*; we require auth on all routes except OPTIONS.
        if request.method == "OPTIONS":
            return await call_next(request)
        auth = request.headers.get("Authorization", "")
        if auth != f"Bearer {self.api_key}":
            return _openai_error(401, "missing or invalid bearer token")
        return await call_next(request)


def create_app(config: LagunaConfig | None = None) -> FastAPI:
    cfg = config or LagunaConfig.from_env()
    manager = LagunaProcessManager(cfg)

    @asynccontextmanager
    async def lifespan(_: FastAPI):
        if cfg.auto_load and cfg.backend != "mock":
            await manager.ensure_ready()
        elif cfg.backend == "mock":
            manager.state = "ready"
        watchdog = asyncio.create_task(manager.watch_idle(), name="laguna-idle-unload")
        try:
            yield
        finally:
            watchdog.cancel()
            await manager.shutdown()

    app = FastAPI(
        title="Synth Laguna Sidecar",
        version="0.1.0",
        lifespan=lifespan,
        docs_url=None,
        redoc_url=None,
    )
    app.state.config = cfg
    app.state.manager = manager
    app.add_middleware(
        CORSMiddleware,
        allow_origins=["*"],
        allow_methods=["*"],
        allow_headers=["*"],
    )
    app.add_middleware(BearerAuthMiddleware, api_key=cfg.api_key)

    @app.api_route("/{full_path:path}", methods=["GET", "POST", "DELETE", "PUT", "PATCH"])
    async def catch_unknown(full_path: str, request: Request) -> Any:
        # Only reached if no more-specific route matched — but FastAPI matches
        # this greedily; register concrete routes below via decorator order.
        # We instead register concrete routes first (Python decorators bottom-up
        # for same path isn't an issue when paths differ).
        return _openai_error(404, f"unknown route /{full_path}")

    return app


def build_app(config: LagunaConfig | None = None) -> FastAPI:
    """Build app with Poolside-compatible routes registered before the catch-all."""
    cfg = config or LagunaConfig.from_env()
    manager = LagunaProcessManager(cfg)
    responses_service = ResponsesService(cfg)

    @asynccontextmanager
    async def lifespan(_: FastAPI):
        if cfg.backend == "mock":
            manager.state = "ready"
        elif cfg.auto_load and cfg.responses_engine == "legacy":
            await manager.ensure_ready()
        else:
            manager.state = "unloaded"
        await responses_service.start()
        watchdog = asyncio.create_task(manager.watch_idle(), name="laguna-idle-unload")
        native_watchdog = asyncio.create_task(
            responses_service.watch_idle(), name="laguna-native-idle-unload"
        )
        try:
            yield
        finally:
            watchdog.cancel()
            native_watchdog.cancel()
            await asyncio.gather(watchdog, native_watchdog, return_exceptions=True)
            await responses_service.close()
            await manager.shutdown()

    app = FastAPI(
        title="Synth Laguna Sidecar",
        version="0.1.0",
        lifespan=lifespan,
        docs_url=None,
        redoc_url=None,
    )
    app.state.config = cfg
    app.state.manager = manager
    app.state.responses_service = responses_service
    app.add_middleware(
        CORSMiddleware,
        allow_origins=["*"],
        allow_methods=["*"],
        allow_headers=["*"],
    )
    app.add_middleware(BearerAuthMiddleware, api_key=cfg.api_key)

    def model_id() -> str:
        return cfg.default_model

    def models_payload() -> dict[str, Any]:
        mid = model_id()
        name = mid.split("/")[-1].replace("-", " ")
        item = {
            "id": mid,
            "object": "model",
            "owned_by": mid.split("/")[0] if "/" in mid else "synth",
            "root": mid,
            "name": name,
            "display_name": name,
            "description": "Native local MLX model served by Synth Laguna.",
            "created": int(time.time()),
            "context_length": cfg.context_length,
            "details": {
                "family": "poolside",
                "format": "safetensors",
                "context_length": cfg.context_length,
            },
        }
        codex_item = {
            "slug": mid,
            "display_name": name,
            "description": "Native local MLX model served by Synth Laguna.",
            "default_reasoning_level": "high",
            "supported_reasoning_levels": [
                {"effort": "none", "description": "Answer without a reasoning phase."},
                {"effort": "high", "description": "Use Laguna's reasoning mode."},
                {"effort": "max", "description": "Use the largest local reasoning budget."},
            ],
            "shell_type": "unified_exec",
            "visibility": "list",
            "supported_in_api": True,
            "priority": 1,
            "service_tiers": [],
            "default_service_tier": "default",
            "availability_nux": None,
            "upgrade": None,
            "include_skills_usage_instructions": True,
            "include_plugin_usage_instructions": True,
            "include_apps_usage_instructions": True,
            "supports_reasoning_summary_parameter": True,
            "default_reasoning_summary": "auto",
            "support_verbosity": False,
            "default_verbosity": None,
            "apply_patch_tool_type": "freeform",
            "web_search_tool_type": "text",
            "truncation_policy": {"mode": "tokens", "limit": 10_000},
            "supports_parallel_tool_calls": True,
            "context_window": cfg.context_length,
            "max_context_window": cfg.context_length,
            "auto_compact_token_limit": int(cfg.context_length * 0.9),
            "experimental_supported_tools": [
                "custom",
                "namespace",
                "mcp",
                "shell",
                "apply_patch",
            ],
            "input_modalities": ["text"],
            "supports_search_tool": False,
            "prefer_websockets": True,
            "base_instructions": "You are Codex, a coding agent running on local Synth Laguna.",
        }
        return {
            "object": "list",
            "data": [item],
            # Codex app-server's model manager consumes its native envelope,
            # while OpenAI SDKs consume data. Supplying both is additive.
            "models": [codex_item],
        }

    @app.get("/health")
    async def health() -> dict[str, Any]:
        status = manager.status()
        loaded = model_id() if status["state"] == "ready" else None
        path = cfg.resolve_model_path(loaded) if loaded else None
        memory = 0
        if path is not None:
            memory = sum(
                f.stat().st_size for f in path.rglob("*") if f.is_file()
            )
        last_used_at = manager.last_used_at
        free_at = (
            last_used_at + cfg.idle_unload_after_seconds
            if cfg.idle_unload_after_seconds > 0
            else None
        )
        health_status = "ok" if status["state"] == "ready" else status["state"]
        idle_seconds = manager.idle_seconds
        idle_unload_after_seconds = cfg.idle_unload_after_seconds
        external = await manager.external_health()
        if external is not None:
            health_status = str(external.get("status") or health_status)
            loaded = external.get("loadedModel")
            memory = external.get("memoryBytes") or 0
            idle_seconds = external.get("idleSeconds")
            idle_unload_after_seconds = external.get("idleUnloadAfterSeconds")
            last_used_at = external.get("lastUsedAt")
            free_at = external.get("freeAt")
        else:
            last_used_at = int(last_used_at * 1000)
            free_at = int(free_at * 1000) if free_at is not None else None
        native_health = await responses_service.health()
        if cfg.responses_engine != "legacy":
            # Native Responses owns the in-process model and loads it lazily;
            # the legacy manager's upstream-process state is not its readiness
            # signal. Advertise the validated local model as available so
            # Desktop can issue the first request that performs the load.
            health_status = "ok"
            native_residency = responses_service.residency()
            native_resident = bool(native_residency and native_residency["loaded"])
            loaded = cfg.default_model if native_resident else None
            native_path = cfg.resolve_model_path(cfg.default_model) if native_resident else None
            if native_path is not None:
                memory = sum(
                    file.stat().st_size
                    for file in native_path.rglob("*")
                    if file.is_file()
                )
            else:
                memory = 0
            if native_residency is not None:
                idle_seconds = native_residency["idle_seconds"]
                last_used_at = native_residency["last_used_at"]
                free_at = native_residency["free_at"]
        return {
            "status": health_status,
            "responsesApi": True,
            "modelsDirectory": str(cfg.models_dir),
            "defaultModel": cfg.default_model,
            "loadedModel": loaded,
            "memoryBytes": memory,
            "idleSeconds": idle_seconds,
            "idleUnloadAfterSeconds": idle_unload_after_seconds,
            "lastUsedAt": last_used_at,
            "freeAt": free_at,
            "backend": cfg.backend,
            "publicUrl": cfg.public_url,
            "responses": native_health,
            "responsesEngine": cfg.responses_engine,
        }

    @app.get("/v1/models")
    async def list_models() -> dict[str, Any]:
        if cfg.responses_engine == "legacy":
            await manager.ensure_ready()
        return models_payload()

    @app.post("/v1/chat/completions")
    async def chat_completions(request: Request) -> Any:
        body = await request.json()
        manager.begin_request()
        stream = bool(body.get("stream"))
        requested = body.get("model") or cfg.default_model
        # Normalize aliases → default model id (Poolside uses full HF-style id)
        aliases = {
            "laguna-xs-2.1",
            "synth/Laguna-XS-2.1",
            "synth/Laguna-XS-2.1-NVFP4",
        }
        if requested in aliases:
            requested = cfg.default_model
        body = {**body, "model": requested}

        try:
            status = await manager.ensure_ready()
            if status["state"] != "ready":
                return _finish_request(
                    _openai_error(503, status.get("lastError") or "model not ready"),
                    manager,
                )

            if cfg.backend == "mock":
                prompt = ""
                for message in reversed(body.get("messages") or []):
                    if message.get("role") == "user":
                        prompt = str(message.get("content") or "")
                        break
                response = await _mock_completion(requested, prompt, stream=stream)
            else:
                # Upstream mlx_lm may not know our alias; pass resolved filesystem path when possible
                path = cfg.resolve_model_path(requested)
                upstream_body = dict(body)
                if path is not None and cfg.backend == "mlx_lm":
                    # mlx_lm.server already loaded one model; keep its id
                    upstream_body["model"] = requested
                response = await _proxy_completion(
                    cfg.upstream_url,
                    upstream_body,
                    stream=stream,
                    api_key=cfg.upstream_api_key,
                )
            return _finish_request(response, manager)
        except BaseException:
            manager.end_request()
            raise

    @app.post("/v1/responses")
    async def responses(request: Request) -> Any:
        """Native Responses surface, with the old translator as an explicit rollback."""
        body = await request.json()
        if cfg.responses_engine != "legacy":
            try:
                responses_service.capture(body, transport="http")
                normalized = responses_service.normalize(body)
                if normalized["stream"]:
                    return StreamingResponse(
                        responses_service.stream(normalized),
                        media_type="text/event-stream",
                        headers={
                            "Cache-Control": "no-cache",
                            "X-Accel-Buffering": "no",
                        },
                    )
                return await responses_service.create(normalized)
            except ResponsesError as error:
                return _responses_error(error)

        manager.begin_request()
        stream = bool(body.get("stream"))
        requested = body.get("model") or cfg.default_model
        aliases = {
            "laguna-xs-2.1",
            "synth/Laguna-XS-2.1",
            "synth/Laguna-XS-2.1-NVFP4",
        }
        if requested in aliases:
            requested = cfg.default_model

        try:
            status = await manager.ensure_ready()
            if status["state"] != "ready":
                return _finish_request(
                    _openai_error(503, status.get("lastError") or "model not ready"),
                    manager,
                )

            chat_body = build_chat_body_from_responses(body, default_model=requested)
            chat_body["model"] = requested
            tool_types = response_tool_types(body.get("tools"))

            if cfg.backend == "mock":
                messages = responses_input_to_messages(body)
                prompt = ""
                for message in reversed(messages):
                    if message.get("role") == "user":
                        prompt = str(message.get("content") or "")
                        break
                if stream:

                    async def mock_stream() -> AsyncIterator[bytes]:
                        for event in iter_response_sse_from_text(
                            model=requested, text="".join(_mock_stream(prompt))
                        ):
                            await _sleep(0.008)
                            yield sse_event(event)

                    response: Any = StreamingResponse(
                        mock_stream(), media_type="text/event-stream"
                    )
                else:
                    response = mock_response_payload(requested, prompt)
            else:
                response = await _proxy_responses(
                    cfg.upstream_url,
                    chat_body,
                    model=requested,
                    stream=stream,
                    api_key=cfg.upstream_api_key,
                    tool_types=tool_types,
                )
            return _finish_request(response, manager)
        except BaseException:
            manager.end_request()
            raise

    @app.websocket("/v1/responses")
    async def responses_websocket(websocket: WebSocket) -> None:
        if cfg.responses_engine == "legacy":
            await websocket.close(code=1008, reason="WebSocket requires native responses engine")
            return
        if cfg.api_key is not None and websocket.headers.get("authorization") != f"Bearer {cfg.api_key}":
            await websocket.close(code=1008, reason="missing or invalid bearer token")
            return
        await websocket.accept()
        connection_cache: dict[str, Any] = {}
        deadline = time.monotonic() + 3600
        try:
            while time.monotonic() < deadline:
                try:
                    body = await asyncio.wait_for(websocket.receive_json(), timeout=60.0)
                except asyncio.TimeoutError:
                    continue
                try:
                    responses_service.capture(body, transport="websocket")
                    async for event in responses_service.websocket_turn(body, connection_cache):
                        await websocket.send_json(event)
                except ResponsesError as error:
                    await websocket.send_json(
                        {
                            "type": "error",
                            "sequence_number": 0,
                            "error": error.payload()["error"],
                        }
                    )
        except WebSocketDisconnect:
            return
        finally:
            connection_cache.clear()
            if websocket.client_state.name == "CONNECTED":
                await websocket.close(code=1000)

    @app.post("/v1/responses/compact")
    async def compact_response(request: Request) -> Any:
        try:
            return await responses_service.compact(await request.json())
        except ResponsesError as error:
            return _responses_error(error)

    @app.post("/v1/responses/input_tokens")
    async def response_input_tokens(request: Request) -> Any:
        try:
            return await responses_service.input_tokens(await request.json())
        except ResponsesError as error:
            return _responses_error(error)

    @app.get("/v1/responses/{response_id}")
    async def get_response(response_id: str) -> Any:
        try:
            return await responses_service.get(response_id)
        except ResponsesError as error:
            return _responses_error(error)

    @app.delete("/v1/responses/{response_id}")
    async def delete_response(response_id: str) -> Any:
        try:
            return await responses_service.delete(response_id)
        except ResponsesError as error:
            return _responses_error(error)

    @app.post("/v1/responses/{response_id}/cancel")
    async def cancel_response(response_id: str) -> Any:
        try:
            return await responses_service.cancel(response_id)
        except ResponsesError as error:
            return _responses_error(error)

    @app.get("/v1/responses/{response_id}/input_items")
    async def response_input_items(
        response_id: str,
        limit: int = 20,
        order: str = "desc",
        after: str | None = None,
    ) -> Any:
        try:
            return await responses_service.input_items(
                response_id, limit=limit, order=order, after=after
            )
        except ResponsesError as error:
            return _responses_error(error)

    @app.get("/v1/synth/responses/telemetry")
    async def responses_telemetry() -> dict[str, Any]:
        return {
            "object": "list",
            "data": list(responses_service.telemetry),
        }

    @app.get("/v1/synth/status")
    async def synth_status() -> dict[str, Any]:
        return manager.status()

    @app.api_route(
        "/{full_path:path}",
        methods=["GET", "POST", "DELETE", "PUT", "PATCH", "OPTIONS"],
    )
    async def unknown(full_path: str) -> JSONResponse:
        return _openai_error(404, f"unknown route /{full_path}")

    return app


# Back-compat export used by __main__
create_app = build_app


async def _mock_completion(model: str, prompt: str, *, stream: bool) -> Any:
    completion_id = f"chatcmpl-{uuid.uuid4().hex.upper()}"
    created = int(time.time())
    chunks = _mock_stream(prompt)
    content = "".join(chunks)
    if not stream:
        return {
            "id": completion_id,
            "object": "chat.completion",
            "created": created,
            "model": model,
            "choices": [
                {
                    "index": 0,
                    "message": {"role": "assistant", "content": content},
                    "finish_reason": "stop",
                }
            ],
            "usage": {
                "prompt_tokens": max(1, len(prompt.split())),
                "completion_tokens": max(1, len(content.split())),
                "total_tokens": max(2, len(prompt.split()) + len(content.split())),
            },
        }

    async def event_stream() -> AsyncIterator[bytes]:
        yield _sse(
            {
                "id": completion_id,
                "object": "chat.completion.chunk",
                "created": created,
                "model": model,
                "choices": [{"index": 0, "delta": {"role": "assistant"}}],
            }
        )
        for piece in chunks:
            await _sleep(0.012)
            yield _sse(
                {
                    "id": completion_id,
                    "object": "chat.completion.chunk",
                    "created": created,
                    "model": model,
                    "choices": [{"index": 0, "delta": {"content": piece}}],
                }
            )
        yield _sse(
            {
                "id": completion_id,
                "object": "chat.completion.chunk",
                "created": created,
                "model": model,
                "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
            }
        )
        yield b"data: [DONE]\n\n"

    return StreamingResponse(event_stream(), media_type="text/event-stream")


async def _sleep(seconds: float) -> None:
    import asyncio

    await asyncio.sleep(seconds)


async def _proxy_completion(
    upstream: str,
    body: dict[str, Any],
    *,
    stream: bool,
    api_key: str | None = None,
) -> Any:
    url = f"{upstream}/v1/chat/completions"
    headers = {"Authorization": f"Bearer {api_key}"} if api_key else None
    if stream:

        async def event_stream() -> AsyncIterator[bytes]:
            async with httpx.AsyncClient(timeout=None) as client:
                async with client.stream(
                    "POST", url, json=body, headers=headers
                ) as response:
                    if response.status_code >= 400:
                        detail = await response.aread()
                        yield _sse(
                            {
                                "error": {
                                    "code": str(response.status_code),
                                    "type": "invalid_request_error",
                                    "message": detail.decode("utf-8", errors="replace")[
                                        :800
                                    ],
                                }
                            }
                        )
                        return
                    async for chunk in response.aiter_bytes():
                        yield chunk

        return StreamingResponse(event_stream(), media_type="text/event-stream")

    async with httpx.AsyncClient(timeout=300.0) as client:
        response = await client.post(url, json=body, headers=headers)
        if response.status_code >= 400:
            return _openai_error(
                502, response.text[:800] or f"upstream {response.status_code}"
            )
        return JSONResponse(response.json())


async def _proxy_responses(
    upstream: str,
    chat_body: dict[str, Any],
    *,
    model: str,
    stream: bool,
    api_key: str | None = None,
    tool_types: dict[str, str] | None = None,
) -> Any:
    url = f"{upstream}/v1/chat/completions"
    headers = {"Authorization": f"Bearer {api_key}"} if api_key else None
    if stream:
        chat_body = {**chat_body, "stream": True}

        async def event_stream() -> AsyncIterator[bytes]:
            async with httpx.AsyncClient(timeout=None) as client:
                async with client.stream(
                    "POST", url, json=chat_body, headers=headers
                ) as response:
                    if response.status_code >= 400:
                        detail = await response.aread()
                        yield sse_event(
                            {
                                "type": "error",
                                "error": {
                                    "code": str(response.status_code),
                                    "message": detail.decode("utf-8", errors="replace")[
                                        :800
                                    ],
                                },
                            }
                        )
                        return

                    async def byte_iter() -> AsyncIterator[bytes]:
                        async for chunk in response.aiter_bytes():
                            yield chunk

                    async for frame in translate_chat_sse_to_responses(
                        byte_iter(), model=model, tool_types=tool_types
                    ):
                        yield frame

        return StreamingResponse(event_stream(), media_type="text/event-stream")

    async with httpx.AsyncClient(timeout=300.0) as client:
        response = await client.post(
            url, json={**chat_body, "stream": False}, headers=headers
        )
        if response.status_code >= 400:
            return _openai_error(
                502, response.text[:800] or f"upstream {response.status_code}"
            )
        return wrap_chat_as_response(
            response.json(), model=model, tool_types=tool_types
        )
