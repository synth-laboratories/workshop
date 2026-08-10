from __future__ import annotations

import asyncio
import importlib.util
import os
import signal
import subprocess
import sys
import time
from typing import Any

import httpx

from .config import LagunaConfig


class LagunaProcessManager:
    """Owns the upstream MLX server process (or external URL)."""

    def __init__(self, config: LagunaConfig) -> None:
        self.config = config
        self.process: subprocess.Popen[bytes] | None = None
        self.state = "unloaded"
        self.last_error: str | None = None
        self.log_path = config.data_dir / "mlx_lm.server.log"
        self._lock = asyncio.Lock()
        self.last_used_at = time.time()
        self._active_requests = 0

    def status(self) -> dict[str, Any]:
        return {
            "backend": self.config.backend,
            "state": self.state,
            "model": self.config.default_model,
            "modelsDir": str(self.config.models_dir),
            "revision": self.config.revision,
            "draftModel": self.config.draft_model,
            "adapter": self.config.adapter,
            "upstreamUrl": self.config.upstream_url,
            "publicUrl": self.config.public_url,
            "pid": self.process.pid if self.process and self.process.poll() is None else None,
            "lastError": self.last_error,
            "logPath": str(self.log_path),
            "note": (
                "vanilla mlx_lm is the integration path; Arena/mere-run optimized "
                "kernels plug in via --backend external"
                if self.config.backend == "mlx_lm"
                else None
            ),
        }

    @property
    def idle_seconds(self) -> int:
        return max(0, int(time.time() - self.last_used_at))

    def begin_request(self) -> None:
        """Record a real prompt and keep eviction from interrupting its response."""
        self.last_used_at = time.time()
        self._active_requests += 1

    def end_request(self) -> None:
        self._active_requests = max(0, self._active_requests - 1)

    async def unload_if_idle(self, *, now: float | None = None) -> bool:
        """Evict an owned model once it has been idle for the configured window."""
        limit = self.config.idle_unload_after_seconds
        observed_at = time.time() if now is None else now
        if (
            limit <= 0
            or self.config.backend == "external"
            or self.state != "ready"
            or self._active_requests > 0
            or observed_at - self.last_used_at < limit
        ):
            return False
        async with self._lock:
            if (
                self.state != "ready"
                or self._active_requests > 0
                or observed_at - self.last_used_at < limit
            ):
                return False
            await self._stop()
            self.state = "unloaded"
            return True

    async def watch_idle(self) -> None:
        """Continuously mirror Laguna's 15-minute, prompt-driven residency."""
        while True:
            await asyncio.sleep(1.0)
            await self.unload_if_idle()

    async def ensure_ready(self) -> dict[str, Any]:
        async with self._lock:
            if self.config.backend == "mock":
                self.state = "ready"
                self.last_error = None
                return self.status()
            if self.config.backend == "external":
                ok = await self._ping(
                    self.config.upstream_url, api_key=self.config.upstream_api_key
                )
                self.state = "ready" if ok else "error"
                if not ok:
                    self.last_error = (
                        f"external Laguna server not reachable at {self.config.upstream_url}"
                    )
                return self.status()
            if self.process and self.process.poll() is None and self.state == "ready":
                return self.status()
            await self._start_mlx_lm()
            return self.status()

    async def _start_mlx_lm(self) -> None:
        if importlib.util.find_spec("mlx_lm") is None:
            self.state = "error"
            self.last_error = (
                "mlx-lm is not installed. Run ./scripts/laguna/setup.sh "
                "or use Docker profile mock."
            )
            return

        model_arg = self.config.default_model
        local = self.config.resolve_model_path(self.config.default_model)
        if local is not None:
            model_arg = str(local)
        else:
            self.state = "error"
            self.last_error = (
                f"model weights not found for {self.config.default_model} under "
                f"{self.config.models_dir}. Place safetensors at "
                f"{self.config.models_dir}/{self.config.default_model}/ "
                "or set SYNTH_LAGUNA_MODELS_DIR."
            )
            return

        await self._stop()
        self.state = "loading"
        self.last_error = None

        command = [
            sys.executable,
            "-m",
            "mlx_lm.server",
            "--host",
            self.config.upstream_host,
            "--port",
            str(self.config.upstream_port),
            "--model",
            model_arg,
        ]
        if self.config.adapter:
            command.extend(["--adapter-path", self.config.adapter])
        if self.config.draft_model:
            command.extend(["--draft-model", self.config.draft_model])

        self.log_path.parent.mkdir(parents=True, exist_ok=True)
        log_fd = self.log_path.open("ab", buffering=0)
        try:
            self.process = subprocess.Popen(
                command,
                stdout=log_fd,
                stderr=subprocess.STDOUT,
                cwd=str(self.config.data_dir),
                start_new_session=True,
                env={**os.environ},
            )
        except Exception as exc:  # pragma: no cover
            self.state = "error"
            self.last_error = f"failed to spawn mlx_lm.server: {exc}"
            return
        finally:
            log_fd.close()

        deadline = time.monotonic() + int(os.getenv("SYNTH_LAGUNA_LOAD_TIMEOUT_S", "900"))
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                self.state = "error"
                self.last_error = f"mlx_lm.server exited early; see {self.log_path}"
                return
            if await self._ping(self.config.upstream_url):
                self.state = "ready"
                # Auto-load time is not idle time. A prompt-triggered load keeps
                # the prompt timestamp recorded by begin_request().
                if self._active_requests == 0:
                    self.last_used_at = time.time()
                return
            await asyncio.sleep(1.0)

        self.state = "error"
        self.last_error = "timed out waiting for mlx_lm.server to become ready"

    async def external_health(self) -> dict[str, Any] | None:
        if self.config.backend != "external":
            return None
        headers = (
            {"Authorization": f"Bearer {self.config.upstream_api_key}"}
            if self.config.upstream_api_key
            else None
        )
        try:
            async with httpx.AsyncClient(timeout=1.5) as client:
                response = await client.get(
                    f"{self.config.upstream_url}/health", headers=headers
                )
                if response.is_success:
                    payload = response.json()
                    return payload if isinstance(payload, dict) else None
        except (httpx.HTTPError, ValueError):
            pass
        return None

    async def _ping(self, base: str, *, api_key: str | None = None) -> bool:
        headers = {"Authorization": f"Bearer {api_key}"} if api_key else None
        try:
            async with httpx.AsyncClient(timeout=1.5) as client:
                for path in ("/v1/models", "/health", "/"):
                    try:
                        response = await client.get(f"{base}{path}", headers=headers)
                        if response.status_code < 500:
                            return True
                    except httpx.HTTPError:
                        continue
        except httpx.HTTPError:
            return False
        return False

    async def _stop(self) -> None:
        process = self.process
        if process is None:
            return
        if process.poll() is not None:
            self.process = None
            return
        try:
            os.killpg(process.pid, signal.SIGTERM)
            await asyncio.to_thread(process.wait, 10)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            await asyncio.to_thread(process.wait, 10)
        except ProcessLookupError:
            await asyncio.to_thread(process.wait, 10)
        finally:
            if process.poll() is not None:
                self.process = None

        if process.poll() is None:
            try:
                os.killpg(process.pid, signal.SIGKILL)
                await asyncio.to_thread(process.wait, 10)
            finally:
                if process.poll() is not None:
                    self.process = None

        if process.poll() is None:
            raise RuntimeError(f"owned MLX process {process.pid} did not terminate")

    async def shutdown(self) -> None:
        async with self._lock:
            await self._stop()
            self.state = "unloaded"
