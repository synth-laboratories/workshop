"""Long-lived Codex app-server session backed by an OpenAI Responses provider."""

from __future__ import annotations

import os
import threading
from pathlib import Path
from typing import Any, Callable

from .config import CodexLaunchConfig, ensure_codex_home, resolve_codex_bin
from .stdio_client import CodexAppServerClient


NotificationHandler = Callable[[str, Any], None]


def _nested_id(payload: dict[str, Any], key: str) -> str | None:
    value = payload.get(key)
    if isinstance(value, str) and value:
        return value
    nested = payload.get(key.removesuffix("Id"))
    if isinstance(nested, dict):
        candidate = nested.get("id")
        if isinstance(candidate, str) and candidate:
            return candidate
    return None


class CodexAgentSession:
    """Own one app-server process and one durable Codex thread."""

    def __init__(
        self,
        config: CodexLaunchConfig,
        *,
        thread_id: str | None = None,
        on_notification: NotificationHandler | None = None,
        client_factory: Callable[..., CodexAppServerClient] = CodexAppServerClient,
    ) -> None:
        self.config = config
        self.thread_id = thread_id
        self.turn_id: str | None = None
        self._handler = on_notification
        self._turn_done = threading.Event()
        self._turn_error: RuntimeError | None = None
        self._turn_terminal_method: str | None = None
        self._started = False
        self._lock = threading.RLock()

        home = ensure_codex_home(config)
        env = dict(os.environ)
        env["CODEX_HOME"] = str(home)
        env[config.provider_env_key] = config.laguna_api_key
        command = [resolve_codex_bin(), "app-server", "--listen", "stdio://"]
        self.client = client_factory(
            command=command,
            cwd=config.workspace,
            env=env,
            on_notification=self._on_notification,
        )

    def start(self) -> str:
        with self._lock:
            if self._started and self.thread_id:
                return self.thread_id
            self.client.start()
            self.client.initialize()
            if self.thread_id:
                result = self.client.request("thread/resume", {
                    "threadId": self.thread_id,
                    "model": self.config.model,
                    "cwd": str(self.config.workspace),
                    "approvalPolicy": self.config.approval_policy,
                    "sandbox": self.config.sandbox_mode,
                }, timeout=30)
            else:
                result = self.client.request(
                    "thread/start",
                    {
                        "model": self.config.model,
                        "cwd": str(self.config.workspace),
                        "approvalPolicy": self.config.approval_policy,
                        "sandbox": self.config.sandbox_mode,
                    },
                    timeout=30,
                )
            resolved = _nested_id(result, "threadId")
            if not resolved:
                raise RuntimeError(f"Codex thread response missing thread id: {result}")
            self.thread_id = resolved
            self._started = True
            return resolved

    def run_turn(self, prompt: str, *, timeout: float = 900) -> str:
        with self._lock:
            if not self._started:
                self.start()
            self._turn_done.clear()
            self._turn_error = None
            self._turn_terminal_method = None
            result = self.client.request(
                "turn/start",
                {
                    "threadId": self.thread_id,
                    "model": self.config.model,
                    "input": [{"type": "text", "text": prompt, "textElements": []}],
                    "approvalPolicy": self.config.approval_policy,
                },
                timeout=30,
            )
            turn_id = _nested_id(result, "turnId")
            if not turn_id:
                raise RuntimeError(f"Codex turn/start response missing turn id: {result}")
            self.turn_id = turn_id
        if not self._turn_done.wait(timeout):
            raise TimeoutError(f"Codex turn {self.turn_id} did not complete")
        if self._turn_error:
            raise self._turn_error
        if self._turn_terminal_method == "turn/interrupted":
            raise InterruptedError(f"Codex turn {self.turn_id} was interrupted")
        return turn_id

    def interrupt(self) -> None:
        if not self.thread_id or not self.turn_id:
            return
        self.client.request(
            "turn/interrupt",
            {"threadId": self.thread_id, "turnId": self.turn_id},
            timeout=30,
        )

    def close(self) -> None:
        self.client.close()

    def _on_notification(self, method: str, params: Any) -> None:
        if self._handler:
            self._handler(method, params)
        if method in {"turn/completed", "turn/failed", "turn/interrupted"}:
            self._turn_terminal_method = method
            if method == "turn/failed":
                detail = params if isinstance(params, dict) else {"detail": params}
                self._turn_error = RuntimeError(f"Codex turn failed: {detail}")
            self._turn_done.set()
