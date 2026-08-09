from __future__ import annotations

import base64
import json
import os
import time
from pathlib import Path
from typing import Any

from cryptography.fernet import Fernet, InvalidToken

from .errors import ResponsesError
from .ids import new_id


class Compactor:
    def __init__(self, key_path: Path) -> None:
        self.key_path = key_path
        self._fernet: Fernet | None = None

    def _cipher(self) -> Fernet:
        if self._fernet is not None:
            return self._fernet
        self.key_path.parent.mkdir(parents=True, exist_ok=True)
        if self.key_path.exists():
            key = self.key_path.read_bytes().strip()
        else:
            key = Fernet.generate_key()
            descriptor = os.open(self.key_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
            with os.fdopen(descriptor, "wb") as handle:
                handle.write(key + b"\n")
        self._fernet = Fernet(key)
        return self._fernet

    def compact(self, items: list[dict[str, Any]], input_tokens: int) -> dict[str, Any]:
        envelope = {
            "version": 1,
            "items": items,
            "created_at": int(time.time()),
        }
        plaintext = json.dumps(envelope, ensure_ascii=False, separators=(",", ":")).encode()
        encrypted = self._cipher().encrypt(plaintext).decode()
        output_tokens = max(1, len(encrypted) // 4)
        return {
            "id": new_id("response"),
            "object": "response.compaction",
            "created_at": int(time.time()),
            "output": [
                {
                    "id": new_id("compaction"),
                    "type": "compaction",
                    "encrypted_content": encrypted,
                    "created_by": "synth-laguna-daemon",
                }
            ],
            "usage": {
                "input_tokens": input_tokens,
                "input_tokens_details": {"cached_tokens": 0},
                "output_tokens": output_tokens,
                "output_tokens_details": {"reasoning_tokens": 0},
                "total_tokens": input_tokens + output_tokens,
            },
        }

    def expand(self, item: dict[str, Any]) -> list[dict[str, Any]]:
        try:
            plaintext = self._cipher().decrypt(str(item["encrypted_content"]).encode())
            envelope = json.loads(plaintext)
        except (KeyError, InvalidToken, ValueError, TypeError) as exc:
            raise ResponsesError(
                "invalid_compaction_item",
                "The compaction item cannot be decrypted by this Laguna daemon.",
                400,
                "input",
            ) from exc
        items = envelope.get("items")
        if not isinstance(items, list):
            raise ResponsesError(
                "invalid_compaction_item",
                "The compaction item payload is malformed.",
                400,
                "input",
            )
        return [dict(entry) for entry in items if isinstance(entry, dict)]
