from __future__ import annotations

import os
import time
from dataclasses import dataclass
from pathlib import Path


DEFAULT_MODEL = "poolside/Laguna-XS-2.1-NVFP4-mlx"
DEFAULT_CONTEXT_LENGTH = 262_144


def _truthy(value: str | None, default: bool = False) -> bool:
    if value is None:
        return default
    return value.strip().lower() in {"1", "true", "yes", "on"}


@dataclass(frozen=True, slots=True)
class LagunaConfig:
    """Synth Laguna sidecar — Poolside-compatible OpenAI loopback."""

    host: str
    port: int
    backend: str  # mock | mlx_lm | external
    api_key: str | None
    models_dir: Path
    default_model: str
    model: str
    revision: str | None
    draft_model: str | None
    adapter: str | None
    external_url: str | None
    upstream_api_key: str | None
    data_dir: Path
    auto_load: bool
    idle_unload_after_seconds: int
    context_length: int
    started_at: float

    @property
    def upstream_url(self) -> str:
        """Base URL of a remote *native Responses* provider.

        This is a passthrough target for `backend == "external"` only. There is
        no local upstream: the daemon owns its MLX weights in-process and never
        manages or proxies a second local server.
        """
        if self.backend == "external" and self.external_url:
            return self.external_url.rstrip("/")
        raise RuntimeError(
            "No upstream is configured. The local backend serves MLX in-process."
        )

    @property
    def public_url(self) -> str:
        return f"http://{self.host}:{self.port}"

    def resolve_model_path(self, model_id: str | None = None) -> Path | None:
        """Resolve owner/name under models_dir (Poolside layout)."""
        mid = (model_id or self.default_model).strip()
        # Accept short aliases
        aliases = {
            "laguna-xs-2.1": self.default_model,
            "synth/Laguna-XS-2.1": self.default_model,
            "synth/Laguna-XS-2.1-NVFP4": self.default_model,
        }
        mid = aliases.get(mid, mid)
        candidate = self.models_dir / mid
        if candidate.exists():
            return candidate
        # Flat directory containing safetensors
        if (self.models_dir / "model.safetensors.index.json").exists():
            return self.models_dir
        # HF hub snapshot fallback
        if self.revision:
            safe = mid.replace("/", "--")
            snap = (
                Path.home()
                / ".cache"
                / "huggingface"
                / "hub"
                / f"models--{safe}"
                / "snapshots"
                / self.revision
            )
            if snap.exists() and any(snap.glob("*.safetensors")):
                return snap
        return None

    @classmethod
    def from_env(cls) -> "LagunaConfig":
        backend = (os.getenv("SYNTH_LAGUNA_BACKEND") or "auto").strip().lower()
        models_dir = Path(
            os.getenv("SYNTH_LAGUNA_MODELS_DIR")
            or os.getenv("SYNTH_MODELS_DIR")
            or (Path.home() / ".synth-desktop" / "models")
        ).expanduser()
        # Prefer existing Poolside weights if present and our dir empty
        poolside_models = Path.home() / ".config" / "poolside" / "models"
        if not any(models_dir.glob("**/*.safetensors")) and (
            poolside_models / "poolside" / "Laguna-XS-2.1-NVFP4-mlx"
        ).exists():
            models_dir = poolside_models

        if backend == "auto":
            if os.getenv("SYNTH_LAGUNA_EXTERNAL_URL"):
                backend = "external"
            elif os.uname().sysname == "Darwin" and os.uname().machine == "arm64":
                backend = "mlx_lm"
            else:
                backend = "mock"

        data_dir = Path(
            os.getenv("SYNTH_LAGUNA_DATA_DIR")
            or (Path.home() / ".synth-desktop" / "laguna")
        ).expanduser()
        data_dir.mkdir(parents=True, exist_ok=True)
        models_dir.mkdir(parents=True, exist_ok=True)

        default_model = (
            os.getenv("SYNTH_LAGUNA_DEFAULT_MODEL")
            or os.getenv("SYNTH_LAGUNA_MODEL")
            or DEFAULT_MODEL
        ).strip()

        api_key = (os.getenv("SYNTH_LAGUNA_API_KEY") or "").strip() or None
        # Generate a stable-ish local key file if none set (independent of Poolside)
        if api_key is None and _truthy(os.getenv("SYNTH_LAGUNA_REQUIRE_AUTH"), default=True):
            key_path = data_dir / "api_key"
            if key_path.exists():
                api_key = key_path.read_text(encoding="utf-8").strip() or None
            if not api_key:
                import secrets

                api_key = f"synth-local-{secrets.token_hex(24)}"
                key_path.write_text(api_key + "\n", encoding="utf-8")
                try:
                    key_path.chmod(0o600)
                except OSError:
                    pass

        return cls(
            host=os.getenv("SYNTH_LAGUNA_HOST", "127.0.0.1"),
            port=int(os.getenv("SYNTH_LAGUNA_PORT", "7333")),
            backend=backend,
            api_key=api_key,
            models_dir=models_dir,
            default_model=default_model,
            model=default_model,
            revision=(os.getenv("SYNTH_LAGUNA_REVISION") or "").strip() or None,
            draft_model=(os.getenv("SYNTH_LAGUNA_DRAFT_MODEL") or "").strip() or None,
            adapter=(os.getenv("SYNTH_LAGUNA_ADAPTER") or "").strip() or None,
            external_url=(os.getenv("SYNTH_LAGUNA_EXTERNAL_URL") or "").rstrip("/")
            or None,
            upstream_api_key=(
                os.getenv("SYNTH_LAGUNA_UPSTREAM_API_KEY")
                or os.getenv("SYNTH_LAGUNA_EXTERNAL_API_KEY")
                or ""
            ).strip()
            or None,
            data_dir=data_dir,
            auto_load=_truthy(os.getenv("SYNTH_LAGUNA_AUTO_LOAD"), default=True),
            idle_unload_after_seconds=int(
                # 15 minutes, matching the reference Poolside sidecar. Evicting
                # a 20 GB model between turns of a normal coding session costs
                # far more than it saves; set this low only to watch the
                # residency cycle during development.
                os.getenv("SYNTH_LAGUNA_IDLE_UNLOAD_SECONDS", "900")
            ),
            context_length=int(
                os.getenv("SYNTH_LAGUNA_CONTEXT_LENGTH", str(DEFAULT_CONTEXT_LENGTH))
            ),
            started_at=time.time(),
        )
