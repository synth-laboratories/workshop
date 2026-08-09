"""Codex coding-agent adapter using Laguna through the Responses protocol."""

from __future__ import annotations

import threading
from typing import Any

from .base import RuntimeAdapter
from ..codex import CodexAgentSession, CodexLaunchConfig, resolve_workspace
from ..models import new_id


def _item(params: Any) -> dict[str, Any]:
    if not isinstance(params, dict):
        return {}
    candidate = params.get("item")
    return candidate if isinstance(candidate, dict) else params


def _item_type(value: dict[str, Any]) -> str:
    return str(value.get("type") or value.get("kind") or "").replace("-", "_").lower()


class LocalLagunaAdapter(RuntimeAdapter):
    """Every local turn is a Codex app-server turn; there is no chat fallback."""

    def __init__(self, service: Any) -> None:
        super().__init__(service)
        self._sessions: dict[str, CodexAgentSession] = {}
        self._active_runs: dict[str, dict[str, Any]] = {}
        self._lock = threading.RLock()

    def send_message(self, session: dict[str, Any], run: dict[str, Any], body: str) -> None:
        thread = threading.Thread(
            target=self._run,
            name=f"codex-laguna-{run['id']}",
            args=(session, run, body),
            daemon=True,
        )
        thread.start()

    def control(self, session: dict[str, Any], kind: str, payload: dict[str, Any]) -> dict[str, Any]:
        if kind != "cancel":
            raise ValueError("Codex sessions currently support only cancel")
        with self._lock:
            agent = self._sessions.get(session["id"])
        if not agent or not agent.turn_id:
            return {"accepted": False, "reason": "run_not_active"}
        agent.interrupt()
        return {"accepted": True}

    def _config(self, session: dict[str, Any]) -> CodexLaunchConfig:
        if not self.service.config.laguna_base_url:
            raise RuntimeError(
                "Laguna Responses server is not configured; set SYNTH_LAGUNA_BASE_URL"
            )
        workspace = resolve_workspace(
            session_metadata=session.get("metadata"),
            workshop_root=self.service.config.workshop_root,
        )
        return CodexLaunchConfig(
            codex_home=self.service.config.data_dir / "codex" / session["id"],
            laguna_base_url=self.service.config.laguna_base_url,
            laguna_api_key="synth-desktop-laguna",
            model="poolside/Laguna-XS-2.1-NVFP4-mlx",
            workspace=workspace,
            workshop_root=self.service.config.workshop_root,
        )

    @staticmethod
    def _source(session: dict[str, Any]) -> str:
        return "remote" if session["target"]["kind"] == "remote" else "local"

    def _agent(self, session: dict[str, Any], run: dict[str, Any]) -> CodexAgentSession:
        with self._lock:
            existing = self._sessions.get(session["id"])
            if existing:
                self._active_runs[session["id"]] = run
                return existing
            metadata = session.get("metadata") or {}
            thread_id = metadata.get("codexThreadId")
            agent = CodexAgentSession(
                self._config(session),
                thread_id=thread_id if isinstance(thread_id, str) else None,
                on_notification=lambda method, params: self._event(session["id"], method, params),
            )
            self._sessions[session["id"]] = agent
            self._active_runs[session["id"]] = run
            return agent

    def _run(self, session: dict[str, Any], run: dict[str, Any], body: str) -> None:
        self.service.mark_run_started(run["id"])
        self.service.emit(
            session_id=session["id"], run_id=run["id"], source=self._source(session),
            event_kind="run.started",
            payload={"model": session["target"].get("model"), "agent": "codex-app-server", "transport": "responses"},
        )
        try:
            agent = self._agent(session, run)
            thread_id = agent.start()
            self.service.merge_session_metadata(session["id"], {
                "agentRuntime": "codex-app-server", "codexThreadId": thread_id,
                "modelTransport": "responses",
            })
            agent.run_turn(body)
            self.service.complete_run(
                run["id"], session["id"],
                outcome={"kind": "completed", "codexThreadId": agent.thread_id, "codexTurnId": agent.turn_id},
            )
        except InterruptedError:
            self.service.mark_run_terminal(
                run["id"], session["id"], status="cancelled",
                outcome={"kind": "cancelled"}, session_status="ready",
            )
            self.service.emit(
                session_id=session["id"], run_id=run["id"], source=self._source(session),
                event_kind="run.cancelled", payload={"codexTurnId": agent.turn_id},
            )
        except Exception as exc:
            with self._lock:
                failed = self._sessions.pop(session["id"], None)
            if failed:
                failed.close()
            self.service.fail_run(run["id"], session["id"], exc)
        finally:
            with self._lock:
                self._active_runs.pop(session["id"], None)

    def _event(self, session_id: str, method: str, params: Any) -> None:
        with self._lock:
            run = self._active_runs.get(session_id)
        if not run:
            return
        payload = params if isinstance(params, dict) else {"value": params}
        item = _item(payload)
        kind = _item_type(item)
        event_kind = method
        normalized = {"codexMethod": method, "raw": payload}
        if method == "item/agentMessage/delta":
            event_kind = "message.delta"
            normalized.update({"messageId": item.get("id") or new_id("msg"), "role": "assistant", "delta": payload.get("delta") or item.get("delta") or ""})
        elif method == "item/completed" and kind in {"agentmessage", "agent_message", "message"}:
            event_kind = "message.completed"
            normalized.update({"messageId": item.get("id") or new_id("msg"), "role": "assistant", "content": item.get("text") or item.get("content") or ""})
        elif kind in {"reasoning", "analysis"}:
            event_kind = "thought.created"
            normalized.update({"summary": item.get("summary") or item.get("text") or "Codex reasoning"})
        elif kind in {"commandexecution", "command_execution"}:
            event_kind = "tool.completed" if method.endswith("/completed") else "tool.requested"
            normalized.update({"name": "shell", "summary": item.get("command") or "command execution", "output": item.get("aggregatedOutput") or item.get("output")})
        elif kind in {"filechange", "file_change"}:
            event_kind = "file.changed"
            normalized.update({"path": item.get("path"), "summary": item.get("status") or "Codex file change"})
        elif "requestApproval" in method:
            event_kind = "approval.requested"
        self.service.emit(
            session_id=session_id, run_id=run["id"], source=self._source(self.service.get_session(session_id)),
            event_kind=event_kind, payload=normalized,
        )
