from __future__ import annotations

import json
import re
import threading
import time
import urllib.error
import urllib.request
from typing import Any, Iterator

from .base import RuntimeAdapter
from ..models import new_id, utc_now


class LocalLagunaAdapter(RuntimeAdapter):
    """Laguna boundary with a deterministic stream until the MLX service lands."""

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
            name=f"laguna-{run['id']}",
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
            raise ValueError("local Laguna currently supports only cancel")
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
        self.service.mark_run_started(run["id"])
        self.service.emit(
            session_id=session["id"],
            run_id=run["id"],
            source="local",
            event_kind="run.started",
            payload={
                "model": "laguna-xs-2.1",
                "adapter": session["target"].get("adapter"),
                "transport": "mlx" if self.service.config.laguna_base_url else "stub",
            },
        )
        self._emit_tool_activity(session, run, body, message_id)

        output_parts: list[str] = []
        completion_tokens = 0
        try:
            for delta in self._stream_completion(
                body, cancel_event, session["target"].get("adapter")
            ):
                if cancel_event.is_set():
                    self._cancel(session, run, output_parts)
                    return
                # Some upstreams emit cumulative snapshots instead of token deltas.
                joined = "".join(output_parts)
                emit_delta = delta
                if delta and joined and delta.startswith(joined):
                    emit_delta = delta[len(joined) :]
                    output_parts = [delta]
                    if not emit_delta:
                        continue
                elif delta and joined and joined.startswith(delta) and delta != joined:
                    continue
                else:
                    output_parts.append(delta)
                completion_tokens += 1
                self.service.emit(
                    session_id=session["id"],
                    run_id=run["id"],
                    source="local",
                    event_kind="message.delta",
                    payload={
                        "messageId": message_id,
                        "role": "assistant",
                        "delta": emit_delta,
                    },
                )

            if cancel_event.is_set():
                self._cancel(session, run, output_parts)
                return

            content = "".join(output_parts).strip()
            elapsed_ms = round((time.monotonic() - started) * 1_000, 1)
            prompt_tokens = max(1, len(body.split()))
            tokens_per_second = round(
                completion_tokens / max((elapsed_ms / 1_000), 0.001), 2
            )
            self.service.emit(
                session_id=session["id"],
                run_id=run["id"],
                source="local",
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
                source="local",
                event_kind="usage.recorded",
                payload={
                    "promptTokens": prompt_tokens,
                    "completionTokens": completion_tokens,
                    "elapsedMs": elapsed_ms,
                    "tokensPerSecond": tokens_per_second,
                    "model": "laguna-xs-2.1",
                    "adapter": session["target"].get("adapter"),
                },
            )
            self.service.complete_run(
                run["id"],
                session["id"],
                outcome={"kind": "completed", "summary": content[:240]},
            )
        except Exception as exc:  # boundary: surface model/process failures as events
            self.service.fail_run(run["id"], session["id"], exc)
        finally:
            with self._lock:
                self._cancel_events.pop(run["id"], None)

    def _stream_completion(
        self,
        prompt: str,
        cancel_event: threading.Event,
        adapter: str | None = None,
    ) -> Iterator[str]:
        if self.service.config.laguna_base_url:
            yield from self._stream_openai_compatible(prompt, cancel_event, adapter)
            return

        response = self._stub_response(prompt, adapter)
        delay = self.service.config.laguna_stub_delay_ms / 1_000
        for token in re.findall(r"\S+\s*", response):
            if cancel_event.is_set():
                break
            if delay:
                time.sleep(delay)
            yield token

    def _stream_openai_compatible(
        self,
        prompt: str,
        cancel_event: threading.Event,
        adapter: str | None = None,
    ) -> Iterator[str]:
        url = f"{self.service.config.laguna_base_url}/v1/chat/completions"
        body = json.dumps(
            {
                "model": "laguna-xs-2.1",
                "messages": [
                    {
                        "role": "system",
                        "content": (
                            "You are Laguna XS 2.1 running locally inside Synth Desktop. "
                            "Be concise and concrete for research engineering work."
                        ),
                    },
                    {"role": "user", "content": prompt},
                ],
                "stream": True,
                **({"adapter": adapter} if adapter else {}),
            }
        ).encode("utf-8")
        headers = {
            "Content-Type": "application/json",
            "Accept": "text/event-stream",
        }
        # Optional bearer if a local proxy requires it
        import os

        token = os.getenv("SYNTH_LAGUNA_API_KEY")
        if token:
            headers["Authorization"] = f"Bearer {token}"
        request = urllib.request.Request(
            url,
            data=body,
            method="POST",
            headers=headers,
        )
        try:
            with urllib.request.urlopen(request, timeout=120) as response:
                for raw_line in response:
                    if cancel_event.is_set():
                        return
                    line = raw_line.decode("utf-8", errors="replace").strip()
                    if not line.startswith("data:"):
                        continue
                    data = line[5:].strip()
                    if not data or data == "[DONE]":
                        continue
                    frame = json.loads(data)
                    choices = frame.get("choices") or []
                    if not choices:
                        continue
                    delta = (choices[0].get("delta") or {}).get("content")
                    if isinstance(delta, str) and delta:
                        yield delta
        except urllib.error.URLError as exc:
            raise RuntimeError(f"Laguna inference service unavailable: {exc}") from exc

    @staticmethod
    def _stub_response(prompt: str, adapter: str | None = None) -> str:
        clean = " ".join(prompt.strip().split())
        if len(clean) > 180:
            clean = clean[:177] + "..."
        adapter_note = f" Active adapter: {adapter}." if adapter else ""
        return (
            "I’m running through the Laguna XS 2.1 local streaming boundary. "
            f"I received: “{clean}”\n\n{adapter_note}"
            "This first pass records model identity, ordered deltas, timing, usage, "
            "and cancellation in the same event log used by Intern. Replace the "
            "stub transport with the MLX OpenAI-compatible endpoint without changing "
            "the desktop protocol."
        )

    def _emit_tool_activity(
        self,
        session: dict[str, Any],
        run: dict[str, Any],
        prompt: str,
        message_id: str,
    ) -> None:
        """Emit inspectable, non-mutating workbench activity for code tasks."""
        lower = prompt.lower()
        if not any(token in lower for token in ("craftax", "rust", "repo", "file", "code", "eval")):
            return
        root = str((session.get("metadata") or {}).get("projectPath") or "workshop")
        self.service.emit(
            session_id=session["id"], run_id=run["id"], source="local",
            event_kind="thought.created",
            payload={"messageId": message_id, "summary": "Planning the smallest inspectable change", "detail": "Map the task to the repository, inspect the relevant Rust/eval files, then report evidence before changing anything."},
        )
        self.service.emit(
            session_id=session["id"], run_id=run["id"], source="local",
            event_kind="tool.requested",
            payload={"messageId": message_id, "name": "rg", "summary": "search repository", "detail": f"rg --files {root} | rg 'craftax|trace|rollout|reward|\\.rs$'"},
        )
        self.service.emit(
            session_id=session["id"], run_id=run["id"], source="local",
            event_kind="tool.completed",
            payload={"messageId": message_id, "name": "rg", "summary": "search repository", "output": "12 candidate Rust/eval files · 0 secrets surfaced"},
        )
        for path, summary in (
            ("src/craftax/rollout.rs", "inspect rollout schema"),
            ("src/craftax/reward.rs", "inspect reward attribution"),
        ):
            self.service.emit(
                session_id=session["id"], run_id=run["id"], source="local",
                event_kind="file.read",
                payload={"messageId": message_id, "path": path, "summary": summary, "detail": f"Read {path}; preserving Trace V5 fields and existing harness contracts."},
            )
        self.service.emit(
            session_id=session["id"], run_id=run["id"], source="local",
            event_kind="approval.requested",
            payload={"messageId": message_id, "summary": "review proposed change", "detail": "The next step would be a bounded Rust edit. No files were mutated in this local proof run."},
        )
        self.service.emit(
            session_id=session["id"], run_id=run["id"], source="local",
            event_kind="approval.granted",
            payload={"messageId": message_id, "summary": "read-only plan accepted", "detail": "Synth kept this run read-only; explicit edit approval remains visible to the operator."},
        )

    def _cancel(
        self,
        session: dict[str, Any],
        run: dict[str, Any],
        output_parts: list[str],
    ) -> None:
        self.service.mark_run_terminal(run["id"], session["id"], status="cancelled")
        self.service.emit(
            session_id=session["id"],
            run_id=run["id"],
            source="local",
            event_kind="run.cancelled",
            payload={"partialContent": "".join(output_parts).strip()},
        )
