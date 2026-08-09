from __future__ import annotations

import os
import shutil
from dataclasses import dataclass
from pathlib import Path


DEFAULT_MODEL = "poolside/Laguna-XS-2.1-NVFP4-mlx"


@dataclass(frozen=True, slots=True)
class CodexLaunchConfig:
    codex_home: Path
    laguna_base_url: str
    laguna_api_key: str
    model: str
    workspace: Path
    workshop_root: Path | None = None
    approval_policy: str = "never"
    sandbox_mode: str = "workspace-write"
    enable_visuals_mcp: bool = True
    provider_name: str = "custom"
    provider_title: str = "Synth Laguna XS"
    provider_env_key: str = "SYNTH_LAGUNA_API_KEY"


def resolve_codex_bin() -> str:
    configured = (os.getenv("SYNTH_CODEX_BIN") or "").strip()
    if configured:
        return configured
    found = shutil.which("codex")
    if not found:
        raise RuntimeError(
            "codex binary not found. Install Codex CLI or set SYNTH_CODEX_BIN."
        )
    return found


def resolve_workspace(
    *,
    session_metadata: dict | None = None,
    workshop_root: Path | None = None,
) -> Path:
    meta = session_metadata or {}
    for key in ("cwd", "workspace", "projectPath"):
        raw = meta.get(key)
        if isinstance(raw, str) and raw.strip():
            path = Path(raw).expanduser()
            if path.exists():
                return path.resolve()
    env = (os.getenv("SYNTH_DESKTOP_WORKSPACE") or "").strip()
    if env:
        path = Path(env).expanduser()
        if path.exists():
            return path.resolve()
    if workshop_root and workshop_root.exists():
        return workshop_root.resolve()
    return Path.cwd().resolve()


def ensure_codex_home(config: CodexLaunchConfig) -> Path:
    """Write an isolated CODEX_HOME pointed at Laguna Responses API."""
    home = config.codex_home
    home.mkdir(parents=True, exist_ok=True)
    (home / "sessions").mkdir(parents=True, exist_ok=True)

    base = config.laguna_base_url.rstrip("/")
    model = config.model
    lines = [
        f'model = "{model}"',
        f'model_provider = "{config.provider_name}"',
        f'approval_policy = "{config.approval_policy}"',
        f'sandbox_mode = "{config.sandbox_mode}"',
        'service_tier = "default"',
        "",
        f"[model_providers.{config.provider_name}]",
        f'name = "{config.provider_title}"',
        f'base_url = "{base}"',
        f'env_key = "{config.provider_env_key}"',
        'wire_api = "responses"',
        'requires_openai_auth = false',
        "",
        "[features]",
        "tool_call_mcp_elicitation = true",
        "shell_tool = true",
        "unified_exec = true",
    ]

    if config.enable_visuals_mcp and config.workshop_root:
        mcp_script = config.workshop_root / "services" / "local-runtime" / "src"
        lines.extend(
            [
                "",
                "[mcp_servers.synth_visuals]",
                "enabled = true",
                'command = "python3"',
                'args = ["-m", "synth_local_runtime.mcp_visuals"]',
                f'cwd = "{config.workshop_root}"',
                "startup_timeout_sec = 30",
                "",
                "[mcp_servers.synth_visuals.env]",
                f'PYTHONPATH = "{mcp_script}"',
                f'SYNTH_WORKSHOP_ROOT = "{config.workshop_root}"',
            ]
        )

    (home / "config.toml").write_text("\n".join(lines) + "\n", encoding="utf-8")

    # Auth stub so providers that peek at auth.json don't crash; Laguna uses bearer env.
    auth_path = home / "auth.json"
    if not auth_path.exists():
        auth_path.write_text(
            '{\n  "OPENAI_API_KEY": "synth-desktop-laguna"\n}\n',
            encoding="utf-8",
        )
    return home
