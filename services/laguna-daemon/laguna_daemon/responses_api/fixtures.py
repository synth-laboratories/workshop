from __future__ import annotations

import hashlib
import json
import os
import threading
import time
from copy import deepcopy
from pathlib import Path
from typing import Any


class FixtureRecorder:
    """Explicit opt-in sanitized request capture for Codex contract fixtures."""

    def __init__(self) -> None:
        configured = os.getenv("SYNTH_LAGUNA_CAPTURE_DIR")
        self.directory = Path(configured).expanduser() if configured else None
        self._lock = threading.Lock()
        self._counter = 0

    def capture(
        self,
        body: Any,
        *,
        transport: str,
        kind: str = "request",
    ) -> Path | None:
        if self.directory is None or not isinstance(body, dict):
            return None
        sanitized = self._sanitize(deepcopy(body))
        envelope = {
            "captured_at": int(time.time()),
            "transport": transport,
            "source": "codex-app-server",
            kind: sanitized,
        }
        with self._lock:
            self._counter += 1
            index = self._counter
        self.directory.mkdir(parents=True, exist_ok=True)
        path = self.directory / f"codex-{kind}-{transport}-{index:04d}.json"
        path.write_text(
            json.dumps(envelope, indent=2, ensure_ascii=False, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        return path

    def _sanitize(
        self,
        value: Any,
        *,
        role: str | None = None,
        field: str | None = None,
    ) -> Any:
        if isinstance(value, dict):
            next_role = str(value.get("role") or role or "") or None
            result: dict[str, Any] = {}
            for key, child in value.items():
                if key in {"safety_identifier", "prompt_cache_key"}:
                    result[key] = "<redacted>"
                elif key == "client_metadata" and isinstance(child, dict):
                    result[key] = {metadata_key: "<redacted>" for metadata_key in child}
                elif key == "metadata":
                    result[key] = {}
                elif key == "instructions" and isinstance(child, str):
                    result[key] = self._fingerprint(child)
                else:
                    result[key] = self._sanitize(child, role=next_role, field=key)
            return result
        if isinstance(value, list):
            return [self._sanitize(child, role=role, field=field) for child in value]
        if (
            isinstance(value, str)
            and role in {"system", "developer", "assistant"}
            and field in {"text", "content"}
        ):
            return self._fingerprint(value)
        return value

    @staticmethod
    def _fingerprint(text: str) -> str:
        digest = hashlib.sha256(text.encode()).hexdigest()[:16]
        return f"<redacted sha256:{digest} chars:{len(text)}>"
