from __future__ import annotations

"""Versioned sidecar control surface under /v1/synth.

This is the typed control plane for the daemon: residency lifecycle
(download, load, unload), a canonical runtime state machine, a JSON mirror of
the rolling telemetry, and an SSE feed of state transitions. The generation
surfaces (/v1/responses, /v1/chat/completions) and the legacy telemetry
endpoints are untouched peers; everything here reads the same backend facts
they do and never invents a number the backend did not measure.
"""

import asyncio
import hashlib
import json
import shutil
import threading
import time
from collections import OrderedDict, deque
from dataclasses import dataclass
from datetime import datetime, timezone
from importlib.metadata import PackageNotFoundError, version
from pathlib import Path
from typing import Any, AsyncIterator, Callable, Protocol

from fastapi import FastAPI, Request
from fastapi.responses import JSONResponse, StreamingResponse

from .config import LagunaConfig
from .responses_api.errors import ResponsesError
from .responses_api.ids import new_id
from .settings import SETTINGS_SCHEMA_VERSION, SettingsError, SettingsStore
from .responses_api.backends.mlx import (
    _MIN_SYSTEM_MEMORY_BYTES,
    _available_memory_bytes,
    _physical_memory_bytes,
    _required_available_memory_bytes,
    _required_system_memory_bytes,
)


SCHEMA_VERSION = "1.0"

#: Matches the Desktop download preflight: a fresh Laguna checkpoint is ~20 GiB
#: and partial shards are useless, so refuse to start without real headroom.
REQUIRED_FREE_DISK_BYTES = 24 * 1024**3

#: The one canonical state enum. Every /v1/synth surface reports exactly one of
#: these; there is no second vocabulary and no contradictory combination.
CANONICAL_STATES = frozenset(
    {
        "starting",
        "checking_memory",
        "downloading",
        "downloaded",
        "loading",
        "resident_idle",
        "queued",
        "prefill",
        "reasoning",
        "decoding",
        "unloading",
        "unloaded",
        "blocked_memory",
        "error",
    }
)

# Generation phases as the backend tracks them, mapped onto the canonical
# enum. "reasoning" is deliberately absent: whether decode is inside a
# thinking span is not a fact the backend records per-token, and reporting it
# would be an invention rather than a measurement.
_PHASE_TO_STATE = {
    "queued": "queued",
    "loading": "loading",
    "compiling": "prefill",
    "prefill": "prefill",
    "decode": "decoding",
    "complete": "resident_idle",
}

_REASONING_CONTRACT = {
    "supported": ["none", "high"],
    "default": "high",
    "legacy_aliases": {"max": "high"},
}

_OPENAPI_PATH = Path(__file__).resolve().parents[1] / "openapi" / "synth-sidecar.yaml"


def _utc_iso(seconds: float | None = None) -> str:
    moment = (
        datetime.now(timezone.utc)
        if seconds is None
        else datetime.fromtimestamp(seconds, tz=timezone.utc)
    )
    return moment.isoformat(timespec="milliseconds").replace("+00:00", "Z")


def _redact_id(generation_id: str) -> str:
    # Same redaction rule as the inference snapshot: control telemetry must
    # never carry a complete stable id that correlates back to stored content.
    return "sha256:" + hashlib.sha256(generation_id.encode()).hexdigest()[:12]


def _sidecar_version() -> str:
    try:
        return version("synth-laguna-daemon")
    except PackageNotFoundError:
        return "0.1.0"


class ControlError(Exception):
    """Typed, fail-closed error for every /v1/synth control endpoint."""

    def __init__(
        self,
        code: str,
        message: str,
        status_code: int,
        *,
        retryable: bool = False,
        details: dict[str, Any] | None = None,
    ) -> None:
        super().__init__(message)
        self.code = code
        self.message = message
        self.status_code = status_code
        self.retryable = retryable
        self.details = details or {}

    def response(self) -> JSONResponse:
        return JSONResponse(
            status_code=self.status_code,
            content={
                "error": {
                    "code": self.code,
                    "message": self.message,
                    "retryable": self.retryable,
                    "details": self.details,
                },
                "request_id": new_id("req"),
            },
        )


class Downloader(Protocol):
    """Fetch one model's weights into a destination directory.

    Runs on a plain worker thread. `progress(bytes_done, bytes_total)` may be
    called with None for either value — unmeasured progress stays null, it is
    never fabricated.
    """

    def download(
        self,
        model: str,
        destination: Path,
        progress: Callable[[int | None, int | None], None],
    ) -> None: ...


