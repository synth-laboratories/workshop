from __future__ import annotations

import asyncio
import json
import os
import signal
import time
from contextlib import asynccontextmanager
from functools import partial
from typing import Any, AsyncIterator, Callable

import anyio
from fastapi import FastAPI, Request, WebSocket, WebSocketDisconnect
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import JSONResponse, PlainTextResponse, StreamingResponse
from starlette.middleware.base import BaseHTTPMiddleware

from .chat_api import ChatService
from .config import LagunaConfig
from .responses_api import ResponsesService
from .responses_api.errors import ResponsesError
from .responses_api.policies import PolicyError
from .settings import SettingsStore
from .synth_control import SynthControl, register_control_routes


def _desktop_parent_pid() -> int | None:
    raw = os.environ.get("SYNTH_DESKTOP_PARENT_PID", "").strip()
    try:
        pid = int(raw)
    except ValueError:
        return None
    return pid if pid > 1 else None


def _process_is_alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
        return True
    except PermissionError:
        return True
    except ProcessLookupError:
        return False


async def _watch_desktop_parent(parent_pid: int) -> None:
    while True:
        await asyncio.sleep(1.0)
        if not _process_is_alive(parent_pid) or os.getppid() == 1:
            os.kill(os.getpid(), signal.SIGTERM)
            return


class DisconnectAwareStreamingResponse(StreamingResponse):
    """Cancel the body iterator as soon as ASGI reports a lost HTTP client.

    Starlette's ASGI 2.4 path waits for the next socket write to raise. Native
    MLX can spend tens of seconds in prefill without yielding a body chunk, so
    an abandoned generation would keep the single GPU slot during that gap.
    """

    async def __call__(self, scope: Any, receive: Any, send: Any) -> None:
        async with anyio.create_task_group() as task_group:
            async def run_and_cancel(call: Callable[[], Any]) -> None:
                await call()
                task_group.cancel_scope.cancel()

            task_group.start_soon(run_and_cancel, partial(self.stream_response, send))
            await run_and_cancel(partial(self.listen_for_disconnect, receive))
        if self.background is not None:
            await self.background()


def _policy_metric_lines(policies: dict[str, Any]) -> list[str]:
    """Decode speed labelled by policy; unmeasured policies emit nothing.

    A policy without enough samples is absent rather than zero: a zero here
    would read as "this policy is infinitely slow" on any dashboard.
    """
    rows = policies.get("policies") or {}
    lines = [
        "# HELP laguna_policy_decode_tokens_per_second Decode speed at the p10 latency.",
        "# TYPE laguna_policy_decode_tokens_per_second gauge",
    ]
    for model_id, row in sorted(rows.items()):
        rate = row.get("tokensPerSecondP10")
        if rate is None:
            continue
        label = str(model_id).replace("\\", "\\\\").replace('"', '\\"')
        lines.append(f'laguna_policy_decode_tokens_per_second{{policy="{label}"}} {rate}')
    return lines


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


