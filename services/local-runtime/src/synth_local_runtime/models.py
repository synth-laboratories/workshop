from __future__ import annotations

from dataclasses import dataclass
from datetime import UTC, datetime
from typing import Any, Literal
from uuid import uuid4

JSON = dict[str, Any]
Source = Literal["local", "remote", "intern", "system"]


def utc_now() -> str:
    return datetime.now(UTC).isoformat().replace("+00:00", "Z")


def new_id(prefix: str) -> str:
    return f"{prefix}_{uuid4().hex}"


OPENROUTER_MODELS = {
    "moonshotai/kimi-k2.5",
    "poolside/laguna-s-2.1",
    "openai/gpt-5.4-nano",
}


def validate_target(value: object) -> JSON:
    if not isinstance(value, dict):
        raise ValueError("target must be an object")
    kind = value.get("kind")
    if kind == "local":
        model = value.get("model", "laguna-xs-2.1")
        if model != "laguna-xs-2.1":
            raise ValueError("local target currently supports laguna-xs-2.1")
        adapter = value.get("adapter")
        if adapter is not None and not isinstance(adapter, str):
            raise ValueError("target.adapter must be a string or null")
        return {"kind": "local", "model": model, "adapter": adapter}
    if kind == "remote":
        provider = value.get("provider", "openrouter")
        if provider != "openrouter":
            raise ValueError("remote provider must be openrouter")
        model = value.get("model")
        if not isinstance(model, str) or not model:
            raise ValueError("remote target.model is required")
        adapter = value.get("adapter")
        if adapter is not None and not isinstance(adapter, str):
            raise ValueError("target.adapter must be a string or null")
        return {
            "kind": "remote",
            "provider": provider,
            "model": model,
            "adapter": adapter,
        }
    if kind == "intern":
        mode = value.get("mode")
        if mode not in {"sync", "async"}:
            raise ValueError("Intern target.mode must be sync or async")
        target: JSON = {"kind": "intern", "mode": mode}
        binding = value.get("binding")
        if binding is not None:
            if not isinstance(binding, dict):
                raise ValueError("target.binding must be an object")
            target["binding"] = binding
        return target
    raise ValueError("target.kind must be local, remote, or intern")


def default_title(target: JSON) -> str:
    if target["kind"] == "local":
        return "Laguna session"
    if target["kind"] == "remote":
        model = str(target.get("model") or "openrouter")
        short = model.split("/")[-1]
        return f"OpenRouter · {short}"
    return "Intern live" if target["mode"] == "sync" else "Intern background"


@dataclass(slots=True)
class EventInput:
    session_id: str
    event_kind: str
    payload: JSON
    source: Source
    run_id: str | None = None
    command_id: str | None = None
    remote_sequence: int | None = None
    created_at: str | None = None
