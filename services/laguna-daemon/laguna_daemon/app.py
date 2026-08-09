from __future__ import annotations

import json
import time
import uuid
from contextlib import asynccontextmanager
from typing import Any, AsyncIterator, Callable

import httpx
from fastapi import FastAPI, Request
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import JSONResponse, StreamingResponse
from starlette.middleware.base import BaseHTTPMiddleware

from .config import LagunaConfig
from .manager import LagunaProcessManager
from .responses import (
    build_chat_body_from_responses,
    iter_response_sse_from_text,
    mock_response_payload,
    responses_input_to_messages,
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
        yield
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

    @asynccontextmanager
    async def lifespan(_: FastAPI):
        if cfg.backend == "mock":
            manager.state = "ready"
        elif cfg.auto_load:
            await manager.ensure_ready()
        yield
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

    def model_id() -> str:
        return cfg.default_model

    def models_payload() -> dict[str, Any]:
        mid = model_id()
        name = mid.split("/")[-1].replace("-", " ")
        return {
            "object": "list",
            "data": [
                {
                    "id": mid,
                    "object": "model",
                    "owned_by": mid.split("/")[0] if "/" in mid else "synth",
                    "root": mid,
                    "name": name,
                    "created": int(time.time()),
                    "context_length": cfg.context_length,
                    "details": {
                        "family": "poolside",
                        "format": "safetensors",
                        "context_length": cfg.context_length,
                    },
                }
            ],
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
        return {
            "status": "ok" if status["state"] == "ready" else status["state"],
            "modelsDirectory": str(cfg.models_dir),
            "defaultModel": cfg.default_model,
            "loadedModel": loaded,
            "memoryBytes": memory,
            "idleSeconds": int(time.time() - cfg.started_at),
            "idleUnloadAfterSeconds": cfg.idle_unload_after_seconds,
            "backend": cfg.backend,
            "publicUrl": cfg.public_url,
        }

    @app.get("/v1/models")
    async def list_models() -> dict[str, Any]:
        await manager.ensure_ready()
        return models_payload()

    @app.post("/v1/chat/completions")
    async def chat_completions(request: Request) -> Any:
        body = await request.json()
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

        status = await manager.ensure_ready()
        if status["state"] != "ready":
            return _openai_error(503, status.get("lastError") or "model not ready")

        if cfg.backend == "mock":
            prompt = ""
            for message in reversed(body.get("messages") or []):
                if message.get("role") == "user":
                    prompt = str(message.get("content") or "")
                    break
            return await _mock_completion(requested, prompt, stream=stream)

        # Upstream mlx_lm may not know our alias; pass resolved filesystem path when possible
        path = cfg.resolve_model_path(requested)
        upstream_body = dict(body)
        if path is not None and cfg.backend == "mlx_lm":
            # mlx_lm.server already loaded one model; keep its id
            upstream_body["model"] = requested
        return await _proxy_completion(
            cfg.upstream_url,
            upstream_body,
            stream=stream,
            api_key=cfg.upstream_api_key,
        )

    @app.post("/v1/responses")
    async def responses(request: Request) -> Any:
        """Codex wire_api=responses surface over chat/completions."""
        body = await request.json()
        stream = bool(body.get("stream"))
        requested = body.get("model") or cfg.default_model
        aliases = {
            "laguna-xs-2.1",
            "synth/Laguna-XS-2.1",
            "synth/Laguna-XS-2.1-NVFP4",
        }
        if requested in aliases:
            requested = cfg.default_model

        status = await manager.ensure_ready()
        if status["state"] != "ready":
            return _openai_error(503, status.get("lastError") or "model not ready")

        chat_body = build_chat_body_from_responses(body, default_model=requested)
        chat_body["model"] = requested

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

                return StreamingResponse(
                    mock_stream(), media_type="text/event-stream"
                )
            return mock_response_payload(requested, prompt)

        return await _proxy_responses(
            cfg.upstream_url,
            chat_body,
            model=requested,
            stream=stream,
            api_key=cfg.upstream_api_key,
        )

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
                        byte_iter(), model=model
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
        return wrap_chat_as_response(response.json(), model=model)
