from __future__ import annotations

import json
import threading
from pathlib import Path
from typing import Any

from .adapters import InternAdapter, LocalLagunaAdapter, OpenRouterAdapter
from .broker import EventBroker
from .config import RuntimeConfig
from .inventory import InventoryStore
from .models import EventInput, JSON, new_id, utc_now, validate_target
from .store import RuntimeStore
from .visual_catalog import list_visual_templates, resolve_visual_template


class RuntimeService:
    """Application service: local authority plus exact adapters to execution targets."""

    protocol_version = "synth.desktop-runtime.v1"

    def __init__(self, config: RuntimeConfig) -> None:
        self.config = config
        self.runtime_id = new_id("runtime")
        self.started_at = utc_now()
        self.store = RuntimeStore(config.database_path)
        self.inventory = InventoryStore(config.database_path)
        self.broker = EventBroker()
        self.local_adapter = LocalLagunaAdapter(self)
        self.remote_adapter = OpenRouterAdapter(self)
        self.intern_adapter = InternAdapter(self)
        self._resume_lock = threading.Lock()
        self._seed_inventory()
        self._resume_existing_sessions()

    def _seed_inventory(self) -> None:
        try:
            self.inventory.seed_demo_inventory(self.config.visuals_root)
        except Exception as exc:  # noqa: BLE001 — first-run seed must not block boot
            print(f"[runtime] inventory seed skipped: {exc}", flush=True)

    def health(self) -> JSON:
        counts = self.inventory.counts()
        return {
            "status": "ok",
            "protocolVersion": self.protocol_version,
            "runtimeId": self.runtime_id,
            "startedAt": self.started_at,
            "intern": {
                "mode": self.config.intern_mode,
                "backendUrl": (
                    self.config.backend_url if self.config.intern_mode == "remote" else None
                ),
            },
            "local": {
                "model": "laguna-xs-2.1",
                "mode": "mlx" if self.config.laguna_base_url else "stub",
                "modelPath": self.config.laguna_model_path,
            },
            "openrouter": {
                "mode": self.config.openrouter_mode,
                "models": [
                    "moonshotai/kimi-k2.5",
                    "poolside/laguna-s-2.1",
                    "openai/gpt-5.4-nano",
                ],
            },
            "inventory": counts,
            "dataStore": {
                "path": str(self.store.path),
                **self.store.counts(),
                "usage": len(self.inventory.list_usage(limit=1000)),
            },
        }

    def create_session(
        self,
        target_value: object,
        *,
        title: str | None = None,
        project_id: str | None = None,
        metadata: JSON | None = None,
    ) -> JSON:
        target = validate_target(target_value)
        if target["kind"] == "intern" and target["mode"] == "async":
            for existing in self.store.list_sessions():
                existing_target = existing.get("target") or {}
                if (
                    existing_target.get("kind") == "intern"
                    and existing_target.get("mode") == "async"
                ):
                    return existing

        if target["kind"] == "remote" and not self.config.openrouter_api_key:
            session = self.store.create_session(
                target, title=title, project_id=project_id, metadata=metadata
            )
            return self.store.update_session(
                session["id"], status="configuration_required"
            )

        session = self.store.create_session(
            target, title=title, project_id=project_id, metadata=metadata
        )
        self.emit(
            session_id=session["id"],
            source="system",
            event_kind="session.created",
            payload={"target": target, "title": session["title"]},
        )
        if target["kind"] == "intern":
            session = self.intern_adapter.prepare_session(session)
        return session

    def list_sessions(self) -> list[JSON]:
        return self.store.list_sessions()

    def list_projects(self) -> list[JSON]:
        return self.store.list_projects()

    def get_project(self, project_id: str) -> JSON:
        return self.store.get_project(project_id)

    def create_project(self, payload: JSON) -> JSON:
        return self.store.create_project(
            payload.get("path"),
            name=payload.get("name"),
            vcs=payload.get("vcs"),
            metadata=payload.get("metadata") if isinstance(payload.get("metadata"), dict) else None,
        )

    def delete_project(self, project_id: str) -> bool:
        return self.store.delete_project(project_id)

    def get_session(self, session_id: str) -> JSON:
        return self.store.get_session(session_id)

    def delete_session(self, session_id: str) -> bool:
        return self.store.delete_session(session_id)

    def list_runs(self, session_id: str) -> list[JSON]:
        return self.store.list_runs(session_id)

    def _adapter_for(self, session: JSON):
        kind = session["target"]["kind"]
        if kind == "local":
            return self.local_adapter
        if kind == "remote":
            return self.remote_adapter
        return self.intern_adapter

    def _source_for(self, session: JSON) -> str:
        kind = session["target"]["kind"]
        if kind == "local":
            return "local"
        if kind == "remote":
            return "remote"
        return "intern"

    def send_message(self, session_id: str, body_value: object) -> JSON:
        if not isinstance(body_value, str) or not body_value.strip():
            raise ValueError("message body is required")
        body = body_value.strip()
        if len(body) > 20_000:
            raise ValueError("message body exceeds 20,000 characters")
        session = self.store.get_session(session_id)
        if session["status"] == "configuration_required":
            raise RuntimeError("the selected execution target is not configured")
        active_run_id = session.get("activeRunId")
        if active_run_id:
            try:
                active_run = self.store.get_run(active_run_id)
            except KeyError:
                active_run = None
            if active_run and active_run["status"] in {
                "queued",
                "starting",
                "running",
                "waiting_for_input",
            }:
                raise RuntimeError("the selected session already has an active run")

        source = self._source_for(session)
        run = self.store.create_run(
            session_id,
            metadata={
                "sourceCursorAtStart": self.store.get_cursor(session_id, source),
                "protocolVersion": self.protocol_version,
            },
        )
        message_id = new_id("msg")
        self.emit(
            session_id=session_id,
            run_id=run["id"],
            source=source,
            event_kind="message.created",
            payload={"messageId": message_id, "role": "user", "content": body},
        )
        session = self.store.get_session(session_id)
        self._adapter_for(session).send_message(session, run, body)
        return {"runId": run["id"]}

    def control(
        self,
        session_id: str,
        kind_value: object,
        payload_value: object,
    ) -> JSON:
        if not isinstance(kind_value, str) or not kind_value:
            raise ValueError("command kind is required")
        payload = payload_value if isinstance(payload_value, dict) else {}
        session = self.store.get_session(session_id)
        if kind_value in {"approve", "reject", "set_approval_mode"}:
            event_kind = {
                "approve": "approval.granted",
                "reject": "approval.rejected",
                "set_approval_mode": "approval.mode_changed",
            }[kind_value]
            self.emit(
                session_id=session_id,
                source="system",
                event_kind=event_kind,
                payload={**payload, "mode": payload.get("mode", "always_ask")},
            )
            return {"accepted": True, "eventKind": event_kind}
        return self._adapter_for(session).control(session, kind_value, payload)

    def events(self, session_id: str, *, after_sequence: int, limit: int) -> JSON:
        return self.store.list_events(
            session_id, after_sequence=after_sequence, limit=limit
        )

    def emit(
        self,
        *,
        session_id: str,
        event_kind: str,
        payload: JSON,
        source: str,
        run_id: str | None = None,
        command_id: str | None = None,
        remote_sequence: int | None = None,
        created_at: str | None = None,
    ) -> JSON:
        event, inserted = self.store.append_event(
            EventInput(
                session_id=session_id,
                run_id=run_id,
                event_kind=event_kind,
                payload=payload,
                source=source,  # type: ignore[arg-type]
                command_id=command_id,
                remote_sequence=remote_sequence,
                created_at=created_at,
            )
        )
        if inserted:
            self.broker.notify(session_id)
            if event_kind in {"visual.created", "resource_ref.created"}:
                pass
        return event

    def mark_run_started(self, run_id: str) -> JSON:
        return self.store.update_run(run_id, status="running", started_at=utc_now())

    def complete_run(self, run_id: str, session_id: str, *, outcome: Any) -> None:
        try:
            run = self.store.get_run(run_id)
        except KeyError:
            return
        if run["status"] in {"completed", "failed", "cancelled"}:
            return
        self.mark_run_terminal(
            run_id,
            session_id,
            status="completed",
            outcome=outcome,
            session_status="ready",
        )
        self.emit(
            session_id=session_id,
            run_id=run_id,
            source="system",
            event_kind="run.completed",
            payload={"outcome": outcome},
        )

    def fail_run(self, run_id: str, session_id: str, error: Exception) -> None:
        message = str(error) or error.__class__.__name__
        self.emit(
            session_id=session_id,
            run_id=run_id,
            source="system",
            event_kind="runtime.error",
            payload={"message": message, "errorType": error.__class__.__name__},
        )
        self.emit(
            session_id=session_id,
            run_id=run_id,
            source="system",
            event_kind="run.failed",
            payload={"message": message},
        )
        self.mark_run_terminal(
            run_id,
            session_id,
            status="failed",
            outcome={"kind": "failed", "message": message},
            session_status="failed",
        )

    def mark_run_terminal(
        self,
        run_id: str,
        session_id: str,
        *,
        status: str,
        outcome: Any = None,
        session_status: str | None = None,
    ) -> None:
        completed_at = utc_now()
        self.store.update_run(
            run_id,
            status=status,
            outcome=outcome,
            completed_at=completed_at,
        )
        if session_status is None:
            session_status = "failed" if status == "failed" else "ready"
        self.store.update_session(
            session_id,
            status=session_status,
            active_run_id=None,
        )

    def merge_session_metadata(self, session_id: str, updates: JSON) -> JSON:
        session = self.store.get_session(session_id)
        metadata = dict(session.get("metadata") or {})
        metadata.update(updates)
        return self.store.update_session(session_id, metadata=metadata)

    def record_usage(self, **kwargs: Any) -> JSON:
        return self.inventory.record_usage(**kwargs)

    # ── Inventory API surface ───────────────────────────────────────────────

    def list_containers(self) -> list[JSON]:
        return self.inventory.list_containers()

    def upsert_container(self, payload: JSON) -> JSON:
        return self.inventory.upsert_container(payload)

    def get_container(self, container_id: str) -> JSON:
        return self.inventory.get_container(container_id)

    def delete_container(self, container_id: str) -> bool:
        return self.inventory.delete_container(container_id)

    def probe_container(self, container_id: str) -> JSON:
        container = self.inventory.get_container(container_id)
        base_url = container.get("baseUrl")
        health: JSON = {"ok": False}
        status = "unhealthy"
        if base_url:
            try:
                import urllib.request

                with urllib.request.urlopen(f"{base_url.rstrip('/')}/health", timeout=2) as resp:
                    health = json.loads(resp.read().decode("utf-8"))
                    status = "ready" if resp.status == 200 else "unhealthy"
            except Exception as exc:  # noqa: BLE001
                health = {"ok": False, "error": str(exc)}
                status = "unhealthy"
        else:
            # Cloud pointer without local URL — treat metadata as authority.
            status = container.get("status") or "ready"
            health = container.get("health") or {"ok": status == "ready"}
        return self.inventory.upsert_container(
            {
                **container,
                "status": status,
                "health": health,
            }
        )

    def list_traces(self) -> list[JSON]:
        return self.inventory.list_traces()

    def ingest_trace(self, payload: JSON) -> JSON:
        return self.inventory.ingest_trace(
            title=str(payload.get("title") or "Trace V5"),
            payload=payload.get("payload") if isinstance(payload.get("payload"), dict) else None,
            path=payload.get("path"),
            source=str(payload.get("source") or "local"),
            container_id=payload.get("containerId"),
            session_id=payload.get("sessionId"),
            run_id=payload.get("runId"),
            reward=payload.get("reward"),
            metrics=payload.get("metrics") if isinstance(payload.get("metrics"), list) else None,
            metadata=payload.get("metadata") if isinstance(payload.get("metadata"), dict) else None,
        )

    def get_trace(self, trace_id: str) -> JSON:
        return self.inventory.get_trace(trace_id)

    def list_visual_templates(self) -> list[JSON]:
        return list_visual_templates(self.config.visuals_root)

    def resolve_visual_template(self, template_id: str) -> JSON:
        return resolve_visual_template(self.config.visuals_root, template_id)

    def list_visuals(self) -> list[JSON]:
        return self.inventory.list_visuals()

    def create_visual(self, payload: JSON) -> JSON:
        template_id = payload.get("templateId")
        if not isinstance(template_id, str) or not template_id:
            raise ValueError("templateId is required")
        # Validate template exists when visuals root is present.
        if self.config.visuals_root:
            resolve_visual_template(self.config.visuals_root, template_id)
        visual = self.inventory.create_visual(payload)
        session_id = payload.get("sessionId")
        if isinstance(session_id, str) and session_id:
            self.emit(
                session_id=session_id,
                run_id=payload.get("runId"),
                source="system",
                event_kind="visual.created",
                payload={
                    "visualId": visual["id"],
                    "templateId": visual["templateId"],
                    "title": visual["title"],
                },
            )
        return visual

    def update_visual(self, visual_id: str, updates: JSON) -> JSON:
        return self.inventory.update_visual(visual_id, updates)

    def get_visual(self, visual_id: str) -> JSON:
        return self.inventory.get_visual(visual_id)

    def save_visual_tsx(self, visual_id: str, *, tsx: str | None = None) -> JSON:
        visual = self.inventory.get_visual(visual_id)
        root = self.config.visuals_root
        if root is None:
            raise RuntimeError("SYNTH_VISUALS_ROOT is not configured")
        instances = root / "instances"
        instances.mkdir(parents=True, exist_ok=True)
        path = instances / f"{visual_id}.tsx"
        if tsx is None:
            tsx = (
                f'/* Auto-saved visual instance */\n'
                f'export const visualId = "{visual_id}";\n'
                f'export const templateId = "{visual["templateId"]}";\n'
                f'export const title = {json.dumps(visual["title"])};\n'
                f'export const bindings = {json.dumps(visual.get("bindings") or {}, indent=2)} as const;\n'
                f'export {{ default as Shell }} from "../templates/{visual["templateId"]}/shell";\n'
            )
        path.write_text(tsx, encoding="utf-8")
        return self.inventory.update_visual(visual_id, {"tsxPath": str(path)})

    def list_usage(self, *, limit: int = 100) -> list[JSON]:
        return self.inventory.list_usage(limit=limit)

    def simulate_live_eval(self, *, kind: str = "eval") -> JSON:
        """Emit a fixture live-eval stream into a system session visual binding."""
        events_path = None
        if self.config.visuals_root:
            candidate = self.config.visuals_root / "fixtures" / "live_eval_events.json"
            if candidate.exists():
                events_path = candidate
        events = []
        if events_path:
            events = json.loads(events_path.read_text(encoding="utf-8"))
        else:
            events = [
                {"t": 0, "kind": "run.started", "message": f"{kind} started"},
                {"t": 1, "kind": "step", "message": "rollout 1/3", "score": 0.4},
                {"t": 2, "kind": "step", "message": "rollout 2/3", "score": 0.7},
                {"t": 3, "kind": "run.completed", "message": "pass", "score": 1.0},
            ]
        visual = self.create_visual(
            {
                "templateId": {
                    "eval": "live.eval_stream.v1",
                    "dock": "live.dock_harbor.v1",
                    "intern": "live.intern_acceptance.v1",
                }.get(kind, "live.eval_stream.v1"),
                "title": f"Live {kind} stream",
                "bindings": {
                    "events": {
                        "kind": "fixture",
                        "events": events,
                    }
                },
                "metadata": {"simulated": True, "kind": kind},
            }
        )
        return {"visual": visual, "eventCount": len(events)}

    def _resume_existing_sessions(self) -> None:
        with self._resume_lock:
            for session in self.store.list_sessions():
                if session.get("target", {}).get("kind") == "intern":
                    self.intern_adapter.resume_existing(session)