class HuggingFaceDownloader:
    """Real snapshot download through huggingface_hub.

    Exercised only by the env-gated live test; the deterministic suite always
    injects a fake so it never touches the network.
    """

    def __init__(self, revision: str | None = None) -> None:
        self.revision = revision

    def download(
        self,
        model: str,
        destination: Path,
        progress: Callable[[int | None, int | None], None],
    ) -> None:
        from huggingface_hub import snapshot_download

        destination.mkdir(parents=True, exist_ok=True)
        # snapshot_download exposes no byte-level callback through this API,
        # so bytes_done/bytes_total remain null rather than being estimated.
        snapshot_download(
            repo_id=model, revision=self.revision, local_dir=str(destination)
        )


@dataclass(slots=True)
class DownloadJob:
    job_id: str
    model: str
    state: str  # queued | downloading | downloaded | failed
    bytes_done: int | None
    bytes_total: int | None
    error: str | None
    created_at: str
    updated_at: str

    def json(self) -> dict[str, Any]:
        return {
            "job_id": self.job_id,
            "operation_id": self.job_id,
            "model": self.model,
            "state": self.state,
            "bytes_done": self.bytes_done,
            "bytes_total": self.bytes_total,
            "error": self.error,
            "created_at": self.created_at,
            "updated_at": self.updated_at,
        }


class EventBroker:
    """In-process fan-out of control events with a bounded replay buffer.

    Publish happens only on the event loop; download worker threads hand their
    transitions to `SynthControl`'s pending queue instead of touching asyncio.
    """

    def __init__(self, history: int = 64) -> None:
        self._history: deque[dict[str, Any]] = deque(maxlen=history)
        self._subscribers: set[asyncio.Queue[dict[str, Any]]] = set()

    def publish(self, event: dict[str, Any]) -> None:
        self._history.append(event)
        for queue in list(self._subscribers):
            try:
                queue.put_nowait(event)
            except asyncio.QueueFull:
                # A stalled subscriber loses its oldest event, never the feed.
                try:
                    queue.get_nowait()
                except asyncio.QueueEmpty:
                    pass
                queue.put_nowait(event)

    def subscribe(self, *, replay: bool = True) -> asyncio.Queue[dict[str, Any]]:
        queue: asyncio.Queue[dict[str, Any]] = asyncio.Queue(maxsize=256)
        if replay:
            for event in self._history:
                queue.put_nowait(event)
        self._subscribers.add(queue)
        return queue

    def unsubscribe(self, queue: asyncio.Queue[dict[str, Any]]) -> None:
        self._subscribers.discard(queue)

    def history(self) -> list[dict[str, Any]]:
        return list(self._history)