def build_app(config: LagunaConfig | None = None) -> FastAPI:
    """Build app with Poolside-compatible routes registered before the catch-all."""
    cfg = config or LagunaConfig.from_env()
    # A settings file with an unknown key must fail startup loudly rather
    # than silently leaving the real default in place.
    settings_store = SettingsStore.load(cfg)
    responses_service = ResponsesService(cfg, settings=settings_store)
    chat_service = ChatService(cfg, responses_service)

    @asynccontextmanager
    async def lifespan(_: FastAPI):
        await responses_service.start()
        # One runtime, one idle watchdog. Weights load lazily on the first
        # request and are released back without terminating the daemon.
        native_watchdog = asyncio.create_task(
            responses_service.watch_idle(), name="laguna-native-idle-unload"
        )
        pressure_watchdog = asyncio.create_task(
            responses_service.watch_memory_pressure(),
            name="laguna-native-memory-pressure",
        )
        parent_pid = _desktop_parent_pid()
        parent_watchdog = (
            asyncio.create_task(
                _watch_desktop_parent(parent_pid), name="laguna-desktop-parent"
            )
            if parent_pid is not None
            else None
        )
        try:
            yield
        finally:
            watchdogs = [native_watchdog, pressure_watchdog]
            if parent_watchdog is not None:
                watchdogs.append(parent_watchdog)
            for watchdog in watchdogs:
                watchdog.cancel()
            await asyncio.gather(*watchdogs, return_exceptions=True)
            await responses_service.close()

    app = FastAPI(
        title="Synth Laguna Sidecar",
        version="0.1.0",
        lifespan=lifespan,
        docs_url=None,
        redoc_url=None,
    )
    synth_control = SynthControl(cfg, responses_service, settings=settings_store)
    app.state.config = cfg
    app.state.responses_service = responses_service
    app.state.chat_service = chat_service
    app.state.synth_control = synth_control
    app.add_middleware(
        CORSMiddleware,
        allow_origins=["*"],
        allow_methods=["*"],
        allow_headers=["*"],
    )
    app.add_middleware(BearerAuthMiddleware, api_key=cfg.api_key)

    def model_id() -> str:
        return cfg.default_model

    def describe_model(mid: str) -> tuple[dict[str, Any], dict[str, Any]]:
        name = mid.split("/")[-1].replace("-", " ")
        runtime_description = "Native local MLX model served by Synth Laguna."
        reasoning_levels = [
            {"effort": "none", "description": "Answer without a reasoning phase."},
            {"effort": "high", "description": "Use Laguna's reasoning mode."},
        ]
        item = {
            "id": mid,
            "object": "model",
            "owned_by": mid.split("/")[0] if "/" in mid else "synth",
            "root": mid,
            "name": name,
            "display_name": name,
            "description": runtime_description,
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
            "description": runtime_description,
            # Derived from runtime settings so the advertised default always
            # matches what an absent reasoning field actually does.
            "default_reasoning_level": settings_store.sampling.reasoning_effort,
            "supported_reasoning_levels": reasoning_levels,
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
            "supports_image_detail_original": False,
            "supports_search_tool": False,
            "multi_agent_version": None,
            "tool_mode": None,
            "use_responses_lite": False,
            "prefer_websockets": True,
            "base_instructions": "You are Codex, a coding agent running on local Synth Laguna.",
        }
        return item, codex_item

    def models_payload() -> dict[str, Any]:
        # One entry per selectable policy. The base weights and each registered
        # adapter are peers here: a client picks one by asking for its model id,
        # which is what keeps the pin in the request instead of in daemon state.
        described = [
            describe_model(policy.model_id) for policy in responses_service.policies.list()
        ]
        return {
            "object": "list",
            "data": [item for item, _ in described],
            # Codex app-server's model manager consumes its native envelope,
            # while OpenAI SDKs consume data. Supplying both is additive.
            "models": [codex for _, codex in described],
        }

    @app.get("/health")
    async def health() -> dict[str, Any]:
        """Authoritative, low-frequency residency.

        Every field is sourced from the in-process MLX backend. There is no
        second runtime whose process state could disagree with this.
        """
        residency = responses_service.residency()
        resident = bool(residency and residency["loaded"])
        loaded = cfg.default_model if resident else None
        model_path = cfg.resolve_model_path(cfg.default_model)
        model_installed = cfg.backend in {"mock", "external"} or model_path is not None
        memory = 0
        if resident:
            if model_path is not None:
                memory = sum(
                    file.stat().st_size
                    for file in model_path.rglob("*")
                    if file.is_file()
                )
        return {
            # Weights load lazily on first use, but a daemon cannot serve when
            # its configured local model does not exist.  Report that state
            # explicitly so Desktop never labels missing weights as "ready".
            "status": "ok" if model_installed else "not_installed",
            "responsesApi": True,
            "chatCompletionsApi": True,
            "modelsDirectory": str(cfg.models_dir),
            "defaultModel": cfg.default_model,
            "loadedModel": loaded,
            "memoryBytes": memory,
            "idleSeconds": residency["idle_seconds"] if residency else None,
            "idleUnloadAfterSeconds": responses_service.idle_unload_after_seconds,
            "lastUsedAt": residency["last_used_at"] if residency else None,
            "freeAt": residency["free_at"] if residency else None,
            "backend": cfg.backend,
            "publicUrl": cfg.public_url,
            "responses": await responses_service.health(),
        }

    @app.get("/v1/models")
    async def list_models() -> dict[str, Any]:
        return models_payload()

    @app.get("/v1/synth/policies")
    async def list_policies() -> dict[str, Any]:
        return {
            "default_model": responses_service.policies.default_model,
            "policies": [policy.json() for policy in responses_service.policies.list()],
        }

    @app.post("/v1/synth/policies")
    async def register_policy(request: Request) -> Any:
        """Register an adapter under a model id clients can ask for.

        Workshop owns adapter identity in its catalog; the daemon only needs a
        name, a validated `mlx-lora.v1` directory, and the digest to report
        back so the two sides can be checked against each other.
        """
        try:
            body = await request.json()
        except (ValueError, TypeError):
            body = {}
        if not isinstance(body, dict):
            body = {}
        try:
            policy = responses_service.policies.register(
                str(body.get("model_id") or ""),
                str(body.get("adapter_path") or ""),
                digest=body.get("digest"),
                title=body.get("title"),
            )
        except PolicyError as error:
            return JSONResponse(
                status_code=400,
                content={
                    "error": {
                        "type": "invalid_request_error",
                        "code": "invalid_policy",
                        "message": str(error),
                        "param": error.field,
                    }
                },
            )
        return {"policy": policy.json()}

    @app.delete("/v1/synth/policies/{model_id:path}")
    async def remove_policy(model_id: str) -> Any:
        try:
            removed = responses_service.policies.remove(model_id)
        except PolicyError as error:
            return JSONResponse(
                status_code=400,
                content={
                    "error": {
                        "type": "invalid_request_error",
                        "code": "invalid_policy",
                        "message": str(error),
                        "param": error.field,
                    }
                },
            )
        if not removed:
            return JSONResponse(
                status_code=404,
                content={
                    "error": {
                        "type": "invalid_request_error",
                        "code": "policy_not_found",
                        "message": f"No policy registered as {model_id!r}.",
                    }
                },
            )
        return {"removed": model_id}

    @app.post("/v1/chat/completions")
    async def chat_completions(request: Request) -> Any:
        """Chat Completions as a peer surface over the neutral turn core.

        This never constructs a Responses object and never makes an internal
        HTTP request. It compiles onto the same core and runs on the same
        `TurnRunner`, so residency, the single admission slot, cancellation,
        and token accounting are shared rather than duplicated.
        """
        body = await request.json()
        try:
            if isinstance(body, dict) and bool(body.get("stream")):
                # Awaited so validation and compilation failures become real
                # HTTP errors instead of frames inside a 200 response.
                frames = await chat_service.open_stream(
                    body, disconnected=request.is_disconnected
                )
                return DisconnectAwareStreamingResponse(
                    frames,
                    media_type="text/event-stream",
                    headers={
                        "Cache-Control": "no-cache",
                        "X-Accel-Buffering": "no",
                    },
                )
            return await chat_service.create(body)
        except ResponsesError as error:
            return _responses_error(error)

    @app.post("/v1/responses")
    async def responses(request: Request) -> Any:
        """Native Responses. Items are canonical; nothing is lowered to Chat."""
        body = await request.json()
        try:
            responses_service.capture(body, transport="http")
            normalized = responses_service.normalize(body)
            if normalized["stream"]:
                return DisconnectAwareStreamingResponse(
                    responses_service.stream(
                        normalized, disconnected=request.is_disconnected
                    ),
                    media_type="text/event-stream",
                    headers={
                        "Cache-Control": "no-cache",
                        "X-Accel-Buffering": "no",
                    },
                )
            return await responses_service.create(normalized)
        except ResponsesError as error:
            return _responses_error(error)

    @app.websocket("/v1/responses")
    async def responses_websocket(websocket: WebSocket) -> None:
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

    @app.get("/v1/synth/inference")
    async def inference_snapshot() -> dict[str, Any]:
        """Live, redacted inference activity for the Desktop monitor."""
        return responses_service.inference_snapshot()

    @app.get("/v1/synth/inference/stream")
    async def inference_stream(request: Request) -> Any:
        """The same snapshot, pushed while a client is watching.

        The interval is deliberately coarse and the work per tick is a pure
        read of counters the backend already keeps: sampling must never add
        cost to the generation loop it is reporting on.
        """

        async def events() -> AsyncIterator[bytes]:
            # Bounded so an undetected client loss cannot leave a poller
            # running for the life of the daemon; a watching client reconnects.
            deadline = time.monotonic() + 3600
            while time.monotonic() < deadline:
                payload = json.dumps(
                    responses_service.inference_snapshot(),
                    ensure_ascii=False,
                    separators=(",", ":"),
                )
                yield f"event: inference\ndata: {payload}\n\n".encode()
                # Four updates per second keeps short prefill/decode phases
                # visible without touching the generation thread itself.
                await asyncio.sleep(0.25)
                if await request.is_disconnected():
                    return

        # A plain streaming response is correct here: this endpoint holds no
        # generation slot, so there is nothing for the disconnect-aware variant
        # to rescue, and its listener would outlive the reader.
        return StreamingResponse(
            events(),
            media_type="text/event-stream",
            headers={"Cache-Control": "no-cache", "X-Accel-Buffering": "no"},
        )

    @app.post("/v1/synth/model/unload")
    async def unload_model() -> Any:
        """Release weights on request, honoring the eviction guard."""
        if await responses_service.unload_now():
            return {"unloaded": True, "model": cfg.default_model}
        return JSONResponse(
            status_code=409,
            content={
                "error": {
                    "type": "invalid_request_error",
                    "code": "generation_in_flight",
                    "message": "A generation is using the model; retry once it completes.",
                    "param": None,
                }
            },
        )

    @app.get("/metrics")
    async def metrics() -> Any:
        """Prometheus exposition of the same snapshot.

        Deliberately label-free: per-request labels would reintroduce
        identifiers that the redaction rules keep out of telemetry.
        """
        snapshot = responses_service.inference_snapshot()
        rolling = snapshot["rolling"]
        lines = [
            "# HELP laguna_model_resident Whether model weights are loaded.",
            "# TYPE laguna_model_resident gauge",
            f"laguna_model_resident {int(snapshot['resident'])}",
            "# HELP laguna_resident_bytes Resident model size in bytes.",
            "# TYPE laguna_resident_bytes gauge",
            f"laguna_resident_bytes {snapshot['residentBytes']}",
            "# HELP laguna_queue_depth Generations admitted and not yet complete.",
            "# TYPE laguna_queue_depth gauge",
            f"laguna_queue_depth {snapshot['queueDepth']}",
            "# HELP laguna_queue_capacity Maximum admitted generations.",
            "# TYPE laguna_queue_capacity gauge",
            f"laguna_queue_capacity {snapshot['queueCapacity']}",
            "# HELP laguna_requests_total Requests by outcome since daemon start.",
            "# TYPE laguna_requests_total counter",
            f'laguna_requests_total{{outcome="completed"}} {rolling["requestsCompleted"]}',
            f'laguna_requests_total{{outcome="failed"}} {rolling["requestsFailed"]}',
            f'laguna_requests_total{{outcome="cancelled"}} {rolling["requestsCancelled"]}',
            "# HELP laguna_tokens_total Tokens by kind since daemon start.",
            "# TYPE laguna_tokens_total counter",
            f'laguna_tokens_total{{kind="input"}} {rolling["inputTokens"]}',
            f'laguna_tokens_total{{kind="output"}} {rolling["outputTokens"]}',
            *_policy_metric_lines(snapshot.get("policies") or {}),
            f'laguna_tokens_total{{kind="cached"}} {rolling["cachedTokens"]}',
        ]
        # An unmeasured percentile is omitted rather than exported as zero.
        for name, key in (
            ("laguna_ttft_ms_p50", "ttftP50Ms"),
            ("laguna_ttft_ms_p95", "ttftP95Ms"),
            ("laguna_decode_tps_p50", "decodeTpsP50"),
            ("laguna_decode_tps_p95", "decodeTpsP95"),
            ("laguna_latency_ms_p50", "latencyP50Ms"),
            ("laguna_latency_ms_p95", "latencyP95Ms"),
        ):
            if rolling[key] is not None:
                lines.append(f"# TYPE {name} gauge")
                lines.append(f"{name} {rolling[key]}")
        histogram = responses_service.prefill_histogram()
        lines.append(
            "# HELP laguna_prefill_requests_total Completed generations by "
            "prompt-size bucket over the rolling window."
        )
        lines.append("# TYPE laguna_prefill_requests_total counter")
        for bucket, entry in histogram.items():
            lines.append(
                f'laguna_prefill_requests_total{{bucket="{bucket}"}} {entry["count"]}'
            )
        # A share that was never measured is omitted, not exported as zero.
        share_lines = [
            f'laguna_prefill_cached_token_share{{bucket="{bucket}"}} '
            f'{entry["cached_token_share"]}'
            for bucket, entry in histogram.items()
            if entry["cached_token_share"] is not None
        ]
        if share_lines:
            lines.append(
                "# HELP laguna_prefill_cached_token_share Cached share of "
                "prompt tokens per bucket over the rolling window."
            )
            lines.append("# TYPE laguna_prefill_cached_token_share gauge")
            lines.extend(share_lines)
        return PlainTextResponse("\n".join(lines) + "\n", media_type="text/plain")

    # Typed /v1/synth control surface (status, capabilities, lifecycle,
    # metrics mirror, events, openapi). The legacy /v1/synth/inference*,
    # /v1/synth/model/unload, and /metrics routes above remain exactly as
    # they are; these are additive and share the same bearer auth.
    register_control_routes(app, synth_control)

    @app.api_route(
        "/{full_path:path}",
        methods=["GET", "POST", "DELETE", "PUT", "PATCH", "OPTIONS"],
    )
    async def unknown(full_path: str) -> JSONResponse:
        return _openai_error(404, f"unknown route /{full_path}")

    return app


# Back-compat export used by __main__
create_app = build_app
