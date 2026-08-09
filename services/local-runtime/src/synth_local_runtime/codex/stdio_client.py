"""Synchronous NDJSON JSON-RPC client for ``codex app-server`` (stdio)."""

from __future__ import annotations

import json
import os
import subprocess
import threading
import time
from collections import deque
from pathlib import Path
from typing import Any, Callable


class CodexAppServerClient:
    def __init__(
        self,
        *,
        command: list[str],
        cwd: Path,
        env: dict[str, str],
        on_notification: Callable[[str, Any], None] | None = None,
    ) -> None:
        self._command = command
        self._cwd = cwd
        self._env = env
        self._on_notification = on_notification
        self._process: subprocess.Popen[bytes] | None = None
        self._next_id = 1
        self._pending: dict[int, dict[str, Any] | None] = {}
        self._pending_events = threading.Event()
        self._lock = threading.Lock()
        self._reader: threading.Thread | None = None
        self._stderr_tail: deque[str] = deque(maxlen=40)
        self._closed = False

    def start(self) -> None:
        if self._process and self._process.poll() is None:
            return
        self._process = subprocess.Popen(
            self._command,
            cwd=str(self._cwd),
            env=self._env,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            bufsize=0,
        )
        self._reader = threading.Thread(target=self._read_loop, name="codex-app-server-reader", daemon=True)
        self._reader.start()
        threading.Thread(target=self._drain_stderr, name="codex-app-server-stderr", daemon=True).start()

    def close(self) -> None:
        self._closed = True
        process = self._process
        if process is None:
            return
        if process.poll() is None:
            try:
                process.terminate()
                process.wait(timeout=3)
            except Exception:
                try:
                    process.kill()
                except Exception:
                    pass
        self._process = None

    def initialize(self) -> dict[str, Any]:
        result = self.request(
            "initialize",
            {
                "clientInfo": {
                    "name": "synth-desktop",
                    "title": "Synth Desktop",
                    "version": "0.1.0",
                },
                "capabilities": {"experimentalApi": True},
            },
            timeout=30,
        )
        self.notify("initialized", None)
        return result

    def request(self, method: str, params: Any, *, timeout: float = 120) -> dict[str, Any]:
        request_id = self._reserve_id()
        with self._lock:
            self._pending[request_id] = None
        self._write(
            {
                "jsonrpc": "2.0",
                "id": request_id,
                "method": method,
                "params": params,
            }
        )
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            with self._lock:
                payload = self._pending.get(request_id)
                if payload is not None:
                    self._pending.pop(request_id, None)
                    if "error" in payload:
                        raise RuntimeError(
                            f"codex app-server {method} error: {payload['error']}"
                        )
                    result = payload.get("result")
                    return result if isinstance(result, dict) else {"result": result}
            self._pending_events.wait(timeout=0.05)
            self._pending_events.clear()
            if self._process and self._process.poll() is not None:
                raise RuntimeError(
                    "codex app-server exited early: "
                    + " | ".join(list(self._stderr_tail)[-8:])
                )
        raise TimeoutError(f"codex app-server timed out waiting for {method}")

    def notify(self, method: str, params: Any) -> None:
        payload: dict[str, Any] = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            payload["params"] = params
        self._write(payload)

    def respond(self, response_id: Any, *, result: Any = None, error: Any = None) -> None:
        payload: dict[str, Any] = {"jsonrpc": "2.0", "id": response_id}
        if error is not None:
            payload["error"] = error
        else:
            payload["result"] = result
        self._write(payload)

    def _reserve_id(self) -> int:
        with self._lock:
            request_id = self._next_id
            self._next_id += 1
            return request_id

    def _write(self, payload: dict[str, Any]) -> None:
        process = self._process
        if process is None or process.stdin is None:
            raise RuntimeError("codex app-server is not running")
        data = json.dumps(payload, separators=(",", ":")).encode("utf-8") + b"\n"
        process.stdin.write(data)
        process.stdin.flush()

    def _read_loop(self) -> None:
        process = self._process
        if process is None or process.stdout is None:
            return
        buffer = b""
        while not self._closed:
            chunk = process.stdout.read(1)
            if not chunk:
                break
            buffer += chunk
            if chunk != b"\n":
                continue
            line = buffer.strip()
            buffer = b""
            if not line:
                continue
            try:
                message = json.loads(line.decode("utf-8"))
            except json.JSONDecodeError:
                continue
            if not isinstance(message, dict):
                continue
            if "id" in message and ("result" in message or "error" in message):
                with self._lock:
                    if message["id"] in self._pending:
                        self._pending[int(message["id"])] = message
                self._pending_events.set()
                continue
            # Server request (approvals etc.)
            if "id" in message and "method" in message:
                self._handle_server_request(message)
                continue
            method = str(message.get("method") or "")
            if method and self._on_notification:
                try:
                    self._on_notification(method, message.get("params"))
                except Exception:
                    pass

    def _handle_server_request(self, message: dict[str, Any]) -> None:
        method = str(message.get("method") or "")
        request_id = message.get("id")
        # Auto-approve local coding agent requests for ASAP dogfood.
        if method in {
            "commandExecution/requestApproval",
            "applyPatch/requestApproval",
            "fileChange/requestApproval",
            "permissions/request",
            "execCommandApproval",
        }:
            self.respond(
                request_id,
                result={"decision": "approved", "approved": True, "accept": True},
            )
            return
        # Default accept empty result for unknown server requests
        self.respond(request_id, result={})

    def _drain_stderr(self) -> None:
        process = self._process
        if process is None or process.stderr is None:
            return
        for raw in iter(process.stderr.readline, b""):
            line = raw.decode("utf-8", errors="replace").rstrip()
            if line:
                self._stderr_tail.append(line)
