from __future__ import annotations

import json
import urllib.error
import urllib.parse
import urllib.request
from typing import Any


class InternHttpError(RuntimeError):
    def __init__(self, status: int, message: str, body: Any = None) -> None:
        super().__init__(f"Intern HTTP {status}: {message}")
        self.status = status
        self.body = body


class InternHttpClient:
    """Small exact-mailbox HTTP client used when synth-ai is not installed."""

    def __init__(self, *, base_url: str, api_key: str, timeout: float = 45.0) -> None:
        self.base_url = base_url.rstrip("/")
        self.api_key = api_key
        self.timeout = timeout

    def request(
        self,
        method: str,
        path: str,
        *,
        body: dict[str, Any] | None = None,
        query: dict[str, Any] | None = None,
    ) -> Any:
        url = f"{self.base_url}{path}"
        if query:
            encoded = urllib.parse.urlencode(
                {key: value for key, value in query.items() if value is not None}
            )
            url = f"{url}?{encoded}"
        data = json.dumps(body).encode("utf-8") if body is not None else None
        request = urllib.request.Request(
            url,
            data=data,
            method=method,
            headers={
                "Authorization": f"Bearer {self.api_key}",
                "Accept": "application/json",
                "Content-Type": "application/json",
                "User-Agent": "synth-desktop-first-pass/0.1",
            },
        )
        try:
            with urllib.request.urlopen(request, timeout=self.timeout) as response:
                raw = response.read()
                if not raw:
                    return None
                return json.loads(raw.decode("utf-8"))
        except urllib.error.HTTPError as exc:
            raw = exc.read().decode("utf-8", errors="replace")
            try:
                error_body: Any = json.loads(raw) if raw else None
            except json.JSONDecodeError:
                error_body = raw
            message = (
                error_body.get("detail")
                if isinstance(error_body, dict) and isinstance(error_body.get("detail"), str)
                else exc.reason
            )
            raise InternHttpError(exc.code, str(message), error_body) from exc
        except urllib.error.URLError as exc:
            raise InternHttpError(0, f"backend unavailable: {exc.reason}") from exc

    def create_sync(self, *, idempotency_key: str, metadata: dict[str, Any]) -> dict[str, Any]:
        return self.request(
            "POST",
            "/smr/research-intern/sync-sessions",
            body={
                "objective": "",
                "idempotency_key": idempotency_key,
                "binding": {
                    "factory_id": None,
                    "project_id": None,
                    "effort_id": None,
                    "run_id": None,
                },
                "metadata": metadata,
                "execution_mode": "standard",
                "require_operator_approval": True,
            },
        )

    def get_sync(self, sync_session_id: str) -> dict[str, Any]:
        return self.request(
            "GET", f"/smr/research-intern/sync-sessions/{sync_session_id}"
        )

    def send_sync(
        self,
        sync_session_id: str,
        *,
        command_id: str,
        idempotency_key: str,
        expected_generation: int,
        body: str,
    ) -> dict[str, Any]:
        return self.request(
            "POST",
            f"/smr/research-intern/sync-sessions/{sync_session_id}/commands",
            body={
                "command_id": command_id,
                "idempotency_key": idempotency_key,
                "expected_generation": expected_generation,
                "command_kind": "operator_message",
                "payload": {"body": body, "context": {}, "turn_id": command_id},
                "execution_mode": "standard",
                "mode": "sync",
                "evidence_refs": [],
            },
        )

    def sync_events(
        self, sync_session_id: str, *, after_sequence: int, limit: int = 500
    ) -> list[dict[str, Any]]:
        result = self.request(
            "GET",
            f"/smr/research-intern/runtimes/sync/{sync_session_id}/events",
            query={"after_sequence": after_sequence, "limit": limit},
        )
        return _event_list(result)

    def ensure_async(self, *, idempotency_key: str, metadata: dict[str, Any]) -> dict[str, Any]:
        return self.request(
            "POST",
            "/smr/research-intern/async/ensure",
            body={
                "objective": "",
                "idempotency_key": idempotency_key,
                "binding": {
                    "factory_id": None,
                    "project_id": None,
                    "effort_id": None,
                    "run_id": None,
                },
                "budget": {"maximum_concurrent_runs": 1},
                "metadata": metadata,
                "factory_ready_wait_seconds": 0,
            },
        )

    def get_async(self) -> dict[str, Any]:
        return self.request("GET", "/smr/research-intern/async")

    def send_async(
        self,
        *,
        command_id: str,
        idempotency_key: str,
        expected_generation: int,
        kind: str,
        body: str | None = None,
        context: dict[str, Any] | None = None,
    ) -> dict[str, Any]:
        instruction_payload: dict[str, Any] = {"context": context or {}}
        if body is not None:
            instruction_payload["body"] = body
        return self.request(
            "POST",
            "/smr/research-intern/async/messages",
            body={
                "command_id": command_id,
                "idempotency_key": idempotency_key,
                "expected_generation": expected_generation,
                "command_kind": kind,
                "payload": instruction_payload,
            },
        )

    def command_async(
        self,
        *,
        command_id: str,
        idempotency_key: str,
        expected_generation: int,
        kind: str,
        payload: dict[str, Any],
    ) -> dict[str, Any]:
        return self.request(
            "POST",
            "/smr/research-intern/async/commands",
            body={
                "command_id": command_id,
                "idempotency_key": idempotency_key,
                "expected_generation": expected_generation,
                "command_kind": kind,
                "payload": payload,
            },
        )

    def async_events(self, *, after_sequence: int, limit: int = 500) -> list[dict[str, Any]]:
        result = self.request(
            "GET",
            "/smr/research-intern/async/events",
            query={"after_sequence": after_sequence, "limit": limit},
        )
        return _event_list(result)


def _event_list(value: Any) -> list[dict[str, Any]]:
    if isinstance(value, list):
        return [item for item in value if isinstance(item, dict)]
    if isinstance(value, dict):
        for key in ("events", "items", "data"):
            items = value.get(key)
            if isinstance(items, list):
                return [item for item in items if isinstance(item, dict)]
    return []
