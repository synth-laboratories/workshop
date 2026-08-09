"""Configured OpenRouter models, always mediated by Codex app-server."""

from __future__ import annotations

from typing import Any

from .local_laguna import LocalLagunaAdapter
from ..codex import CodexLaunchConfig, resolve_workspace


class OpenRouterAdapter(LocalLagunaAdapter):
    """Run remote models as Codex agents over OpenRouter's Responses API."""

    def _config(self, session: dict[str, Any]) -> CodexLaunchConfig:
        api_key = self.service.config.openrouter_api_key
        if not api_key:
            raise RuntimeError("OPENROUTER_API_KEY is not configured")
        workspace = resolve_workspace(
            session_metadata=session.get("metadata"),
            workshop_root=self.service.config.workshop_root,
        )
        return CodexLaunchConfig(
            codex_home=self.service.config.data_dir / "codex" / session["id"],
            laguna_base_url="https://openrouter.ai/api/v1",
            laguna_api_key=api_key,
            model=str(session["target"]["model"]),
            workspace=workspace,
            workshop_root=self.service.config.workshop_root,
            provider_name="openrouter",
            provider_title="OpenRouter Responses",
            provider_env_key="OPENROUTER_API_KEY",
        )
