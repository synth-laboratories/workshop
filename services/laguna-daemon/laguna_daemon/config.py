from __future__ import annotations

import os
import time
from dataclasses import dataclass
from pathlib import Path


DEFAULT_MODEL = "poolside/Laguna-XS-2.1-NVFP4-mlx"
#: The one canonical Muse id. It matches the Hugging Face artifact, the Desktop
#: catalog, the `--alias` the engine advertises, and `selected_model_path`. The
#: pre-GGUF spelling below is accepted on input only, and normalized away at
#: config load, so a stale selection cannot fork the identity again.
MUSE_GLIMMER_MODEL = "meta-models/Muse-Glimmer-30B-GGUF"
MUSE_GLIMMER_LEGACY_MODEL = "meta-models/Muse-Glimmer-30B"
DEFAULT_CONTEXT_LENGTH = 262_144
#: The Muse engine's serve default. Kept here so the context window Codex is
#: told about, the compaction limit, and the engine's own `--ctx-size` agree.
MUSE_CONTEXT_LENGTH = 131_072

#: Backends that serve both wire surfaces from one turn core. `external` is the
#: native-Responses passthrough and is deliberately absent: it cannot serve
#: Chat, so it is never a local model's backend.
LOCAL_BACKENDS = frozenset({"mock", "mlx_lm", "llama_cpp"})


def _truthy(value: str | None, default: bool = False) -> bool:
    if value is None:
        return default
    return value.strip().lower() in {"1", "true", "yes", "on"}


@dataclass(frozen=True, slots=True)
class LagunaConfig:
    """Synth Laguna sidecar — Poolside-compatible OpenAI loopback."""

    host: str
    port: int
    backend: str  # mock | mlx_lm | llama_cpp | external
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
    #: Loopback address of a supervisor-owned GGUF engine. Defaulted because
    #: only a llama.cpp selection has one, and it is supplied by the process
    #: that started that engine.
    engine_url: str | None = None
    #: Bearer token the engine itself requires. Desktop gives the engine the
    #: same token that guards the daemon, so the weights are not reachable by
    #: any other local process through the engine's own port.
    engine_api_key: str | None = None

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
    def engine_base_url(self) -> str:
        """Loopback base of the supervisor-owned GGUF engine.

        This is *not* an upstream in the `external` sense: no client request is
        forwarded to it, and it has no Responses surface. It is the transport
        the local llama.cpp backend uses to reach weights that live in another
        process because a GGUF runtime cannot be loaded in-process. Both wire
        surfaces are still compiled, admitted, cancelled, and accounted for
        here. The daemon never starts, restarts, or discovers this process; the
        Desktop supervisor owns its lifecycle and passes the address in.
        """
        if self.engine_url:
            return self.engine_url.rstrip("/")
        raise RuntimeError(
            "No engine address is configured. Set SYNTH_LAGUNA_ENGINE_URL to the "
            "loopback address of the supervisor-owned GGUF engine."
        )

    @property
    def is_muse(self) -> bool:
        return self.default_model == MUSE_GLIMMER_MODEL

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
            MUSE_GLIMMER_LEGACY_MODEL: MUSE_GLIMMER_MODEL,
            "muse-glimmer": MUSE_GLIMMER_MODEL,
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
    def normalize_model_id(cls, model_id: str) -> str:
        """Collapse the accepted spellings of a model onto its canonical id."""
        return MUSE_GLIMMER_MODEL if model_id == MUSE_GLIMMER_LEGACY_MODEL else model_id

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

        data_dir = Path(
            os.getenv("SYNTH_LAGUNA_DATA_DIR")
            or (Path.home() / ".synth-desktop" / "laguna")
        ).expanduser()
        data_dir.mkdir(parents=True, exist_ok=True)
        models_dir.mkdir(parents=True, exist_ok=True)

        default_model = cls.normalize_model_id(
            (
                os.getenv("SYNTH_LAGUNA_DEFAULT_MODEL")
                or os.getenv("SYNTH_LAGUNA_MODEL")
                or DEFAULT_MODEL
            ).strip()
        )
        is_muse = default_model == MUSE_GLIMMER_MODEL
        external_url = (os.getenv("SYNTH_LAGUNA_EXTERNAL_URL") or "").rstrip("/") or None
        engine_url = (os.getenv("SYNTH_LAGUNA_ENGINE_URL") or "").rstrip("/") or None
        if is_muse and engine_url is None and external_url is not None:
            # A Desktop build from before the llama.cpp backend existed passes
            # the engine address as an `external` upstream. That address never
            # had a Responses surface, so honoring the old spelling as a
            # passthrough is the bug this backend replaces: read it as the
            # engine address instead, and drop the passthrough reading.
            engine_url = external_url
            external_url = None

        if backend == "auto":
            if is_muse or engine_url:
                backend = "llama_cpp"
            elif external_url:
                backend = "external"
            elif os.uname().sysname == "Darwin" and os.uname().machine == "arm64":
                backend = "mlx_lm"
            else:
                backend = "mock"
        elif is_muse and backend not in {"llama_cpp", "mock"}:
            # Fail closed on the historical mis-binding rather than serving a
            # surface that cannot answer: a GGUF engine speaks Chat
            # Completions, so neither the MLX loader nor the native-Responses
            # passthrough can drive it.
            backend = "llama_cpp"

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
            external_url=external_url,
            upstream_api_key=(
                os.getenv("SYNTH_LAGUNA_UPSTREAM_API_KEY")
                or os.getenv("SYNTH_LAGUNA_EXTERNAL_API_KEY")
                or ""
            ).strip()
            or None,
            engine_url=engine_url,
            engine_api_key=(os.getenv("SYNTH_LAGUNA_ENGINE_API_KEY") or "").strip()
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
                os.getenv("SYNTH_LAGUNA_CONTEXT_LENGTH")
                or (MUSE_CONTEXT_LENGTH if is_muse else DEFAULT_CONTEXT_LENGTH)
            ),
            started_at=time.time(),
        )
