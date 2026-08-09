from __future__ import annotations

import asyncio
import json
import time
import uuid
from contextlib import asynccontextmanager
from typing import AsyncIterator

import httpx
from fastapi import FastAPI, HTTPException
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import JSONResponse, StreamingResponse

from .contracts import ChatCompletionRequest, LoadModelRequest
from .state import InferenceState


def _sse(payload: dict, *, event: str | None = None) -> bytes:
    prefix = f"event: {event}\n" if event else ""
    return f"{prefix}data: {json.dumps(payload, separators=(',', ':'))}\n\n".encode()


def _last_user_text(request: ChatCompletionRequest) -> str:
    for message in reversed(request.messages):
        if message.role != "user":
            continue
        if isinstance(message.content, str):
            return message.content
        return " ".join(
            str(part.get("text", ""))
            for part in message.content
            if isinstance(part, dict) and part.get("type") in {"text", "input_text"}
        ).strip()
    return ""


def _mock_answer(prompt: str) -> str:
    lowered = prompt.lower()
    if "data" in lowered or "csv" in lowered or "dataset" in lowered:
        return (
            "I’m running as the local Laguna XS 2.1 preview. For a data task, I would first inspect "
            "the available files, infer the schema, summarize missingness and distributions, and then "
            "produce a reproducible analysis with tables and charts. Connect a local tool harness in "
            "the next pass to let me execute that plan against your workspace."
        )
    if "code" in lowered or "repo" in lowered:
        return (
            "I’m running locally through the Laguna-compatible inference boundary. The next useful "
            "step is to attach the repository tool loop; this first pass already preserves the same "
            "session, event, cancellation, model, and adapter identities that the agent loop will use."
        )
    return (
        "This is the local Laguna XS 2.1 path running in first-pass mode. Streaming, cancellation, "
        "model lifecycle, session persistence, and the LoRA adapter slot are wired. Install the MLX "
        "extra on an Apple Silicon Mac and load the model to replace this deterministic preview."
    )


async def _mock_stream(request: ChatCompletionRequest, state: InferenceState) -> AsyncIterator[bytes]:
    completion_id = f"chatcmpl-local-{uuid.uuid4().hex}"
    created = int(time.time())
    model = request.model or state.model
    answer = _mock_answer(_last_user_text(request))
    yield _sse(
        {
            "id": completion_id,
            "object": "chat.completion.chunk",
            "created": created,
            "model": model,
            "choices": [{"index": 0, "delta": {"role": "assistant"}, "finish_reason": None}],
        }
    )
    chunks = answer.split(" ")
    for index, word in enumerate(chunks):
        await asyncio.sleep(0.018)
        text = word if index == 0 else f" {word}"
        yield _sse(
            {
                "id": completion_id,
                "object": "chat.completion.chunk",
                "created": created,
                "model": model,
                "choices": [{"index": 0, "delta": {"content": text}, "finish_reason": None}],
            }
        )
    yield _sse(
        {
            "id": completion_id,
            "object": "chat.completion.chunk",
            "created": created,
            "model": model,
            "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
            "usage": {
                "prompt_tokens": max(1, len(_last_user_text(request)) // 4),
                "completion_tokens": max(1, len(answer) // 4),
                "total_tokens": max(2, (len(_last_user_text(request)) + len(answer)) // 4),
            },
        }
    )
    yield b"data: [DONE]\n\n"


def create_app(state: InferenceState | None = None) -> FastAPI:
    inference = state or InferenceState()

    @asynccontextmanager
    async def lifespan(_: FastAPI):
        await inference.startup()
        yield
        await inference.shutdown()

    app = FastAPI(title="Synth Local Inference", version="0.1.0", lifespan=lifespan)
    app.state.inference = inference
    app.add_middleware(
        CORSMiddleware,
        allow_origins=["null"],
        allow_origin_regex=r"^https?://(localhost|127\.0\.0\.1)(:\d+)?$",
        allow_methods=["GET", "POST", "OPTIONS"],
        allow_headers=["*"],
    )

    @app.get("/health")
    async def health() -> dict:
        status = await inference.status()
        return {"ok": True, "model": status.model_dump(mode="json")}

    @app.get("/v1/models/status")
    async def model_status() -> dict:
        return (await inference.status()).model_dump(mode="json")

    @app.post("/v1/models/load", status_code=202)
    async def load_model(request: LoadModelRequest) -> dict:
        return (
            await inference.load(
                model=request.model,
                adapter=request.adapter,
                draft_model=request.draft_model,
            )
        ).model_dump(mode="json")

    @app.post("/v1/models/unload")
    async def unload_model() -> dict:
        return (await inference.unload()).model_dump(mode="json")

    @app.get("/v1/metrics")
    async def metrics() -> dict:
        return await inference.proxy_metrics()

    @app.post("/v1/chat/completions")
    async def chat_completions(request: ChatCompletionRequest):
        status = await inference.status()
        if status.state != "ready":
            raise HTTPException(status_code=503, detail={"message": "Laguna is not ready", "status": status.model_dump()})
        if request.adapter != status.adapter:
            raise HTTPException(
                status_code=409,
                detail={
                    "message": "Requested adapter is not the active Laguna adapter",
                    "requestedAdapter": request.adapter,
                    "activeAdapter": status.adapter,
                },
            )

        if status.active_mode == "mock":
            if request.stream:
                return StreamingResponse(_mock_stream(request, inference), media_type="text/event-stream")
            answer = _mock_answer(_last_user_text(request))
            return JSONResponse(
                {
                    "id": f"chatcmpl-local-{uuid.uuid4().hex}",
                    "object": "chat.completion",
                    "created": int(time.time()),
                    "model": request.model or inference.model,
                    "choices": [{"index": 0, "message": {"role": "assistant", "content": answer}, "finish_reason": "stop"}],
                }
            )

        payload = request.model_dump(exclude_none=True)
        payload.pop("adapter", None)
        payload["model"] = inference.model
        client = httpx.AsyncClient(timeout=None)
        try:
            upstream_request = client.build_request(
                "POST",
                f"{inference.upstream_url}/v1/chat/completions",
                json=payload,
            )
            upstream = await client.send(upstream_request, stream=True)
        except httpx.HTTPError as exc:
            await client.aclose()
            raise HTTPException(status_code=502, detail=f"MLX-VLM request failed: {exc}") from exc

        if not upstream.is_success:
            body = await upstream.aread()
            await upstream.aclose()
            await client.aclose()
            raise HTTPException(status_code=upstream.status_code, detail=body.decode(errors="replace"))

        async def proxy() -> AsyncIterator[bytes]:
            try:
                async for chunk in upstream.aiter_raw():
                    yield chunk
            finally:
                await upstream.aclose()
                await client.aclose()

        return StreamingResponse(proxy(), media_type=upstream.headers.get("content-type", "text/event-stream"))

    return app


app = create_app()
