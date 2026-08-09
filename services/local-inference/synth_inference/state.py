from __future__ import annotations

import asyncio
import importlib.util
import os
import platform
import signal
import subprocess
import sys
from pathlib import Path
from typing import Any

import httpx

from .contracts import ModelStatus


DEFAULT_MODEL_3BIT = "mlx-community/Laguna-XS-2.1-3bit"
DEFAULT_MODEL_4BIT = "mlx-community/Laguna-XS-2.1-4bit"


def _truthy(value: str | None) -> bool:
    return (value or "").strip().lower() in {"1", "true", "yes", "on"}


def _total_memory_gb() -> float | None:
    """Return physical memory without adding a heavy system dependency."""

    try:
        pages = int(os.sysconf("SC_PHYS_PAGES"))
        page_size = int(os.sysconf("SC_PAGE_SIZE"))
        return round((pages * page_size) / (1024**3), 1)
    except (AttributeError, OSError, TypeError, ValueError):
        pass

    # macOS does not expose SC_PHYS_PAGES consistently across Python builds.
    if platform.system() == "Darwin":
        try:
            raw = subprocess.check_output(
                ["/usr/sbin/sysctl", "-n", "hw.memsize"],
                stderr=subprocess.DEVNULL,
                text=True,
                timeout=2.0,
            )
            return round(int(raw.strip()) / (1024**3), 1)
        except (OSError, subprocess.SubprocessError, TypeError, ValueError):
            pass

    return None


def _default_model(total_memory_gb: float | None) -> str:
    # The 3-bit conversion is the only plausible XS 2.1 option on a 16 GB
    # machine. It remains tight once macOS and the desktop app are included,
    # so the status response also carries an explicit warning.
    if total_memory_gb is not None and total_memory_gb < 22:
        return DEFAULT_MODEL_3BIT
    return DEFAULT_MODEL_4BIT


def _memory_warning(model: str, total_memory_gb: float | None) -> str | None:
    if total_memory_gb is None:
        return None
    lower = model.lower()
    estimated_peak = 16.0 if "3bit" in lower else 20.0 if "4bit" in lower else 24.0
    recommended_total = estimated_peak + 4.0
    if total_memory_gb >= recommended_total:
        return None
    return (
        f"{model.rsplit('/', 1)[-1]} may exceed comfortable unified-memory headroom "
        f"on this {total_memory_gb:g} GB machine; {recommended_total:g} GB or more is recommended."
    )


