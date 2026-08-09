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

    async def ensure_ready(self) -> dict[str, Any]:
        async with self._lock:
            if self.config.backend == "mock":
                self.state = "ready"
                return self.status()
            if self.config.backend == "external":
                ok = await self._ping(self.config.upstream_url)
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
                return
            await asyncio.sleep(1.0)

        self.state = "error"
        self.last_error = "timed out waiting for mlx_lm.server to become ready"

    async def _ping(self, base: str) -> bool:
        try:
            async with httpx.AsyncClient(timeout=1.5) as client:
                for path in ("/v1/models", "/health", "/"):
                    try:
                        response = await client.get(f"{base}{path}")
                        if response.status_code < 500:
                            return True
                    except httpx.HTTPError:
                        continue
        except httpx.HTTPError:
            return False
        return False

    async def _stop(self) -> None:
        process = self.process
        self.process = None
        if process is None or process.poll() is not None:
            return
        try:
            os.killpg(process.pid, signal.SIGTERM)
            await asyncio.to_thread(process.wait, 10)
        except Exception:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except Exception:
                pass

    async def shutdown(self) -> None:
        async with self._lock:
            await self._stop()
            self.state = "unloaded"