class SynthControl:
    """State machine and operations behind the /v1/synth control endpoints."""

    def __init__(
        self,
        config: LagunaConfig,
        service: Any,
        *,
        downloader: Downloader | None = None,
        system_memory_bytes: int | None = None,
        available_memory_bytes: int | None = None,
        settings: SettingsStore | None = None,
    ) -> None:
        self.config = config
        self.service = service
        self.settings = settings or getattr(service, "settings", None) or SettingsStore.load(config)
        self.downloader: Downloader = downloader or HuggingFaceDownloader(
            revision=config.revision
        )
        self.broker = EventBroker()
        self.system_memory_bytes = (
            system_memory_bytes
            if system_memory_bytes is not None
            else _physical_memory_bytes()
        )
        self.available_memory_bytes = available_memory_bytes
        self._state = "starting"
        self._state_since = time.time()
        self._load_lock = asyncio.Lock()
        self._unloading = False
        self._last_error: dict[str, Any] | None = None
        # True after a download completes and until the next successful load;
        # distinguishes "downloaded" from a plain "unloaded".
        self._downloaded_marker = False
        self._jobs: OrderedDict[str, DownloadJob] = OrderedDict()
        self._jobs_lock = threading.Lock()
        self._max_jobs = 32
        # Download threads append transitions here; the event loop drains them
        # on the next control-plane touch. Threads never call into asyncio.
        self._pending_events: deque[dict[str, Any]] = deque()
        self._openapi_cache: dict[str, Any] | None = None

    # -- model identity -------------------------------------------------------

    def resolve_model(self, model: str) -> str:
        """This daemon serves exactly one configured model (plus aliases)."""
        candidate = model.strip()
        aliases = {
            self.config.default_model,
            "laguna-xs-2.1",
            "synth/Laguna-XS-2.1",
            "synth/Laguna-XS-2.1-NVFP4",
        }
        if candidate not in aliases:
            raise ControlError(
                "invalid_model",
                f"This sidecar serves {self.config.default_model!r}; "
                f"{candidate!r} is not a model it can manage.",
                400,
                details={"model": candidate, "served_model": self.config.default_model},
            )
        return self.config.default_model

    def _model_path(self) -> Path | None:
        return self.config.resolve_model_path(self.config.default_model)

    # -- memory facts ----------------------------------------------------------

    def _required_bytes(self) -> int:
        path = self._model_path()
        if path is None:
            return _MIN_SYSTEM_MEMORY_BYTES
        return _required_system_memory_bytes(path)

    def _free_memory_bytes(self) -> int | None:
        available = (
            self.available_memory_bytes
            if self.available_memory_bytes is not None
            else _available_memory_bytes()
        )
        if available is None:
            return self.system_memory_bytes
        if self.system_memory_bytes is None:
            return available
        return min(available, self.system_memory_bytes)

    def _admission_allowed(self) -> bool:
        capacity_ok = (
            self.system_memory_bytes is None
            or self.system_memory_bytes >= self._required_bytes()
        )
        available = self._free_memory_bytes()
        path = self._model_path()
        available_ok = (
            path is None
            or available is None
            or available >= _required_available_memory_bytes(path)
        )
        return capacity_ok and available_ok

    # -- canonical state -------------------------------------------------------

    def _diagnostics(self) -> dict[str, Any]:
        diagnostics = getattr(self.service.backend, "diagnostics", None)
        return diagnostics() if callable(diagnostics) else {}

    def _active_generation(self) -> Any | None:
        active = getattr(self.service.backend, "active_generation", None)
        return active() if callable(active) else None

    def _compute_state(self) -> str:
        with self._jobs_lock:
            downloading = any(
                job.model == self.config.default_model
                and job.state in {"queued", "downloading"}
                for job in self._jobs.values()
            )
        if downloading:
            return "downloading"
        diagnostics = self._diagnostics()
        if diagnostics.get("loading"):
            return "loading"
        if self._unloading:
            return "unloading"
        if diagnostics.get("loaded"):
            active = self._active_generation()
            if active is None:
                return "resident_idle"
            return _PHASE_TO_STATE.get(active.phase, "resident_idle")
        if self._last_error is not None:
            return "error"
        if self._model_path() is not None:
            if not self._admission_allowed():
                return "blocked_memory"
            if self._downloaded_marker:
                return "downloaded"
        return "unloaded"

    def _set_state(self, state: str, operation_id: str | None) -> None:
        if state == self._state:
            return
        previous = self._state
        self._state = state
        self._state_since = time.time()
        self.broker.publish(
            {
                "event": "runtime.state_changed",
                "operation_id": operation_id,
                "state": state,
                "previous_state": previous,
                "timestamp": _utc_iso(),
            }
        )

    def _drain_pending(self) -> None:
        """Publish transitions recorded by download threads. Loop-side only.

        All queued job events are published in their recorded order before
        the runtime state is recomputed once, so a fast download cannot see
        its runtime.state_changed land in the middle of its own progress.
        """
        with self._jobs_lock:
            drained = list(self._pending_events)
            self._pending_events.clear()
        for event in drained:
            self.broker.publish(event)
        if drained:
            self._set_state(self._compute_state(), drained[-1]["operation_id"])

    def poll(self, operation_id: str | None = None) -> None:
        """Reconcile the published state with current facts."""
        self._drain_pending()
        self._set_state(self._compute_state(), operation_id)

    # -- status / capabilities / models ---------------------------------------

    def status(self) -> dict[str, Any]:
        self.poll()
        residency = self.service.residency() or {}
        resident = bool(residency.get("loaded"))
        diagnostics = self._diagnostics()
        active = self._active_generation()
        path = self._model_path()
        return {
            "schema_version": SCHEMA_VERSION,
            "sidecar_version": _sidecar_version(),
            "backend": self.config.backend,
            "model": {
                "id": self.config.default_model,
                "revision": self.config.revision,
                "available": path is not None,
                "resident": resident,
                "resident_bytes": self.service.memory_bytes(),
            },
            "state": self._state,
            "state_since": _utc_iso(self._state_since),
            "memory": {
                "free_bytes": self._free_memory_bytes(),
                "required_bytes": self._required_bytes(),
                "admission": "allowed" if self._admission_allowed() else "blocked",
            },
            "generation": {
                "in_flight": int(diagnostics.get("inflight_generations") or 0),
                "queued": int(diagnostics.get("queued_generations") or 0),
                "active_request_id": (
                    _redact_id(active.generation_id) if active is not None else None
                ),
            },
            "reasoning": dict(_REASONING_CONTRACT),
            "idle_unload_after_seconds": int(self.settings.idle_unload_after_seconds),
            "settings_schema_version": SETTINGS_SCHEMA_VERSION,
        }

    async def capabilities(self) -> dict[str, Any]:
        capabilities = await self.service.backend.capabilities(
            self.config.default_model
        )
        return {
            "schema_version": SCHEMA_VERSION,
            "sidecar_version": _sidecar_version(),
            "backend": self.config.backend,
            "model": self.config.default_model,
            "reasoning": dict(_REASONING_CONTRACT),
            "capabilities": capabilities.json(),
            "control_api": {
                "version": SCHEMA_VERSION,
                "openapi_url": "/v1/synth/openapi.json",
                "events_url": "/v1/synth/events",
            },
        }

    def models(self) -> dict[str, Any]:
        """Control-plane inventory: what is on disk and what is resident.

        This is deliberately not the OpenAI /v1/models list — it reports
        filesystem and residency facts, not a serving catalog.
        """
        self.poll()
        residency = self.service.residency() or {}
        resident = bool(residency.get("loaded"))
        default = self.config.default_model
        entries: dict[str, dict[str, Any]] = {}
        for index in sorted(
            self.config.models_dir.glob("*/*/model.safetensors.index.json")
        ):
            model_id = f"{index.parent.parent.name}/{index.parent.name}"
            entries[model_id] = {
                "id": model_id,
                "available": True,
                "resident": False,
                "resident_bytes": None,
                "default": model_id == default,
                "path": str(index.parent),
            }
        default_path = self._model_path()
        entries.setdefault(
            default,
            {
                "id": default,
                "available": default_path is not None,
                "resident": False,
                "resident_bytes": None,
                "default": True,
                "path": str(default_path) if default_path is not None else None,
            },
        )
        if resident:
            entries[default]["resident"] = True
            entries[default]["resident_bytes"] = self.service.memory_bytes()
        return {
            "schema_version": SCHEMA_VERSION,
            "object": "list",
            "data": sorted(entries.values(), key=lambda entry: entry["id"]),
        }

    def metrics(self) -> dict[str, Any]:
        """JSON mirror of the rolling telemetry; Prometheus /metrics is a peer."""
        self.poll()
        snapshot = self.service.inference_snapshot()
        return {
            "schema_version": SCHEMA_VERSION,
            "timestamp": _utc_iso(),
            "model": self.config.default_model,
            "state": self._state,
            "resident": snapshot["resident"],
            "resident_bytes": snapshot["residentBytes"],
            "queue_depth": snapshot["queueDepth"],
            "queue_capacity": snapshot["queueCapacity"],
            "rolling": snapshot["rolling"],
            "prefill_histogram": self.service.prefill_histogram(),
        }

    # -- downloads -------------------------------------------------------------

    def get_download(self, job_id: str) -> dict[str, Any]:
        self.poll()
        with self._jobs_lock:
            job = self._jobs.get(job_id)
            if job is None:
                # The error vocabulary is fixed; an unknown operation handle is
                # reported under model_not_found rather than a new code.
                raise ControlError(
                    "model_not_found",
                    f"Unknown download job {job_id!r}.",
                    404,
                    details={"job_id": job_id},
                )
            return job.json()

    def start_download(self, model: str) -> dict[str, Any]:
        canonical = self.resolve_model(model)
        self.poll()
        with self._jobs_lock:
            for job in self._jobs.values():
                if job.model == canonical and job.state in {"queued", "downloading"}:
                    raise ControlError(
                        "download_in_progress",
                        f"A download for {canonical!r} is already running.",
                        409,
                        retryable=True,
                        details={"job_id": job.job_id},
                    )
        if self._model_path() is not None:
            raise ControlError(
                "invalid_state_transition",
                f"Weights for {canonical!r} are already on disk; nothing to download.",
                409,
                details={"model": canonical},
            )
        free_disk = shutil.disk_usage(self.config.models_dir).free
        if free_disk < REQUIRED_FREE_DISK_BYTES:
            raise ControlError(
                "download_failed",
                "Not enough free disk space to download the model safely.",
                507,
                details={
                    "reason": "insufficient_disk",
                    "required_free_bytes": REQUIRED_FREE_DISK_BYTES,
                    "free_bytes": free_disk,
                },
            )
        job_id = new_id("op")
        now = _utc_iso()
        job = DownloadJob(
            job_id=job_id,
            model=canonical,
            state="queued",
            bytes_done=None,
            bytes_total=None,
            error=None,
            created_at=now,
            updated_at=now,
        )
        with self._jobs_lock:
            self._jobs[job_id] = job
            while len(self._jobs) > self._max_jobs:
                self._jobs.popitem(last=False)
        self._set_state(self._compute_state(), job_id)
        destination = self.config.models_dir / canonical
        downloader = self.downloader

        def record(state: str, *, error: str | None = None) -> None:
            with self._jobs_lock:
                previous = job.state
                job.state = state
                job.error = error
                job.updated_at = _utc_iso()
                self._pending_events.append(
                    {
                        "event": "download.state_changed",
                        "operation_id": job_id,
                        "state": state,
                        "previous_state": previous,
                        "timestamp": job.updated_at,
                        "model": canonical,
                        "bytes_done": job.bytes_done,
                        "bytes_total": job.bytes_total,
                        "error": error,
                    }
                )
            if state == "downloaded":
                self._downloaded_marker = True
                self._last_error = None
            elif state == "failed":
                self._last_error = {"code": "download_failed", "message": error}

        def progress(bytes_done: int | None, bytes_total: int | None) -> None:
            with self._jobs_lock:
                job.bytes_done = bytes_done
                job.bytes_total = bytes_total
                job.updated_at = _utc_iso()
                self._pending_events.append(
                    {
                        "event": "download.progress",
                        "operation_id": job_id,
                        "state": job.state,
                        "previous_state": job.state,
                        "timestamp": job.updated_at,
                        "model": canonical,
                        "bytes_done": bytes_done,
                        "bytes_total": bytes_total,
                        "error": None,
                    }
                )

        def worker() -> None:
            record("downloading")
            try:
                downloader.download(canonical, destination, progress)
            except BaseException as exc:  # a failed job must report, not vanish
                record("failed", error=str(exc))
                return
            record("downloaded")

        threading.Thread(
            target=worker, name=f"laguna-download-{job_id}", daemon=True
        ).start()
        return job.json()

    # -- load / unload ---------------------------------------------------------

    async def load(self, model: str) -> dict[str, Any]:
        canonical = self.resolve_model(model)
        async with self._load_lock:
            operation_id = new_id("op")
            self.poll(operation_id)
            with self._jobs_lock:
                active = next(
                    (
                        job
                        for job in self._jobs.values()
                        if job.model == canonical
                        and job.state in {"queued", "downloading"}
                    ),
                    None,
                )
            if active is not None:
                raise ControlError(
                    "download_in_progress",
                    f"Weights for {canonical!r} are still downloading.",
                    409,
                    retryable=True,
                    details={"job_id": active.job_id},
                )
            residency = self.service.residency() or {}
            if residency.get("loaded"):
                # Idempotent: a second load of the resident model is a no-op.
                return {
                    "operation_id": operation_id,
                    "model": canonical,
                    "state": self._state,
                    "resident": True,
                    "already_resident": True,
                }
            model_path = self._model_path()
            if model_path is None:
                raise ControlError(
                    "model_not_found",
                    f"No weights for {canonical!r} on disk; download them first.",
                    404,
                    details={"model": canonical, "models_dir": str(self.config.models_dir)},
                )
            loader = getattr(self.service.backend, "load", None)
            if loader is None:
                raise ControlError(
                    "invalid_state_transition",
                    "This backend does not manage local weights.",
                    409,
                    details={"backend": type(self.service.backend).__name__},
                )
            self._set_state("checking_memory", operation_id)
            if not self._admission_allowed():
                self._set_state("blocked_memory", operation_id)
                raise ControlError(
                    "insufficient_memory",
                    "This machine does not have enough unified memory to load the model.",
                    503,
                    details={
                        "required_bytes": self._required_bytes(),
                        "required_available_bytes": _required_available_memory_bytes(
                            model_path
                        ),
                        "available_bytes": self._free_memory_bytes(),
                    },
                )
            self._set_state("loading", operation_id)
            try:
                await loader()
            except ResponsesError as error:
                if error.code == "insufficient_system_memory":
                    self._set_state("blocked_memory", operation_id)
                    raise ControlError(
                        "insufficient_memory",
                        error.message,
                        503,
                        details={
                            "required_bytes": self._required_bytes(),
                            "required_available_bytes": _required_available_memory_bytes(
                                model_path
                            ),
                            "available_bytes": self._free_memory_bytes(),
                        },
                    ) from error
                self._last_error = {"code": error.code, "message": error.message}
                self._set_state("error", operation_id)
                raise ControlError(
                    "load_failed",
                    error.message,
                    500,
                    retryable=True,
                    details={"backend_code": error.code},
                ) from error
            except Exception as error:
                self._last_error = {"code": "load_failed", "message": str(error)}
                self._set_state("error", operation_id)
                raise ControlError(
                    "load_failed", str(error), 500, retryable=True
                ) from error
            self._last_error = None
            self._downloaded_marker = False
            self.poll(operation_id)
            return {
                "operation_id": operation_id,
                "model": canonical,
                "state": self._state,
                "resident": True,
                "already_resident": False,
            }

    async def unload(self, model: str) -> dict[str, Any]:
        canonical = self.resolve_model(model)
        operation_id = new_id("op")
        self.poll(operation_id)
        if self._unloading:
            raise ControlError(
                "unload_in_progress",
                "An unload is already running.",
                409,
                retryable=True,
            )
        residency = self.service.residency() or {}
        if not residency.get("loaded"):
            # Idempotent: unloading an unloaded model is a no-op.
            return {
                "operation_id": operation_id,
                "model": canonical,
                "state": self._state,
                "resident": False,
                "already_unloaded": True,
            }
        self._unloading = True
        self._set_state("unloading", operation_id)
        try:
            # Same eviction guard as the legacy /v1/synth/model/unload route:
            # a generation in flight keeps the weights.
            released = await self.service.unload_now()
        finally:
            self._unloading = False
        if not released:
            self.poll(operation_id)
            raise ControlError(
                "generation_busy",
                "A generation is using the model; retry once it completes.",
                409,
                retryable=True,
            )
        self._last_error = None
        self.poll(operation_id)
        return {
            "operation_id": operation_id,
            "model": canonical,
            "state": self._state,
            "resident": False,
            "already_unloaded": False,
        }

    # -- settings --------------------------------------------------------------

    def _settings_payload(self) -> dict[str, Any]:
        return {
            "schema_version": SETTINGS_SCHEMA_VERSION,
            "settings": self.settings.effective(),
            "source": {
                "path": str(self.settings.path),
                "loaded_at": self.settings.loaded_at,
            },
            # Startup-only facts, read-only here: they stay CLI/env config.
            "startup": {
                "backend": self.config.backend,
                "default_model": self.config.default_model,
                "models_dir": str(self.config.models_dir),
                "host": self.config.host,
                "port": self.config.port,
            },
        }

    def get_settings(self) -> dict[str, Any]:
        return self._settings_payload()

    def update_settings(self, changes: Any) -> dict[str, Any]:
        if not isinstance(changes, dict):
            raise ControlError(
                "invalid_setting",
                "The settings update body must be a JSON object.",
                400,
            )
        try:
            self.settings.update(changes, backend=self.service.backend)
        except SettingsError as error:
            raise ControlError(
                "invalid_setting",
                str(error),
                400,
                details={"field": error.field} if error.field else {},
            ) from error
        except OSError as error:
            # Values were validated but the file write failed; the running
            # daemon may now be ahead of the file on disk.
            raise ControlError(
                "settings_write_failed",
                f"Settings validated but could not be persisted: {error}",
                500,
                retryable=True,
                details={"path": str(self.settings.path)},
            ) from error
        return self._settings_payload()

    # -- openapi ---------------------------------------------------------------

    def openapi_document(self) -> dict[str, Any]:
        if self._openapi_cache is None:
            try:
                import yaml

                self._openapi_cache = yaml.safe_load(
                    _OPENAPI_PATH.read_text(encoding="utf-8")
                )
            except FileNotFoundError as error:
                raise ControlError(
                    "openapi_unavailable",
                    f"The checked-in OpenAPI document is missing: {_OPENAPI_PATH}",
                    503,
                    details={"path": str(_OPENAPI_PATH)},
                ) from error
            except ImportError as error:
                raise ControlError(
                    "openapi_unavailable",
                    "PyYAML is not installed in this environment.",
                    503,
                ) from error
        return self._openapi_cache


