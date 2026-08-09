from __future__ import annotations

import hashlib
import json
import sqlite3
from pathlib import Path
from typing import Any

from .models import JSON, new_id, utc_now


class InventoryStore:
    """First-class local inventory for containers, Trace V5, visuals, usage."""

    def __init__(self, database_path: Path) -> None:
        self.database_path = database_path
        self._ensure_schema()

    def _connect(self) -> sqlite3.Connection:
        conn = sqlite3.connect(self.database_path)
        conn.row_factory = sqlite3.Row
        conn.execute("PRAGMA journal_mode=WAL")
        return conn

    def _ensure_schema(self) -> None:
        with self._connect() as conn:
            conn.executescript(
                """
                CREATE TABLE IF NOT EXISTS containers (
                  id TEXT PRIMARY KEY,
                  name TEXT NOT NULL,
                  location TEXT NOT NULL,
                  status TEXT NOT NULL,
                  base_url TEXT,
                  pool_id TEXT,
                  task_family TEXT,
                  last_rollout_id TEXT,
                  health_json TEXT NOT NULL DEFAULT '{}',
                  created_at TEXT NOT NULL,
                  updated_at TEXT NOT NULL,
                  metadata_json TEXT NOT NULL DEFAULT '{}'
                );
                CREATE TABLE IF NOT EXISTS traces (
                  id TEXT PRIMARY KEY,
                  digest TEXT NOT NULL UNIQUE,
                  title TEXT NOT NULL,
                  source TEXT NOT NULL,
                  container_id TEXT,
                  session_id TEXT,
                  run_id TEXT,
                  reward REAL,
                  metrics_json TEXT NOT NULL DEFAULT '[]',
                  created_at TEXT NOT NULL,
                  path TEXT,
                  metadata_json TEXT NOT NULL DEFAULT '{}'
                );
                CREATE TABLE IF NOT EXISTS visuals (
                  id TEXT PRIMARY KEY,
                  template_id TEXT NOT NULL,
                  title TEXT NOT NULL,
                  bindings_json TEXT NOT NULL DEFAULT '{}',
                  tsx_path TEXT,
                  created_at TEXT NOT NULL,
                  updated_at TEXT NOT NULL,
                  metadata_json TEXT NOT NULL DEFAULT '{}'
                );
                CREATE TABLE IF NOT EXISTS usage_ledger (
                  id TEXT PRIMARY KEY,
                  provider TEXT NOT NULL,
                  model TEXT NOT NULL,
                  session_id TEXT,
                  run_id TEXT,
                  prompt_tokens INTEGER NOT NULL,
                  completion_tokens INTEGER NOT NULL,
                  total_tokens INTEGER NOT NULL,
                  cost_usd REAL,
                  created_at TEXT NOT NULL
                );
                """
            )

    # ── Containers ──────────────────────────────────────────────────────────

    def list_containers(self) -> list[JSON]:
        with self._connect() as conn:
            rows = conn.execute(
                "SELECT * FROM containers ORDER BY updated_at DESC"
            ).fetchall()
        return [self._container_row(row) for row in rows]

    def upsert_container(self, payload: JSON) -> JSON:
        now = utc_now()
        container_id = payload.get("id") or new_id("ctr")
        with self._connect() as conn:
            existing = conn.execute(
                "SELECT id FROM containers WHERE id = ?", (container_id,)
            ).fetchone()
            if existing:
                conn.execute(
                    """
                    UPDATE containers SET
                      name = ?, location = ?, status = ?, base_url = ?,
                      pool_id = ?, task_family = ?, last_rollout_id = ?,
                      health_json = ?, updated_at = ?, metadata_json = ?
                    WHERE id = ?
                    """,
                    (
                        payload.get("name") or "container",
                        payload.get("location") or "local",
                        payload.get("status") or "pending",
                        payload.get("baseUrl"),
                        payload.get("poolId"),
                        payload.get("taskFamily"),
                        payload.get("lastRolloutId"),
                        json.dumps(payload.get("health") or {}),
                        now,
                        json.dumps(payload.get("metadata") or {}),
                        container_id,
                    ),
                )
            else:
                conn.execute(
                    """
                    INSERT INTO containers (
                      id, name, location, status, base_url, pool_id, task_family,
                      last_rollout_id, health_json, created_at, updated_at, metadata_json
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                    """,
                    (
                        container_id,
                        payload.get("name") or "container",
                        payload.get("location") or "local",
                        payload.get("status") or "pending",
                        payload.get("baseUrl"),
                        payload.get("poolId"),
                        payload.get("taskFamily"),
                        payload.get("lastRolloutId"),
                        json.dumps(payload.get("health") or {}),
                        now,
                        now,
                        json.dumps(payload.get("metadata") or {}),
                    ),
                )
            row = conn.execute(
                "SELECT * FROM containers WHERE id = ?", (container_id,)
            ).fetchone()
        return self._container_row(row)

    def get_container(self, container_id: str) -> JSON:
        with self._connect() as conn:
            row = conn.execute(
                "SELECT * FROM containers WHERE id = ?", (container_id,)
            ).fetchone()
        if row is None:
            raise KeyError(container_id)
        return self._container_row(row)

    def delete_container(self, container_id: str) -> bool:
        with self._connect() as conn:
            cur = conn.execute("DELETE FROM containers WHERE id = ?", (container_id,))
            return cur.rowcount > 0

    @staticmethod
    def _container_row(row: sqlite3.Row) -> JSON:
        return {
            "id": row["id"],
            "name": row["name"],
            "location": row["location"],
            "status": row["status"],
            "baseUrl": row["base_url"],
            "poolId": row["pool_id"],
            "taskFamily": row["task_family"],
            "lastRolloutId": row["last_rollout_id"],
            "health": json.loads(row["health_json"] or "{}"),
            "createdAt": row["created_at"],
            "updatedAt": row["updated_at"],
            "metadata": json.loads(row["metadata_json"] or "{}"),
        }

    # ── Traces ──────────────────────────────────────────────────────────────

    def list_traces(self) -> list[JSON]:
        with self._connect() as conn:
            rows = conn.execute(
                "SELECT * FROM traces ORDER BY created_at DESC"
            ).fetchall()
        return [self._trace_row(row) for row in rows]

    def ingest_trace(
        self,
        *,
        title: str,
        payload: JSON | None = None,
        path: str | None = None,
        source: str = "local",
        container_id: str | None = None,
        session_id: str | None = None,
        run_id: str | None = None,
        reward: float | None = None,
        metrics: list[JSON] | None = None,
        metadata: JSON | None = None,
    ) -> JSON:
        if path:
            raw = Path(path).read_bytes()
            digest = hashlib.sha256(raw).hexdigest()
            try:
                body = json.loads(raw.decode("utf-8"))
            except (UnicodeDecodeError, json.JSONDecodeError):
                body = {}
        else:
            body = payload or {}
            raw = json.dumps(body, sort_keys=True, separators=(",", ":")).encode("utf-8")
            digest = hashlib.sha256(raw).hexdigest()
            cas_dir = self.database_path.parent / "cas" / "traces"
            cas_dir.mkdir(parents=True, exist_ok=True)
            path = str(cas_dir / f"{digest}.json")
            Path(path).write_bytes(raw)

        if reward is None and isinstance(body.get("reward"), (int, float)):
            reward = float(body["reward"])
        if metrics is None and isinstance(body.get("metrics"), list):
            metrics = body["metrics"]

        trace_id = new_id("tr")
        now = utc_now()
        with self._connect() as conn:
            existing = conn.execute(
                "SELECT * FROM traces WHERE digest = ?", (digest,)
            ).fetchone()
            if existing:
                return self._trace_row(existing)
            conn.execute(
                """
                INSERT INTO traces (
                  id, digest, title, source, container_id, session_id, run_id,
                  reward, metrics_json, created_at, path, metadata_json
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                """,
                (
                    trace_id,
                    digest,
                    title,
                    source,
                    container_id,
                    session_id,
                    run_id,
                    reward,
                    json.dumps(metrics or []),
                    now,
                    path,
                    json.dumps(metadata or {}),
                ),
            )
            row = conn.execute(
                "SELECT * FROM traces WHERE id = ?", (trace_id,)
            ).fetchone()
        return self._trace_row(row)

    def get_trace(self, trace_id: str) -> JSON:
        with self._connect() as conn:
            row = conn.execute(
                "SELECT * FROM traces WHERE id = ? OR digest = ?",
                (trace_id, trace_id),
            ).fetchone()
        if row is None:
            raise KeyError(trace_id)
        return self._trace_row(row)

    @staticmethod
    def _trace_row(row: sqlite3.Row) -> JSON:
        return {
            "id": row["id"],
            "digest": row["digest"],
            "title": row["title"],
            "source": row["source"],
            "containerId": row["container_id"],
            "sessionId": row["session_id"],
            "runId": row["run_id"],
            "reward": row["reward"],
            "metrics": json.loads(row["metrics_json"] or "[]"),
            "createdAt": row["created_at"],
            "path": row["path"],
            "metadata": json.loads(row["metadata_json"] or "{}"),
        }

    # ── Visuals ─────────────────────────────────────────────────────────────

    def list_visuals(self) -> list[JSON]:
        with self._connect() as conn:
            rows = conn.execute(
                "SELECT * FROM visuals ORDER BY updated_at DESC"
            ).fetchall()
        return [self._visual_row(row) for row in rows]

    def create_visual(self, payload: JSON) -> JSON:
        visual_id = payload.get("id") or new_id("vis")
        now = utc_now()
        with self._connect() as conn:
            conn.execute(
                """
                INSERT INTO visuals (
                  id, template_id, title, bindings_json, tsx_path,
                  created_at, updated_at, metadata_json
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                """,
                (
                    visual_id,
                    payload["templateId"],
                    payload.get("title") or payload["templateId"],
                    json.dumps(payload.get("bindings") or {}),
                    payload.get("tsxPath"),
                    now,
                    now,
                    json.dumps(payload.get("metadata") or {}),
                ),
            )
            row = conn.execute(
                "SELECT * FROM visuals WHERE id = ?", (visual_id,)
            ).fetchone()
        return self._visual_row(row)

    def update_visual(self, visual_id: str, updates: JSON) -> JSON:
        current = self.get_visual(visual_id)
        merged = {**current, **updates, "id": visual_id}
        now = utc_now()
        with self._connect() as conn:
            conn.execute(
                """
                UPDATE visuals SET
                  template_id = ?, title = ?, bindings_json = ?, tsx_path = ?,
                  updated_at = ?, metadata_json = ?
                WHERE id = ?
                """,
                (
                    merged["templateId"],
                    merged["title"],
                    json.dumps(merged.get("bindings") or {}),
                    merged.get("tsxPath"),
                    now,
                    json.dumps(merged.get("metadata") or {}),
                    visual_id,
                ),
            )
            row = conn.execute(
                "SELECT * FROM visuals WHERE id = ?", (visual_id,)
            ).fetchone()
        return self._visual_row(row)

    def get_visual(self, visual_id: str) -> JSON:
        with self._connect() as conn:
            row = conn.execute(
                "SELECT * FROM visuals WHERE id = ?", (visual_id,)
            ).fetchone()
        if row is None:
            raise KeyError(visual_id)
        return self._visual_row(row)

    @staticmethod
    def _visual_row(row: sqlite3.Row) -> JSON:
        return {
            "id": row["id"],
            "templateId": row["template_id"],
            "title": row["title"],
            "bindings": json.loads(row["bindings_json"] or "{}"),
            "tsxPath": row["tsx_path"],
            "createdAt": row["created_at"],
            "updatedAt": row["updated_at"],
            "metadata": json.loads(row["metadata_json"] or "{}"),
        }

    # ── Usage ───────────────────────────────────────────────────────────────

    def record_usage(
        self,
        *,
        provider: str,
        model: str,
        prompt_tokens: int,
        completion_tokens: int,
        cost_usd: float | None = None,
        session_id: str | None = None,
        run_id: str | None = None,
    ) -> JSON:
        entry_id = new_id("use")
        now = utc_now()
        total = prompt_tokens + completion_tokens
        with self._connect() as conn:
            conn.execute(
                """
                INSERT INTO usage_ledger (
                  id, provider, model, session_id, run_id,
                  prompt_tokens, completion_tokens, total_tokens, cost_usd, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                """,
                (
                    entry_id,
                    provider,
                    model,
                    session_id,
                    run_id,
                    prompt_tokens,
                    completion_tokens,
                    total,
                    cost_usd,
                    now,
                ),
            )
        return {
            "id": entry_id,
            "provider": provider,
            "model": model,
            "sessionId": session_id,
            "runId": run_id,
            "promptTokens": prompt_tokens,
            "completionTokens": completion_tokens,
            "totalTokens": total,
            "costUsd": cost_usd,
            "createdAt": now,
        }

    def list_usage(self, *, limit: int = 100) -> list[JSON]:
        with self._connect() as conn:
            rows = conn.execute(
                "SELECT * FROM usage_ledger ORDER BY created_at DESC LIMIT ?",
                (limit,),
            ).fetchall()
        return [
            {
                "id": row["id"],
                "provider": row["provider"],
                "model": row["model"],
                "sessionId": row["session_id"],
                "runId": row["run_id"],
                "promptTokens": row["prompt_tokens"],
                "completionTokens": row["completion_tokens"],
                "totalTokens": row["total_tokens"],
                "costUsd": row["cost_usd"],
                "createdAt": row["created_at"],
            }
            for row in rows
        ]

    def counts(self) -> JSON:
        with self._connect() as conn:
            containers = conn.execute("SELECT COUNT(*) FROM containers").fetchone()[0]
            traces = conn.execute("SELECT COUNT(*) FROM traces").fetchone()[0]
            visuals = conn.execute("SELECT COUNT(*) FROM visuals").fetchone()[0]
        return {
            "containers": int(containers),
            "traces": int(traces),
            "visuals": int(visuals),
        }

    def seed_demo_inventory(self, visuals_root: Path | None = None) -> JSON:
        """Seed local Craftax container + fixture traces for first-run dogfood."""
        if self.list_containers():
            return {"seeded": False, "reason": "already_populated"}

        local = self.upsert_container(
            {
                "name": "craftax-local",
                "location": "local",
                "status": "ready",
                "baseUrl": "http://127.0.0.1:8100",
                "taskFamily": "craftax",
                "health": {"ok": True, "contract": "synth-containers"},
                "metadata": {"kind": "demo"},
            }
        )
        cloud = self.upsert_container(
            {
                "name": "craftax-pool-slot",
                "location": "cloud",
                "status": "ready",
                "poolId": "pool_craftax_demo",
                "taskFamily": "craftax",
                "health": {"ok": True, "replicas": 2},
                "metadata": {"kind": "demo", "slot": "local-slot"},
            }
        )

        fixture_paths: list[Path] = []
        if visuals_root:
            for name in (
                "craftax_matrix_slice.json",
                "rollout_steps.json",
                "reward_breakdown.json",
            ):
                candidate = visuals_root / "fixtures" / name
                if candidate.exists():
                    fixture_paths.append(candidate)

        traces = []
        for path in fixture_paths:
            traces.append(
                self.ingest_trace(
                    title=f"Fixture · {path.stem}",
                    path=str(path),
                    source="local",
                    container_id=local["id"],
                    metadata={"fixture": True},
                )
            )

        if not traces:
            traces.append(
                self.ingest_trace(
                    title="Craftax demo Trace V5",
                    payload={
                        "schema": "synth.trace.v5",
                        "reward": 11.4,
                        "metrics": [
                            {"name": "achievements", "value": 11.4},
                            {"name": "cost_usd", "value": 0.12},
                        ],
                        "steps": [
                            {"t": 0, "action": "noop", "reward": 0.0},
                            {"t": 1, "action": "move_right", "reward": 0.1},
                        ],
                    },
                    source="local",
                    container_id=local["id"],
                )
            )

        return {
            "seeded": True,
            "containers": [local["id"], cloud["id"]],
            "traces": [t["id"] for t in traces],
        }
