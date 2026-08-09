from __future__ import annotations

import json
import threading
import time
import urllib.error
import urllib.request
from typing import Any, Iterator

from .base import RuntimeAdapter
from ..models import new_id, utc_now

# Approximate OpenRouter prices ($ / 1M tokens) for local ledger — illustrative.
_MODEL_PRICES: dict[str, tuple[float, float]] = {
    "moonshotai/kimi-k2.5": (0.5, 2.4),
    "poolside/laguna-s-2.1": (0.4, 1.6),
    "openai/gpt-5.4-nano": (0.1, 0.4),
}


class OpenRouterAdapter(RuntimeAdapter):
    """Remote chat completions via OpenRouter with a local usage ledger."""

    def __init__(self, service: "Any") -> None:
        super().__init__(service)
        self._cancel_events: dict[str, threading.Event] = {}
        self._lock = threading.Lock()

    def send_message(self, session: dict[str, Any], run: dict[str, Any], body: str) -> None:
        cancel_event = threading.Event()
        with self._lock:
            self._cancel_events[run["id"]] = cancel_event
        thread = threading.Thread(
            target=self._run,
            name=f"openrouter-{run['id']}",
            args=(session, run, body, cancel_event),
            daemon=True,
        )
        thread.start()

    def control(
        self,
        session: dict[str, Any],
        kind: str,
        payload: dict[str, Any],
    ) -> dict[str, Any]:
        if kind != "cancel":
            raise ValueError("OpenRouter currently supports only cancel")
        run_id = session.get("activeRunId")
        if not run_id:
            return {"accepted": False, "reason": "no_active_run"}
        with self._lock:
            cancel_event = self._cancel_events.get(run_id)
        if cancel_event is None:
            return {"accepted": False, "reason": "run_not_active"}
        cancel_event.set()
        return {"accepted": True}

    def _run(
        self,
        session: dict[str, Any],
        run: dict[str, Any],
        body: str,
        cancel_event: threading.Event,
    ) -> None:
        started = time.monotonic()
        message_id = new_id("msg")
        target = session["target"]
        model = target.get("model") or "moonshotai/kimi-k2.5"
        self.service.mark_run_started(run["id"])
        self.service.emit(
            session_id=session["id"],
            run_id=run["id"],
            source="remote",
            event_kind="run.started",
            payload={
                "model": model,
                "adapter": target.get("adapter"),
                "provider": "openrouter",
                "transport": "openrouter",
            },
        )

        output_parts: list[str] = []
        completion_tokens = 0
        prompt_tokens = max(1, len(body.split()))
        try:
            for delta in self._stream_completion(model, body, cancel_event):
                if cancel_event.is_set():
                    self._cancel(session, run, output_parts)
                    return
                output_parts.append(delta)
                completion_tokens += 1
                self.service.emit(
                    session_id=session["id"],
                    run_id=run["id"],
                    source="remote",
                    event_kind="message.delta",
                    payload={
                        "messageId": message_id,
                        "role": "assistant",
                        "delta": delta,
                    },
                )

            if cancel_event.is_set():
                self._cancel(session, run, output_parts)
                return

            content = "".join(output_parts).strip()
            elapsed_ms = round((time.monotonic() - started) * 1_000, 1)
            completion_tokens = max(completion_tokens, max(1, len(content.split())))
            cost = self._estimate_cost(model, prompt_tokens, completion_tokens)
            self.service.emit(
                session_id=session["id"],
                run_id=run["id"],
                source="remote",
                event_kind="message.completed",
                payload={
                    "messageId": message_id,
                    "role": "assistant",
                    "content": content,
                },
            )
            self.service.emit(
                session_id=session["id"],
                run_id=run["id"],
                source="remote",
                event_kind="usage.recorded",
                payload={
                    "provider": "openrouter",
                    "model": model,
                    "promptTokens": prompt_tokens,
                    "completionTokens": completion_tokens,
                    "totalTokens": prompt_tokens + completion_tokens,
                    "costUsd": cost,
                    "elapsedMs": elapsed_ms,
                },
            )
            self.service.record_usage(
                provider="openrouter",
                model=model,
                session_id=session["id"],
                run_id=run["id"],
                prompt_tokens=prompt_tokens,
                completion_tokens=completion_tokens,
                cost_usd=cost,
            )
            self.service.complete_run(
                run["id"],
                session["id"],
                outcome={
                    "kind": "completed",
                    "model": model,
                    "provider": "openrouter",
                    "elapsedMs": elapsed_ms,
                    "usage": {
                        "promptTokens": prompt_tokens,
                        "completionTokens": completion_tokens,
                        "costUsd": cost,
                    },
                },
            )
        except Exception as exc:
            self.service.fail_run(run["id"], session["id"], exc)
        finally:
            with self._lock:
                self._cancel_events.pop(run["id"], None)

    def _cancel(
        self,
        session: dict[str, Any],
        run: dict[str, Any],
        output_parts: list[str],
    ) -> None:
        self.service.emit(
            session_id=session["id"],
            run_id=run["id"],
            source="remote",
            event_kind="run.cancelled",
            payload={"partial": "".join(output_parts)},
        )
        self.service.mark_run_terminal(
            run["id"],
            session["id"],
            status="cancelled",
            outcome={"kind": "cancelled"},
            session_status="ready",
        )

    def _stream_completion(
        self,
        model: str,
        body: str,
        cancel_event: threading.Event,
    ) -> Iterator[str]:
        api_key = self.service.config.openrouter_api_key
        if not api_key:
            raise RuntimeError(
                "OPENROUTER_API_KEY is not configured — set it in the environment"
            )
        payload = {
            "model": model,
            "stream": True,
            "messages": [
                {
                    "role": "system",
                    "content": (
                        "You are a research engineering assistant inside Synth Desktop. "
                        "Be concise, concrete, and prefer Trace V5 / container / visual "
                        "workflows when relevant."
                    ),
                },
                {"role": "user", "content": body},
            ],
        }
        request = urllib.request.Request(
            "https://openrouter.ai/api/v1/chat/completions",
            data=json.dumps(payload).encode("utf-8"),
            headers={
                "Authorization": f"Bearer {api_key}",
                "Content-Type": "application/json",
                "HTTP-Referer": "https://usesynth.ai",
                "X-Title": "Synth Desktop",
            },
            method="POST",
        )
        try:
            with urllib.request.urlopen(request, timeout=120) as response:
                for raw_line in response:
                    if cancel_event.is_set():
                        return
                    line = raw_line.decode("utf-8", errors="replace").strip()
                    if not line or not line.startswith("data:"):
                        continue
                    data = line[5:].strip()
                    if data == "[DONE]":
                        return
                    try:
                        chunk = json.loads(data)
                    except json.JSONDecodeError:
                        continue
                    choices = chunk.get("choices") or []
                    if not choices:
                        continue
                    delta = (choices[0].get("delta") or {}).get("content")
                    if delta:
                        yield delta
        except urllib.error.HTTPError as exc:
            detail = exc.read().decode("utf-8", errors="replace")[:500]
            raise RuntimeError(f"OpenRouter HTTP {exc.code}: {detail}") from exc
        except urllib.error.URLError as exc:
            raise RuntimeError(f"OpenRouter unavailable: {exc}") from exc

    @staticmethod
    def _estimate_cost(model: str, prompt_tokens: int, completion_tokens: int) -> float:
        prompt_rate, completion_rate = _MODEL_PRICES.get(model, (0.5, 2.0))
        return round(
            (prompt_tokens * prompt_rate + completion_tokens * completion_rate) / 1_000_000,
            6,
        )
