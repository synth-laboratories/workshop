from __future__ import annotations

import threading
import time
from typing import Any

from .base import RuntimeAdapter
from ..intern_client import InternHttpClient, InternHttpError
from ..models import new_id, utc_now


class InternAdapter(RuntimeAdapter):
    """Maps the existing Sync/Async mailbox planes into desktop RuntimeEvents."""

    def __init__(self, service: "Any") -> None:
        super().__init__(service)
        self._client = (
            InternHttpClient(
                base_url=service.config.backend_url,
                api_key=service.config.synth_api_key,
            )
            if service.config.synth_api_key and not service.config.intern_demo
            else None
        )
        self._lock = threading.Lock()
        self._poller_stops: dict[str, threading.Event] = {}
        self._demo_cancel: dict[str, threading.Event] = {}
        self._demo_pause: dict[str, threading.Event] = {}

    @property
    def mode(self) -> str:
        return self.service.config.intern_mode

    def prepare_session(self, session: dict[str, Any]) -> dict[str, Any]:
        if self.mode == "unconfigured":
            self.service.store.update_session(
                session["id"], status="configuration_required"
            )
            self.service.emit(
                session_id=session["id"],
                source="system",
                event_kind="runtime.configuration_required",
                payload={
                    "target": "intern",
                    "message": "Set SYNTH_API_KEY or enable SYNTH_INTERN_DEMO=1.",
                },
            )
            return self.service.store.get_session(session["id"])

        if self.mode == "demo":
            remote_id = (
                f"demo-sync-{session['id'][-8:]}"
                if session["target"]["mode"] == "sync"
                else "demo-async-org-singleton"
            )
            metadata = dict(session.get("metadata") or {})
            metadata.update(
                {
                    "internTransport": "demo",
                    "leaveSafe": session["target"]["mode"] == "async",
                    "phase": "ready",
                }
            )
            prepared = self.service.store.update_session(
                session["id"],
                remote_id=remote_id,
                state_generation=0,
                status="ready",
                metadata=metadata,
            )
            self.service.emit(
                session_id=session["id"],
                source="system",
                event_kind="intern.demo.connected",
                payload={
                    "runtimeKind": session["target"]["mode"],
                    "runtimeId": remote_id,
                    "leaveSafe": session["target"]["mode"] == "async",
                },
            )
            return prepared

        assert self._client is not None
        try:
            if session["target"]["mode"] == "sync":
                projection = self._client.create_sync(
                    idempotency_key=f"desktop:{session['id']}:create",
                    metadata={"desktop_session_id": session["id"]},
                )
                remote_id = _first_text(projection, "sync_session_id", "runtime_id", "id")
            else:
                projection = self._client.ensure_async(
                    idempotency_key=f"desktop:{session['id']}:ensure",
                    metadata={"desktop_session_id": session["id"]},
                )
                remote_id = _first_text(
                    projection,
                    "async_runtime_id",
                    "async_assignment_id",
                    "runtime_id",
                    "id",
                )
            if not remote_id:
                raise RuntimeError("Intern projection did not include a runtime id")
            metadata = self._projection_metadata(projection)
            metadata["internTransport"] = "remote"
            prepared = self.service.store.update_session(
                session["id"],
                remote_id=remote_id,
                state_generation=_int_value(projection.get("state_generation"), 0),
                status=self._session_status(projection),
                metadata=metadata,
            )
            self.service.emit(
                session_id=session["id"],
                source="intern",
                event_kind="intern.projection",
                payload=projection,
            )
            self._ensure_poller(prepared)
            return prepared
        except Exception as exc:
            self.service.store.update_session(session["id"], status="failed")
            self.service.emit(
                session_id=session["id"],
                source="system",
                event_kind="runtime.connection_error",
                payload={"message": str(exc), "operation": "prepare_intern"},
            )
            return self.service.store.get_session(session["id"])

    def resume_existing(self, session: dict[str, Any]) -> None:
        if session["target"].get("kind") != "intern":
            return
        if self.mode == "remote" and session.get("remoteId"):
            self._ensure_poller(session)
            return
        if self.mode == "demo":
            demo_run = (session.get("metadata") or {}).get("demoRun")
            active_run_id = session.get("activeRunId")
            if isinstance(demo_run, dict) and active_run_id:
                try:
                    run = self.service.store.get_run(active_run_id)
                except KeyError:
                    return
                if run["status"] in {"queued", "starting", "running", "waiting_for_input"}:
                    self._start_demo(
                        session,
                        run,
                        str(demo_run.get("body") or "Continue the delegated task."),
                        start_step=_int_value(demo_run.get("step"), 0),
                    )

    def send_message(self, session: dict[str, Any], run: dict[str, Any], body: str) -> None:
        if self.mode == "unconfigured":
            self.service.fail_run(
                run["id"],
                session["id"],
                RuntimeError("Synth Intern is not configured"),
            )
            return
        if self.mode == "demo":
            self._start_demo(session, run, body, start_step=0)
            return
        thread = threading.Thread(
            target=self._send_remote,
            name=f"intern-send-{run['id']}",
            args=(session, run, body),
            daemon=True,
        )
        thread.start()

    def control(
        self,
        session: dict[str, Any],
        kind: str,
        payload: dict[str, Any],
    ) -> dict[str, Any]:
        if self.mode == "demo":
            return self._control_demo(session, kind, payload)
        if self.mode == "unconfigured":
            raise RuntimeError("Synth Intern is not configured")
        return self._control_remote(session, kind, payload)

    def _send_remote(
        self,
        session: dict[str, Any],
        run: dict[str, Any],
        body: str,
    ) -> None:
        assert self._client is not None
        command_id = new_id("cmd")
        idempotency_key = new_id("idem")
        generation = _int_value(session.get("stateGeneration"), 0)
        self.service.mark_run_started(run["id"])
        self.service.emit(
            session_id=session["id"],
            run_id=run["id"],
            source="intern",
            event_kind="run.started",
            payload={
                "runtimeKind": session["target"]["mode"],
                "runtimeId": session.get("remoteId"),
                "transport": "mailbox",
            },
            command_id=command_id,
        )
        try:
            if session["target"]["mode"] == "sync":
                remote_id = session.get("remoteId")
                if not remote_id:
                    raise RuntimeError("Sync session is missing remoteId")
                receipt = self._client.send_sync(
                    remote_id,
                    command_id=command_id,
                    idempotency_key=idempotency_key,
                    expected_generation=generation,
                    body=body,
                )
            else:
                receipt = self._client.send_async(
                    command_id=command_id,
                    idempotency_key=idempotency_key,
                    expected_generation=generation,
                    kind="message",
                    body=body,
                    context={"desktop_session_id": session["id"]},
                )
            self._record_receipt(session, run, receipt, command_id)
            self._ensure_poller(self.service.store.get_session(session["id"]))
        except Exception as exc:
            self.service.fail_run(run["id"], session["id"], exc)

    def _record_receipt(
        self,
        session: dict[str, Any],
        run: dict[str, Any] | None,
        receipt: dict[str, Any],
        command_id: str,
    ) -> None:
        generation = _int_value(
            receipt.get("state_generation"),
            _int_value(session.get("stateGeneration"), 0),
        )
        self.service.store.update_session(session["id"], state_generation=generation)
        self.service.emit(
            session_id=session["id"],
            run_id=run["id"] if run else session.get("activeRunId"),
            source="intern",
            event_kind="command.receipt",
            payload=receipt,
            command_id=command_id,
        )
        receipt_status = str(receipt.get("status") or "")
        if run is not None and receipt_status in {"conflict", "refused", "superseded"}:
            outcome = {
                "kind": "command_rejected",
                "receiptStatus": receipt_status,
                "decisionCode": receipt.get("decision_code"),
            }
            self.service.mark_run_terminal(
                run["id"],
                session["id"],
                status="failed",
                outcome=outcome,
                session_status="ready",
            )
            self.service.emit(
                session_id=session["id"],
                run_id=run["id"],
                source="intern",
                event_kind="run.failed",
                payload=outcome,
                command_id=command_id,
            )

    def _ensure_poller(self, session: dict[str, Any]) -> None:
        if self.mode != "remote" or not session.get("remoteId"):
            return
        with self._lock:
            current = self._poller_stops.get(session["id"])
            if current is not None and not current.is_set():
                return
            stop_event = threading.Event()
            self._poller_stops[session["id"]] = stop_event
        thread = threading.Thread(
            target=self._poll_loop,
            name=f"intern-tail-{session['id']}",
            args=(session["id"], stop_event),
            daemon=True,
        )
        thread.start()

    def _poll_loop(self, session_id: str, stop_event: threading.Event) -> None:
        assert self._client is not None
        projection_at = 0.0
        error_backoff = 1.0
        while not stop_event.is_set():
            try:
                session = self.service.store.get_session(session_id)
                remote_id = session.get("remoteId")
                if not remote_id:
                    return
                cursor = self.service.store.get_cursor(session_id, "intern")
                if session["target"]["mode"] == "sync":
                    events = self._client.sync_events(
                        remote_id, after_sequence=cursor, limit=500
                    )
                else:
                    events = self._client.async_events(after_sequence=cursor, limit=500)
                for event in events:
                    self._append_remote_event(session, event)
                now = time.monotonic()
                if events or now >= projection_at:
                    projection = (
                        self._client.get_sync(remote_id)
                        if session["target"]["mode"] == "sync"
                        else self._client.get_async()
                    )
                    self._apply_projection(session_id, projection)
                    projection_at = now + 4.0
                error_backoff = 1.0
                stop_event.wait(0.35 if events else 0.9)
            except KeyError:
                return
            except Exception as exc:
                self.service.emit(
                    session_id=session_id,
                    source="system",
                    event_kind="runtime.connection_error",
                    payload={"message": str(exc), "operation": "tail_intern"},
                )
                stop_event.wait(error_backoff)
                error_backoff = min(error_backoff * 2, 15.0)

    def _append_remote_event(
        self,
        session: dict[str, Any],
        remote_event: dict[str, Any],
    ) -> None:
        remote_sequence = _int_value(remote_event.get("sequence"), 0)
        if remote_sequence <= 0:
            return
        generation = _int_value(
            remote_event.get("state_generation"),
            _int_value(session.get("stateGeneration"), 0),
        )
        if generation:
            self.service.store.update_session(
                session["id"], state_generation=generation
            )
        payload = remote_event.get("payload")
        if not isinstance(payload, dict):
            payload = {"value": payload}
        payload = dict(payload)
        payload.setdefault("intern", {})
        if isinstance(payload["intern"], dict):
            payload["intern"].update(
                {
                    "eventId": remote_event.get("event_id"),
                    "runtimeKind": remote_event.get("runtime_kind"),
                    "runtimeId": remote_event.get("runtime_id"),
                    "stateGeneration": generation,
                }
            )
        event_kind = str(remote_event.get("event_kind") or "intern.event")
        self.service.emit(
            session_id=session["id"],
            run_id=session.get("activeRunId"),
            source="intern",
            remote_sequence=remote_sequence,
            event_kind=event_kind,
            payload=payload,
            command_id=_optional_text(remote_event.get("command_id")),
            created_at=_optional_text(remote_event.get("created_at")),
        )

    def _apply_projection(self, session_id: str, projection: dict[str, Any]) -> None:
        session = self.service.store.get_session(session_id)
        status = self._session_status(projection)
        generation = _int_value(
            projection.get("state_generation"),
            _int_value(session.get("stateGeneration"), 0),
        )
        metadata = dict(session.get("metadata") or {})
        metadata.update(self._projection_metadata(projection))
        self.service.store.update_session(
            session_id,
            status=status,
            state_generation=generation,
            metadata=metadata,
        )

        active_run_id = session.get("activeRunId")
        if not active_run_id:
            return
        try:
            run = self.service.store.get_run(active_run_id)
        except KeyError:
            return
        if run["status"] in {"completed", "failed", "cancelled"}:
            return
        raw_status = str(projection.get("status") or "").lower()
        if raw_status in {"failed"}:
            self.service.emit(
                session_id=session_id,
                run_id=active_run_id,
                source="intern",
                event_kind="run.failed",
                payload={"failureCode": projection.get("failure_code")},
            )
            self.service.mark_run_terminal(
                active_run_id,
                session_id,
                status="failed",
                outcome={"failureCode": projection.get("failure_code")},
                session_status="failed",
            )
        elif raw_status in {"cancelled", "canceled"}:
            self.service.emit(
                session_id=session_id,
                run_id=active_run_id,
                source="intern",
                event_kind="run.cancelled",
                payload={"outcome": projection.get("outcome") or {"kind": raw_status}},
            )
            self.service.mark_run_terminal(
                active_run_id,
                session_id,
                status="cancelled",
                outcome=projection.get("outcome") or {"kind": raw_status},
                session_status="cancelled",
            )
        elif raw_status in {"closed", "completed"}:
            self.service.complete_run(
                active_run_id,
                session_id,
                outcome=projection.get("outcome") or {"kind": raw_status},
            )
            self.service.store.update_session(session_id, status="completed")
        elif session["target"]["mode"] == "sync" and raw_status == "ready":
            if self.service.store.get_cursor(session_id, "intern") > 0:
                self.service.complete_run(
                    active_run_id,
                    session_id,
                    outcome={"kind": "turn_completed"},
                )
        elif session["target"]["mode"] == "async" and raw_status == "sleeping":
            self.service.complete_run(
                active_run_id,
                session_id,
                outcome={"kind": "instruction_applied", "phase": "sleeping"},
            )

        checkpoint = projection.get("checkpoint")
        if checkpoint is not None:
            self.service.store.update_run(active_run_id, checkpoint=checkpoint)

    def _control_remote(
        self,
        session: dict[str, Any],
        kind: str,
        payload: dict[str, Any],
    ) -> dict[str, Any]:
        assert self._client is not None
        command_id = new_id("cmd")
        idempotency_key = new_id("idem")
        generation = _int_value(session.get("stateGeneration"), 0)
        mode = session["target"]["mode"]
        if mode == "sync":
            if kind not in {"pause", "resume", "close"}:
                raise ValueError("Sync supports pause, resume, or close")
            remote_id = session.get("remoteId")
            if not remote_id:
                raise RuntimeError("Sync session is missing remoteId")
            command_payload: dict[str, Any]
            if kind == "pause":
                command_payload = {"reason": str(payload.get("reason") or "Operator pause")}
            elif kind == "close":
                command_payload = {
                    "outcome": str(payload.get("outcome") or "stopped"),
                    "reason": str(payload.get("reason") or "Closed from Synth Desktop"),
                }
            else:
                command_payload = {}
            receipt = self._client.request(
                "POST",
                f"/smr/research-intern/sync-sessions/{remote_id}/commands",
                body={
                    "command_id": command_id,
                    "idempotency_key": idempotency_key,
                    "expected_generation": generation,
                    "command_kind": kind,
                    "payload": command_payload,
                    "execution_mode": "standard",
                    "mode": "sync",
                    "evidence_refs": [],
                },
            )
        else:
            if kind not in {"pause", "resume", "cancel", "request_checkpoint"}:
                raise ValueError(
                    "Async supports pause, resume, cancel, or request_checkpoint"
                )
            command_payload = dict(payload)
            if kind in {"pause", "cancel"}:
                command_payload.setdefault("reason", "Requested from Synth Desktop")
            receipt = self._client.command_async(
                command_id=command_id,
                idempotency_key=idempotency_key,
                expected_generation=generation,
                kind=kind,
                payload=command_payload,
            )
        self._record_receipt(session, None, receipt, command_id)
        return {"accepted": True, "receipt": receipt}

    def _start_demo(
        self,
        session: dict[str, Any],
        run: dict[str, Any],
        body: str,
        *,
        start_step: int,
    ) -> None:
        cancel = threading.Event()
        pause = threading.Event()
        with self._lock:
            self._demo_cancel[run["id"]] = cancel
            self._demo_pause[run["id"]] = pause
        metadata = dict(session.get("metadata") or {})
        metadata["demoRun"] = {
            "body": body,
            "step": start_step,
            "mode": session["target"]["mode"],
        }
        self.service.store.update_session(
            session["id"], status="running", metadata=metadata
        )
        thread = threading.Thread(
            target=self._run_demo,
            name=f"intern-demo-{run['id']}",
            args=(session["id"], run["id"], body, start_step, cancel, pause),
            daemon=True,
        )
        thread.start()

    def _run_demo(
        self,
        session_id: str,
        run_id: str,
        body: str,
        start_step: int,
        cancel: threading.Event,
        pause: threading.Event,
    ) -> None:
        try:
            session = self.service.store.get_session(session_id)
            mode = session["target"]["mode"]
            self.service.mark_run_started(run_id)
            command_id = new_id("cmd")
            receipt = {
                "schema_version": "smr.intern-runtime-command-receipt.v1",
                "command_id": command_id,
                "runtime_kind": mode,
                "runtime_id": session.get("remoteId"),
                "status": "applied",
                "previous_generation": _int_value(session.get("stateGeneration"), 0),
                "state_generation": _int_value(session.get("stateGeneration"), 0) + 1,
                "decision_code": "desktop_demo_applied",
                "created_at": utc_now(),
                "duplicate": False,
            }
            self._record_receipt(session, self.service.store.get_run(run_id), receipt, command_id)
            self.service.emit(
                session_id=session_id,
                run_id=run_id,
                source="intern",
                event_kind="run.started",
                payload={
                    "runtimeKind": mode,
                    "runtimeId": session.get("remoteId"),
                    "transport": "demo-mailbox",
                    "leaveSafe": mode == "async",
                },
                command_id=command_id,
            )

            steps = self._demo_steps(mode, body, command_id)
            remote_sequence = self.service.store.get_cursor(session_id, "intern")
            for index, (delay, event_kind, payload) in enumerate(steps):
                if index < start_step:
                    continue
                while pause.is_set() and not cancel.is_set():
                    time.sleep(0.1)
                if cancel.wait(delay):
                    self.service.mark_run_terminal(
                        run_id,
                        session_id,
                        status="cancelled",
                        outcome={"kind": "cancelled"},
                    )
                    self.service.emit(
                        session_id=session_id,
                        run_id=run_id,
                        source="intern",
                        event_kind="run.cancelled",
                        payload={"kind": "cancelled"},
                    )
                    return
                self.service.emit(
                    session_id=session_id,
                    run_id=run_id,
                    source="intern",
                    event_kind=event_kind,
                    payload=payload,
                    command_id=command_id,
                    remote_sequence=remote_sequence + 1,
                )
                remote_sequence += 1
                session = self.service.store.get_session(session_id)
                metadata = dict(session.get("metadata") or {})
                metadata["demoRun"] = {
                    "body": body,
                    "step": index + 1,
                    "mode": mode,
                }
                if event_kind == "checkpoint.created":
                    metadata["checkpoint"] = payload
                    self.service.store.update_run(run_id, checkpoint=payload)
                self.service.store.update_session(session_id, metadata=metadata)

            self.service.complete_run(
                run_id,
                session_id,
                outcome={"kind": "completed", "demo": True},
            )
            final_session = self.service.store.get_session(session_id)
            metadata = dict(final_session.get("metadata") or {})
            metadata["demoRun"] = None
            metadata["phase"] = "sleeping" if mode == "async" else "ready"
            metadata["leaveSafe"] = mode == "async"
            self.service.store.update_session(
                session_id, status="ready", metadata=metadata
            )
        except Exception as exc:
            self.service.fail_run(run_id, session_id, exc)
        finally:
            with self._lock:
                self._demo_cancel.pop(run_id, None)
                self._demo_pause.pop(run_id, None)

    @staticmethod
    def _demo_steps(
        mode: str,
        body: str,
        command_id: str,
    ) -> list[tuple[float, str, dict[str, Any]]]:
        clean = " ".join(body.split())
        if len(clean) > 180:
            clean = clean[:177] + "..."
        if mode == "sync":
            return [
                (0.25, "intern.status", {"status": "thinking", "phase": "live_turn"}),
                (
                    0.45,
                    "intern.progress",
                    {"summary": "Inspecting the task and forming a bounded execution plan."},
                ),
                (
                    0.55,
                    "agent_message",
                    {
                        "body": (
                            f"I received the live Intern task: “{clean}”\n\n"
                            "This demo follows the real generation-fenced command and cursor-replay "
                            "shape. Configure SYNTH_API_KEY to switch the same UI to the hosted mailbox."
                        ),
                        "provenance": {"transport": "demo", "command_id": command_id},
                    },
                ),
                (
                    0.2,
                    "resource_ref.created",
                    {
                        "kind": "artifact",
                        "id": f"demo-artifact-{command_id[-6:]}",
                        "title": "Intern demo result",
                    },
                ),
            ]
        return [
            (
                0.2,
                "async.assignment.accepted",
                {"leave_safe": True, "summary": "Delegation accepted; Desktop may disconnect."},
            ),
            (0.65, "async.cycle.started", {"cycle_number": 1, "phase": "executing_cycle"}),
            (
                0.8,
                "intern.progress",
                {"summary": "Gathered context and selected the next bounded action."},
            ),
            (
                0.8,
                "checkpoint.created",
                {
                    "checkpoint_id": f"cp-{command_id[-8:]}",
                    "cycle_number": 1,
                    "summary": f"Checkpoint after beginning: {clean}",
                    "leave_safe": True,
                },
            ),
            (
                0.7,
                "agent_message",
                {
                    "body": (
                        "The background Intern demo completed its first durable cycle. "
                        "The event cursor, checkpoint, and result remain available after the window closes."
                    )
                },
            ),
            (
                0.2,
                "async.sleeping",
                {"phase": "sleeping", "leave_safe": True, "next_wake_at": None},
            ),
        ]

    def _control_demo(
        self,
        session: dict[str, Any],
        kind: str,
        payload: dict[str, Any],
    ) -> dict[str, Any]:
        run_id = session.get("activeRunId")
        if kind == "request_checkpoint":
            checkpoint = {
                "checkpoint_id": new_id("cp"),
                "summary": "Operator-requested desktop demo checkpoint.",
                "created_at": utc_now(),
                "leave_safe": session["target"]["mode"] == "async",
            }
            self.service.emit(
                session_id=session["id"],
                run_id=run_id,
                source="intern",
                event_kind="checkpoint.created",
                payload=checkpoint,
            )
            if run_id:
                self.service.store.update_run(run_id, checkpoint=checkpoint)
            return {"accepted": True, "receipt": {"status": "applied"}}
        if not run_id:
            return {"accepted": False, "reason": "no_active_run"}
        with self._lock:
            cancel = self._demo_cancel.get(run_id)
            pause = self._demo_pause.get(run_id)
        if kind == "pause" and pause is not None:
            pause.set()
            self.service.store.update_session(session["id"], status="paused")
            self.service.emit(
                session_id=session["id"],
                run_id=run_id,
                source="intern",
                event_kind="runtime.paused",
                payload={"reason": payload.get("reason") or "Operator pause"},
            )
            return {"accepted": True, "receipt": {"status": "applied"}}
        if kind == "resume" and pause is not None:
            pause.clear()
            self.service.store.update_session(session["id"], status="running")
            self.service.emit(
                session_id=session["id"],
                run_id=run_id,
                source="intern",
                event_kind="runtime.resumed",
                payload={},
            )
            return {"accepted": True, "receipt": {"status": "applied"}}
        if kind in {"cancel", "close"} and cancel is not None:
            cancel.set()
            return {"accepted": True, "receipt": {"status": "applied"}}
        return {"accepted": False, "reason": "unsupported_or_inactive"}

    @staticmethod
    def _session_status(projection: dict[str, Any]) -> str:
        raw = str(projection.get("status") or "ready").lower()
        if raw in {
            "planning",
            "executing_cycle",
            "checkpointing",
            "reconciling",
            "thinking",
            "closing",
        }:
            return "running"
        if raw in {"waiting_for_operator", "waiting_for_input", "blocked"}:
            return "waiting_for_input"
        if raw in {"paused"}:
            return "paused"
        if raw in {"completed", "closed"}:
            return "completed"
        if raw in {"failed"}:
            return "failed"
        if raw in {"cancelled", "canceled"}:
            return "cancelled"
        return "ready"

    @staticmethod
    def _projection_metadata(projection: dict[str, Any]) -> dict[str, Any]:
        keys = {
            "status": "internStatus",
            "phase": "phase",
            "leave_safe": "leaveSafe",
            "next_wake_at": "nextWakeAt",
            "cycle_number": "cycleNumber",
            "checkpoint": "checkpoint",
            "spend": "spend",
            "budget": "budget",
            "blocker": "blocker",
            "evidence_readiness": "evidenceReadiness",
            "pending_interaction_id": "pendingInteractionId",
        }
        return {
            destination: projection[source]
            for source, destination in keys.items()
            if source in projection
        }


def _first_text(value: dict[str, Any], *keys: str) -> str | None:
    for key in keys:
        candidate = value.get(key)
        if isinstance(candidate, str) and candidate:
            return candidate
    return None


def _optional_text(value: Any) -> str | None:
    return value if isinstance(value, str) and value else None


def _int_value(value: Any, default: int) -> int:
    if isinstance(value, bool):
        return default
    try:
        return int(value)
    except (TypeError, ValueError):
        return default
