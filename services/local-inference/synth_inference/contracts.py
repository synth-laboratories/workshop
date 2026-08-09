from __future__ import annotations

from typing import Any, Literal

from pydantic import BaseModel, ConfigDict, Field


class ChatMessage(BaseModel):
    model_config = ConfigDict(extra="allow")

    role: Literal["system", "user", "assistant", "tool"]
    content: str | list[dict[str, Any]]
    name: str | None = None
    tool_call_id: str | None = None


class ChatCompletionRequest(BaseModel):
    model_config = ConfigDict(extra="allow", populate_by_name=True)

    model: str | None = None
    messages: list[ChatMessage]
    stream: bool = True
    max_tokens: int = Field(default=768, ge=1, le=32_768)
    temperature: float = Field(default=0.2, ge=0.0, le=2.0)
    top_p: float | None = Field(default=None, ge=0.0, le=1.0)
    stop: str | list[str] | None = None
    stream_options: dict[str, Any] | None = None
    tools: list[dict[str, Any]] | None = None
    tool_choice: str | dict[str, Any] | None = None
    adapter: str | None = None


class LoadModelRequest(BaseModel):
    model: str | None = None
    adapter: str | None = None
    draft_model: str | None = None


class ModelStatus(BaseModel):
    service: Literal["synth-local-inference"] = "synth-local-inference"
    requested_mode: str
    active_mode: Literal["mock", "mlx"]
    state: Literal["unloaded", "loading", "ready", "error"]
    model: str
    adapter: str | None = None
    draft_model: str | None = None
    upstream_url: str | None = None
    pid: int | None = None
    last_error: str | None = None
    log_path: str | None = None
    platform: str
    machine: str
    total_memory_gb: float | None = None
    recommended_model: str | None = None
    memory_warning: str | None = None