class InferenceState:
    """Owns one MLX-VLM process or a deterministic mock backend.

    The manager intentionally wraps MLX-VLM instead of importing model internals.
    That keeps Laguna architecture support, adapter loading, DFlash and future
    batching improvements behind the upstream OpenAI-compatible server boundary.
    """

    def __init__(self) -> None:
        self.requested_mode = os.getenv("SYNTH_INFERENCE_MODE", "auto").strip().lower()
        self.total_memory_gb = _total_memory_gb()
        recommended_model = _default_model(self.total_memory_gb)
        self.model = os.getenv("SYNTH_LAGUNA_MODEL", recommended_model).strip() or recommended_model
        self.adapter = os.getenv("SYNTH_LAGUNA_ADAPTER", "").strip() or None
        self.draft_model = os.getenv("SYNTH_LAGUNA_DRAFT_MODEL", "").strip() or None
        self.upstream_port = int(os.getenv("SYNTH_INFERENCE_UPSTREAM_PORT", "7333"))
        self.upstream_url = f"http://127.0.0.1:{self.upstream_port}"
        self.data_dir = Path(os.getenv("SYNTH_INFERENCE_DATA_DIR", "~/.synth-desktop/inference")).expanduser()
        self.data_dir.mkdir(parents=True, exist_ok=True)
        self.log_path = self.data_dir / "mlx-vlm.log"
        self.process: subprocess.Popen[bytes] | None = None
        self._state = "unloaded"
        self.last_error: str | None = None
        self._lock = asyncio.Lock()
        self.active_mode = self._resolve_mode()
        if self.active_mode == "mock":
            self._state = "ready"

    def _resolve_mode(self) -> str:
        if self.requested_mode in {"mock", "mlx"}:
            return self.requested_mode
        is_apple_silicon = platform.system() == "Darwin" and platform.machine() in {"arm64", "aarch64"}
        has_mlx_vlm = importlib.util.find_spec("mlx_vlm") is not None
        return "mlx" if is_apple_silicon and has_mlx_vlm else "mock"

    async def startup(self) -> None:
        if _truthy(os.getenv("SYNTH_LAGUNA_AUTO_LOAD")):
            await self.load()

    async def shutdown(self) -> None:
        await self.unload()

    async def status(self, refresh: bool = True) -> ModelStatus:
        if self.active_mode == "mlx" and refresh:
            await self._refresh_process_state()
        return ModelStatus(
            requested_mode=self.requested_mode,
            active_mode=self.active_mode,  # type: ignore[arg-type]
            state=self._state,  # type: ignore[arg-type]
            model=self.model,
            adapter=self.adapter,
            draft_model=self.draft_model,
            upstream_url=self.upstream_url if self.active_mode == "mlx" else None,
            pid=self.process.pid if self.process and self.process.poll() is None else None,
            last_error=self.last_error,
            log_path=str(self.log_path),
            platform=platform.system(),
            machine=platform.machine(),
            total_memory_gb=self.total_memory_gb,
            recommended_model=_default_model(self.total_memory_gb),
            memory_warning=_memory_warning(self.model, self.total_memory_gb),
        )

    async def load(
        self,
        *,
        model: str | None = None,
        adapter: str | None = None,
        draft_model: str | None = None,
    ) -> ModelStatus:
        async with self._lock:
            if model:
                self.model = model
            if adapter is not None:
                self.adapter = adapter or None
            if draft_model is not None:
                self.draft_model = draft_model or None

            if self.active_mode == "mock":
                self._state = "ready"
                self.last_error = None
                return await self.status(refresh=False)

            if importlib.util.find_spec("mlx_vlm") is None:
                self._state = "error"
                self.last_error = (
                    "mlx-vlm is not installed. Run `python3 scripts/setup_python.py --mlx` "
                    "on an Apple Silicon Mac."
                )
                return await self.status(refresh=False)

            await self._stop_process()
            self._state = "loading"
            self.last_error = None
            command = [
                sys.executable,
                "-m",
                "mlx_vlm.server",
                "--host",
                "127.0.0.1",
                "--port",
                str(self.upstream_port),
                "--model",
                self.model,
            ]
            if self.adapter:
                command.extend(["--adapter-path", self.adapter])
            if self.draft_model:
                command.extend(["--draft-model", self.draft_model])

            log_handle = self.log_path.open("ab", buffering=0)
            try:
                self.process = subprocess.Popen(
                    command,
                    stdout=log_handle,
                    stderr=subprocess.STDOUT,
                    cwd=str(self.data_dir),
                    start_new_session=True,
                )
            except Exception as exc:  # pragma: no cover - platform dependent
                log_handle.close()
                self._state = "error"
                self.last_error = f"Failed to start mlx-vlm: {exc}"
                return await self.status(refresh=False)
            finally:
                # Popen duplicates the descriptor for the child. Keeping the
                # parent handle open would leak one descriptor on every reload.
                log_handle.close()

            process = self.process
            asyncio.create_task(
                self._watch_until_ready(process),
                name="mlx-vlm-ready-watch",
            )
            return await self.status(refresh=False)

    async def unload(self) -> ModelStatus:
        async with self._lock:
            await self._stop_process()
            self._state = "ready" if self.active_mode == "mock" else "unloaded"
            return await self.status(refresh=False)

    async def _stop_process(self) -> None:
        process = self.process
        self.process = None
        if process is None or process.poll() is not None:
            return
        try:
            if os.name == "posix":
                os.killpg(process.pid, signal.SIGTERM)
            else:  # pragma: no cover - Windows fallback
                process.terminate()
            await asyncio.to_thread(process.wait, 8)
        except Exception:
            try:
                if os.name == "posix":
                    os.killpg(process.pid, signal.SIGKILL)
                else:  # pragma: no cover
                    process.kill()
            except Exception:
                pass

    async def _refresh_process_state(self) -> None:
        process = self.process
        if process is None:
            if self._state not in {"error", "unloaded"}:
                self._state = "unloaded"
            return
        return_code = process.poll()
        if return_code is not None:
            if self._state != "unloaded":
                self._state = "error"
                self.last_error = f"mlx-vlm exited with code {return_code}; see {self.log_path}"
            return
        try:
            async with httpx.AsyncClient(timeout=0.8) as client:
                response = await client.get(f"{self.upstream_url}/health")
                if response.is_success:
                    self._state = "ready"
                    self.last_error = None
        except httpx.HTTPError:
            if self._state != "error":
                self._state = "loading"

    async def _watch_until_ready(self, expected_process: subprocess.Popen[bytes]) -> None:
        for _ in range(3600):
            if self.process is not expected_process:
                return
            await self._refresh_process_state()
            if self._state in {"ready", "error", "unloaded"}:
                return
            await asyncio.sleep(1.0)

    async def proxy_metrics(self) -> dict[str, Any]:
        if self.active_mode != "mlx" or self._state != "ready":
            return {"mode": self.active_mode, "state": self._state}
        try:
            async with httpx.AsyncClient(timeout=2.0) as client:
                response = await client.get(f"{self.upstream_url}/v1/metrics")
                if response.is_success:
                    return response.json()
        except (httpx.HTTPError, ValueError):
            pass
        return {"mode": self.active_mode, "state": self._state}
