from __future__ import annotations

import os
import tomllib
from dataclasses import dataclass
from pathlib import Path


INTERN_ENDPOINTS = {
    "prod": "https://api.usesynth.ai",
    "staging": "https://api-dev.usesynth.ai",
    "local": "http://127.0.0.1:8000",
}


def _truthy(value: str | None, *, default: bool = False) -> bool:
    if value is None:
        return default
    return value.strip().lower() in {"1", "true", "yes", "on"}


@dataclass(frozen=True, slots=True)
class RuntimeConfig:
    host: str
    port: int
    data_dir: Path
    runtime_token: str | None
    connection_file: Path | None
    backend_url: str
    synth_api_key: str | None
    intern_demo: bool
    laguna_base_url: str | None
    laguna_stub_delay_ms: int
    openrouter_api_key: str | None
    laguna_model_path: str | None
    visuals_root: Path | None
    workshop_root: Path | None
    intern_profile: str = "prod"
    intern_config_path: Path | None = None

    @property
    def database_path(self) -> Path:
        return self.data_dir / "runtime.sqlite3"

    @property
    def intern_mode(self) -> str:
        if self.synth_api_key and not self.intern_demo:
            return "remote"
        if self.intern_demo:
            return "demo"
        return "unconfigured"

    @property
    def openrouter_mode(self) -> str:
        return "ready" if self.openrouter_api_key else "unconfigured"

    @classmethod
    def from_env(
        cls,
        *,
        host: str | None = None,
        port: int | None = None,
        data_dir: str | Path | None = None,
        connection_file: str | Path | None = None,
    ) -> "RuntimeConfig":
        default_data_dir = Path.home() / ".synth-desktop" / "runtime"
        resolved_data_dir = Path(
            data_dir or os.getenv("SYNTH_RUNTIME_DATA_DIR") or default_data_dir
        ).expanduser()
        resolved_data_dir.mkdir(parents=True, exist_ok=True)

        config_path_value = os.getenv("SYNTH_INTERN_CONFIG")
        config_path = (
            Path(config_path_value).expanduser()
            if config_path_value
            else Path.home() / ".synth-desktop" / "config.toml"
        )
        file_config: dict[str, object] = {}
        if config_path.exists():
            try:
                with config_path.open("rb") as handle:
                    parsed = tomllib.load(handle)
                if isinstance(parsed, dict):
                    file_config = parsed
            except (OSError, tomllib.TOMLDecodeError):
                # A malformed optional config should not prevent the desktop
                # from booting; environment overrides remain authoritative.
                file_config = {}

        intern_config = file_config.get("intern")
        intern_table = intern_config if isinstance(intern_config, dict) else {}
        profile_value = os.getenv("SYNTH_INTERN_PROFILE") or intern_table.get("profile") or "prod"
        profile = str(profile_value).strip().lower()
        if profile not in INTERN_ENDPOINTS:
            profile = "prod"

        endpoint = INTERN_ENDPOINTS[profile]
        endpoint_table = intern_table.get("endpoints")
        if isinstance(endpoint_table, dict):
            candidate = endpoint_table.get(profile)
            if isinstance(candidate, str) and candidate.strip():
                endpoint = candidate.strip()
        endpoint_override = os.getenv("SYNTH_BACKEND_URL")
        if endpoint_override:
            endpoint = endpoint_override

        env_api_key = os.getenv("SYNTH_API_KEY") or None
        demo_env = os.getenv("SYNTH_INTERN_DEMO")
        # Production is the default profile. Demo mode is explicit so a
        # configured desktop never silently sends work to a fake backend.
        intern_demo = _truthy(demo_env, default=False)

        delay_value = os.getenv("SYNTH_LAGUNA_STUB_DELAY_MS", "22")
        try:
            delay_ms = max(0, min(2_000, int(delay_value)))
        except ValueError:
            delay_ms = 22

        connection_path = connection_file or os.getenv("SYNTH_RUNTIME_CONNECTION_FILE")
        default_hf = (
            Path.home()
            / ".cache"
            / "huggingface"
            / "hub"
            / "models--poolside--Laguna-XS-2.1"
        )
        laguna_model_path = (
            os.getenv("SYNTH_LAGUNA_MODEL_PATH")
            or (str(default_hf) if default_hf.exists() else None)
        )

        workshop_root_env = os.getenv("SYNTH_WORKSHOP_ROOT")
        workshop_root = Path(workshop_root_env).expanduser() if workshop_root_env else None
        visuals_env = os.getenv("SYNTH_VISUALS_ROOT")
        if visuals_env:
            visuals_root = Path(visuals_env).expanduser()
        elif workshop_root and (workshop_root / "visuals").exists():
            visuals_root = workshop_root / "visuals"
        else:
            # best-effort relative to this package → workshop/
            candidate = Path(__file__).resolve().parents[4] / "visuals"
            visuals_root = candidate if candidate.exists() else None

        return cls(
            host=host or os.getenv("SYNTH_RUNTIME_HOST", "127.0.0.1"),
            port=port if port is not None else int(os.getenv("SYNTH_RUNTIME_PORT", "8765")),
            data_dir=resolved_data_dir,
            runtime_token=os.getenv("SYNTH_RUNTIME_TOKEN") or None,
            connection_file=Path(connection_path).expanduser() if connection_path else None,
            backend_url=endpoint.rstrip("/"),
            synth_api_key=env_api_key,
            intern_demo=intern_demo,
            laguna_base_url=(os.getenv("SYNTH_LAGUNA_BASE_URL") or "").rstrip("/") or None,
            laguna_stub_delay_ms=delay_ms,
            openrouter_api_key=os.getenv("OPENROUTER_API_KEY") or None,
            laguna_model_path=laguna_model_path,
            visuals_root=visuals_root,
            workshop_root=workshop_root,
            intern_profile=profile,
            intern_config_path=config_path,
        )