def register_control_routes(app: FastAPI, control: SynthControl) -> None:
    """Wire the /v1/synth control surface, matching app.py's routing style.

    Auth is inherited from the app-wide bearer middleware; every route here,
    including openapi.json, requires the same key as the generation surface.
    """

    @app.get("/v1/synth/status")
    async def synth_status() -> Any:
        try:
            return control.status()
        except ControlError as error:
            return error.response()

    @app.get("/v1/synth/capabilities")
    async def synth_capabilities() -> Any:
        try:
            return await control.capabilities()
        except ControlError as error:
            return error.response()

    @app.get("/v1/synth/models")
    async def synth_models() -> Any:
        try:
            return control.models()
        except ControlError as error:
            return error.response()

    @app.post("/v1/synth/models/{model:path}/download")
    async def synth_download(model: str) -> Any:
        try:
            return JSONResponse(status_code=202, content=control.start_download(model))
        except ControlError as error:
            return error.response()

    @app.get("/v1/synth/downloads/{job_id}")
    async def synth_download_job(job_id: str) -> Any:
        try:
            return control.get_download(job_id)
        except ControlError as error:
            return error.response()

    @app.post("/v1/synth/models/{model:path}/load")
    async def synth_load(model: str) -> Any:
        try:
            return await control.load(model)
        except ControlError as error:
            return error.response()

    @app.post("/v1/synth/models/{model:path}/unload")
    async def synth_unload(model: str) -> Any:
        try:
            return await control.unload(model)
        except ControlError as error:
            return error.response()

    @app.get("/v1/synth/metrics")
    async def synth_metrics() -> Any:
        try:
            return control.metrics()
        except ControlError as error:
            return error.response()

    @app.get("/v1/synth/settings")
    async def synth_settings() -> Any:
        try:
            return control.get_settings()
        except ControlError as error:
            return error.response()

    @app.put("/v1/synth/settings")
    async def synth_settings_update(request: Request) -> Any:
        try:
            body = await request.json()
        except (ValueError, UnicodeDecodeError):
            return ControlError(
                "invalid_setting", "The request body must be valid JSON.", 400
            ).response()
        try:
            return control.update_settings(body)
        except ControlError as error:
            return error.response()

    @app.get("/v1/synth/openapi.json")
    async def synth_openapi() -> Any:
        try:
            return control.openapi_document()
        except ControlError as error:
            return error.response()

    @app.get("/v1/synth/events")
    async def synth_events(request: Request) -> Any:
        queue = control.broker.subscribe(replay=True)

        async def events() -> AsyncIterator[bytes]:
            # Bounded like the inference stream: an undetected client loss
            # cannot leave a poller running for the daemon's lifetime.
            deadline = time.monotonic() + 3600
            try:
                while time.monotonic() < deadline:
                    # Polling reconciles generation-phase transitions that
                    # happen without a control operation attached.
                    control.poll()
                    try:
                        event = await asyncio.wait_for(queue.get(), timeout=0.25)
                    except (asyncio.TimeoutError, TimeoutError):
                        if await request.is_disconnected():
                            return
                        continue
                    payload = json.dumps(
                        event, ensure_ascii=False, separators=(",", ":")
                    )
                    yield f"event: {event['event']}\ndata: {payload}\n\n".encode()
                    if await request.is_disconnected():
                        return
            finally:
                control.broker.unsubscribe(queue)

        # Plain streaming response: this endpoint holds no generation slot,
        # so there is nothing for the disconnect-aware variant to rescue.
        return StreamingResponse(
            events(),
            media_type="text/event-stream",
            headers={"Cache-Control": "no-cache", "X-Accel-Buffering": "no"},
        )
