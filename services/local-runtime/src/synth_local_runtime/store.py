from __future__ import annotations

import json
import sqlite3
import threading
from contextlib import contextmanager
from pathlib import Path
from typing import Any, Iterator

from .models import EventInput, JSON, default_title, new_id, utc_now, validate_target


class RuntimeStore:
    """SQLite authority for desktop sessions, runs, normalized events, and cursors."""

    def __init__(self, path: str | Path) -> None:
        self.path = Path(path)
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self._local = threading.local()
        self._schema_lock = threading.Lock()
        self._initialize_schema()

    def close_thread_connection(self) -> None:
        connection = getattr(self._local, "connection", None)
        if connection is not None:
            connection.close()
            self._local.connection = None

    def _connect(self) -> sqlite3.Connection:
        connection = getattr(self._local, "connection", None)
        if connection is None:
            connection = sqlite3.connect(
                self.path,
                timeout=30,
                isolation_level=None,
                check_same_thread=False,
            )
            connection.row_factory = sqlite3.Row
            connection.execute("PRAGMA journal_mode=WAL")
            connection.execute("PRAGMA foreign_keys=ON")
            connection.execute("PRAGMA busy_timeout=30000")
            self._local.connection = connection
        return connection

    @contextmanager
    def _transaction(self) -> Iterator[sqlite3.Connection]:
        connection = self._connect()
        connection.execute("BEGIN IMMEDIATE")
        try:
            yield connection
        except Exception:
            connection.execute("ROLLBACK")
            raise
        else:
            connection.execute("COMMIT")

    def _initialize_schema(self) -> None:
        with self._schema_lock:
            connection = self._connect()
            connection.executescript(
                """
                CREATE TABLE IF NOT EXISTS projects (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    path TEXT NOT NULL UNIQUE,
                    vcs TEXT,
                    metadata_json TEXT NOT NULL DEFAULT '{}',
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS sessions (
                    id TEXT PRIMARY KEY,
                    title TEXT NOT NULL,
                    target_json TEXT NOT NULL,
                    project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,
                    remote_id TEXT,
                    status TEXT NOT NULL,
                    state_generation INTEGER,
                    latest_cursor INTEGER NOT NULL DEFAULT 0,
                    active_run_id TEXT,
                    metadata_json TEXT NOT NULL DEFAULT '{}',
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS runs (
                    id TEXT PRIMARY KEY,
                    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                    mode TEXT NOT NULL,
                    status TEXT NOT NULL,
                    latest_cursor INTEGER NOT NULL DEFAULT 0,
                    checkpoint_json TEXT,
                    outcome_json TEXT,
                    model TEXT,
                    adapter TEXT,
                    metadata_json TEXT NOT NULL DEFAULT '{}',
                    created_at TEXT NOT NULL,
                    started_at TEXT,
                    completed_at TEXT
                );

                CREATE TABLE IF NOT EXISTS events (
                    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                    sequence INTEGER NOT NULL,
                    run_id TEXT REFERENCES runs(id) ON DELETE SET NULL,
                    source TEXT NOT NULL,
                    remote_sequence INTEGER,
                    event_kind TEXT NOT NULL,
                    payload_json TEXT NOT NULL,
                    command_id TEXT,
                    created_at TEXT NOT NULL,
                    PRIMARY KEY (session_id, sequence)
                );

                CREATE UNIQUE INDEX IF NOT EXISTS events_remote_dedupe
                ON events(session_id, source, remote_sequence)
                WHERE remote_sequence IS NOT NULL;

                CREATE TABLE IF NOT EXISTS cursors (
                    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                    source TEXT NOT NULL,
                    cursor INTEGER NOT NULL DEFAULT 0,
                    PRIMARY KEY (session_id, source)
                );

                CREATE INDEX IF NOT EXISTS events_session_sequence
                ON events(session_id, sequence);

                CREATE INDEX IF NOT EXISTS sessions_updated_at
                ON sessions(updated_at DESC);
                """
            )
            columns = {
                str(row[1])
                for row in connection.execute("PRAGMA table_info(sessions)").fetchall()
            }
            if "project_id" not in columns:
                connection.execute(
                    "ALTER TABLE sessions ADD COLUMN project_id TEXT REFERENCES projects(id)"
                )

    @staticmethod
    def _json(value: Any) -> str:
        return json.dumps(value, separators=(",", ":"), ensure_ascii=False)

    @staticmethod
    def _loads(value: str | None, default: Any) -> Any:
        if value is None:
            return default
        try:
            return json.loads(value)
        except json.JSONDecodeError:
            return default

    def create_project(
        self,
        path_value: object,
        *,
        name: str | None = None,
        vcs: str | None = None,
        metadata: JSON | None = None,
    ) -> JSON:
        if not isinstance(path_value, str) or not path_value.strip():
            raise ValueError("project path is required")
        path = Path(path_value).expanduser().resolve()
        if not path.exists() or not path.is_dir():
            raise ValueError("project path must be an existing directory")
        project_id = new_id("proj")
        clean_name = (name or path.name or "Project").strip()[:160]
        now = utc_now()
        try:
            with self._transaction() as connection:
                connection.execute(
                    """
                    INSERT INTO projects(id, name, path, vcs, metadata_json, created_at, updated_at)
                    VALUES (?, ?, ?, ?, ?, ?, ?)
                    """,
                    (project_id, clean_name, str(path), vcs, self._json(metadata or {}), now, now),
                )
        except sqlite3.IntegrityError as exc:
            if "projects.path" in str(exc):
                row = self._connect().execute(
                    "SELECT * FROM projects WHERE path = ?", (str(path),)
                ).fetchone()
                if row is not None:
                    return self._project_row(row)
            raise ValueError("project already exists") from exc
        return self.get_project(project_id)

    def list_projects(self) -> list[JSON]:
        rows = self._connect().execute(
            "SELECT * FROM projects ORDER BY updated_at DESC, created_at DESC"
        ).fetchall()
        return [self._project_row(row) for row in rows]

    def get_project(self, project_id: str) -> JSON:
        row = self._connect().execute(
            "SELECT * FROM projects WHERE id = ?", (project_id,)
        ).fetchone()
        if row is None:
            raise KeyError(project_id)
        return self._project_row(row)

    def delete_project(self, project_id: str) -> bool:
        with self._transaction() as connection:
            cursor = connection.execute("DELETE FROM projects WHERE id = ?", (project_id,))
        return cursor.rowcount > 0

    def create_session(
        self,
        target_value: object,
        *,
        title: str | None = None,
        project_id: str | None = None,
        metadata: JSON | None = None,
    ) -> JSON:
        target = validate_target(target_value)
        now = utc_now()
        session_id = new_id("ses")
        clean_title = (title or "").strip() or default_title(target)
        if project_id is not None:
            self.get_project(project_id)
        with self._transaction() as connection:
            connection.execute(
                """
                INSERT INTO sessions(
                    id, title, target_json, project_id, status, metadata_json, created_at, updated_at
                ) VALUES (?, ?, ?, ?, 'ready', ?, ?, ?)
                """,
                (
                    session_id,
                    clean_title[:160],
                    self._json(target),
                    project_id,
                    self._json(metadata or {}),
                    now,
                    now,
                ),
            )
            connection.execute(
                "INSERT INTO cursors(session_id, source, cursor) VALUES (?, ?, 0)",
                (session_id, target["kind"]),
            )
        return self.get_session(session_id)

    def list_sessions(self) -> list[JSON]:
        rows = self._connect().execute(
            "SELECT * FROM sessions ORDER BY updated_at DESC, created_at DESC"
        ).fetchall()
        return [self._session_row(row) for row in rows]

    def get_session(self, session_id: str) -> JSON:
        row = self._connect().execute(
            "SELECT * FROM sessions WHERE id = ?", (session_id,)
        ).fetchone()
        if row is None:
            raise KeyError(session_id)
        return self._session_row(row)

    def delete_session(self, session_id: str) -> bool:
        with self._transaction() as connection:
            cursor = connection.execute("DELETE FROM sessions WHERE id = ?", (session_id,))
        return cursor.rowcount > 0

    def update_session(self, session_id: str, **changes: Any) -> JSON:
        allowed = {
            "title": "title",
            "remote_id": "remote_id",
            "status": "status",
            "state_generation": "state_generation",
            "latest_cursor": "latest_cursor",
            "active_run_id": "active_run_id",
            "metadata": "metadata_json",
        }
        assignments: list[str] = []
        values: list[Any] = []
        for key, value in changes.items():
            column = allowed.get(key)
            if column is None:
                raise ValueError(f"unsupported session field: {key}")
            if key == "metadata":
                value = self._json(value)
            assignments.append(f"{column} = ?")
            values.append(value)
        if not assignments:
            return self.get_session(session_id)
        assignments.append("updated_at = ?")
        values.append(utc_now())
        values.append(session_id)
        with self._transaction() as connection:
            cursor = connection.execute(
                f"UPDATE sessions SET {', '.join(assignments)} WHERE id = ?", values
            )
            if cursor.rowcount == 0:
                raise KeyError(session_id)
        return self.get_session(session_id)

    def create_run(self, session_id: str, *, metadata: JSON | None = None) -> JSON:
        session = self.get_session(session_id)
        target = session["target"]
        if target["kind"] == "local":
            mode = "local"
            model = target.get("model")
            adapter = target.get("adapter")
        elif target["kind"] == "remote":
            mode = "remote"
            model = target.get("model")
            adapter = target.get("adapter")
        else:
            mode = target["mode"]
            model = None
            adapter = None
        now = utc_now()
        run_id = new_id("run")
        with self._transaction() as connection:
            connection.execute(
                """
                INSERT INTO runs(
                    id, session_id, mode, status, model, adapter, metadata_json, created_at
                ) VALUES (?, ?, ?, 'queued', ?, ?, ?, ?)
                """,
                (run_id, session_id, mode, model, adapter, self._json(metadata or {}), now),
            )
            connection.execute(
                """
                UPDATE sessions
                SET active_run_id = ?, status = 'running', updated_at = ?
                WHERE id = ?
                """,
                (run_id, now, session_id),
            )
        return self.get_run(run_id)

    def get_run(self, run_id: str) -> JSON:
        row = self._connect().execute("SELECT * FROM runs WHERE id = ?", (run_id,)).fetchone()
        if row is None:
            raise KeyError(run_id)
        return self._run_row(row)

    def list_runs(self, session_id: str) -> list[JSON]:
        rows = self._connect().execute(
            "SELECT * FROM runs WHERE session_id = ? ORDER BY created_at", (session_id,)
        ).fetchall()
        return [self._run_row(row) for row in rows]

    def update_run(self, run_id: str, **changes: Any) -> JSON:
        allowed = {
            "status": "status",
            "latest_cursor": "latest_cursor",
            "checkpoint": "checkpoint_json",
            "outcome": "outcome_json",
            "metadata": "metadata_json",
            "started_at": "started_at",
            "completed_at": "completed_at",
        }
        assignments: list[str] = []
        values: list[Any] = []
        for key, value in changes.items():
            column = allowed.get(key)
            if column is None:
                raise ValueError(f"unsupported run field: {key}")
            if key in {"checkpoint", "outcome", "metadata"}:
                value = self._json(value) if value is not None else None
            assignments.append(f"{column} = ?")
            values.append(value)
        if not assignments:
            return self.get_run(run_id)
        values.append(run_id)
        with self._transaction() as connection:
            cursor = connection.execute(
                f"UPDATE runs SET {', '.join(assignments)} WHERE id = ?", values
            )
            if cursor.rowcount == 0:
                raise KeyError(run_id)
        return self.get_run(run_id)

    def append_event(self, event: EventInput) -> tuple[JSON, bool]:
        created_at = event.created_at or utc_now()
        with self._transaction() as connection:
            if event.remote_sequence is not None:
                existing = connection.execute(
                    """
                    SELECT * FROM events
                    WHERE session_id = ? AND source = ? AND remote_sequence = ?
                    """,
                    (event.session_id, event.source, event.remote_sequence),
                ).fetchone()
                if existing is not None:
                    return self._event_row(existing), False

            session_row = connection.execute(
                "SELECT latest_cursor FROM sessions WHERE id = ?", (event.session_id,)
            ).fetchone()
            if session_row is None:
                raise KeyError(event.session_id)
            sequence = int(session_row["latest_cursor"]) + 1
            connection.execute(
                """
                INSERT INTO events(
                    session_id, sequence, run_id, source, remote_sequence,
                    event_kind, payload_json, command_id, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                """,
                (
                    event.session_id,
                    sequence,
                    event.run_id,
                    event.source,
                    event.remote_sequence,
                    event.event_kind,
                    self._json(event.payload),
                    event.command_id,
                    created_at,
                ),
            )
            connection.execute(
                "UPDATE sessions SET latest_cursor = ?, updated_at = ? WHERE id = ?",
                (sequence, created_at, event.session_id),
            )
            if event.run_id:
                connection.execute(
                    "UPDATE runs SET latest_cursor = ? WHERE id = ?",
                    (sequence, event.run_id),
                )
            if event.remote_sequence is not None:
                connection.execute(
                    """
                    INSERT INTO cursors(session_id, source, cursor)
                    VALUES (?, ?, ?)
                    ON CONFLICT(session_id, source)
                    DO UPDATE SET cursor = MAX(cursors.cursor, excluded.cursor)
                    """,
                    (event.session_id, event.source, event.remote_sequence),
                )
            row = connection.execute(
                "SELECT * FROM events WHERE session_id = ? AND sequence = ?",
                (event.session_id, sequence),
            ).fetchone()
            assert row is not None
        return self._event_row(row), True

    def list_events(self, session_id: str, *, after_sequence: int = 0, limit: int = 500) -> JSON:
        if after_sequence < 0:
            raise ValueError("after_sequence must be non-negative")
        bounded_limit = max(1, min(500, int(limit)))
        self.get_session(session_id)
        rows = self._connect().execute(
            """
            SELECT * FROM events
            WHERE session_id = ? AND sequence > ?
            ORDER BY sequence ASC
            LIMIT ?
            """,
            (session_id, after_sequence, bounded_limit),
        ).fetchall()
        events = [self._event_row(row) for row in rows]
        next_sequence = events[-1]["sequence"] if events else after_sequence
        return {"events": events, "nextSequence": next_sequence}

    def get_cursor(self, session_id: str, source: str) -> int:
        row = self._connect().execute(
            "SELECT cursor FROM cursors WHERE session_id = ? AND source = ?",
            (session_id, source),
        ).fetchone()
        return int(row["cursor"]) if row else 0

    def set_cursor(self, session_id: str, source: str, cursor: int) -> None:
        with self._transaction() as connection:
            connection.execute(
                """
                INSERT INTO cursors(session_id, source, cursor)
                VALUES (?, ?, ?)
                ON CONFLICT(session_id, source) DO UPDATE SET cursor = excluded.cursor
                """,
                (session_id, source, max(0, int(cursor))),
            )

    def _session_row(self, row: sqlite3.Row) -> JSON:
        return {
            "id": row["id"],
            "title": row["title"],
            "target": self._loads(row["target_json"], {}),
            "projectId": row["project_id"],
            "remoteId": row["remote_id"],
            "createdAt": row["created_at"],
            "updatedAt": row["updated_at"],
            "status": row["status"],
            "stateGeneration": row["state_generation"],
            "latestCursor": int(row["latest_cursor"]),
            "activeRunId": row["active_run_id"],
            "metadata": self._loads(row["metadata_json"], {}),
        }

    def _project_row(self, row: sqlite3.Row) -> JSON:
        return {
            "id": row["id"],
            "name": row["name"],
            "path": row["path"],
            "vcs": row["vcs"],
            "metadata": self._loads(row["metadata_json"], {}),
            "createdAt": row["created_at"],
            "updatedAt": row["updated_at"],
        }

    def counts(self) -> JSON:
        connection = self._connect()
        return {
            "projects": int(connection.execute("SELECT COUNT(*) FROM projects").fetchone()[0]),
            "sessions": int(connection.execute("SELECT COUNT(*) FROM sessions").fetchone()[0]),
            "runs": int(connection.execute("SELECT COUNT(*) FROM runs").fetchone()[0]),
            "events": int(connection.execute("SELECT COUNT(*) FROM events").fetchone()[0]),
        }

    def _run_row(self, row: sqlite3.Row) -> JSON:
        return {
            "id": row["id"],
            "sessionId": row["session_id"],
            "mode": row["mode"],
            "status": row["status"],
            "latestCursor": int(row["latest_cursor"]),
            "checkpoint": self._loads(row["checkpoint_json"], None),
            "outcome": self._loads(row["outcome_json"], None),
            "model": row["model"],
            "adapter": row["adapter"],
            "metadata": self._loads(row["metadata_json"], {}),
            "createdAt": row["created_at"],
            "startedAt": row["started_at"],
            "completedAt": row["completed_at"],
        }

    def _event_row(self, row: sqlite3.Row) -> JSON:
        return {
            "schemaVersion": "synth.desktop-runtime-event.v1",
            "sessionId": row["session_id"],
            "runId": row["run_id"],
            "sequence": int(row["sequence"]),
            "remoteSequence": row["remote_sequence"],
            "eventKind": row["event_kind"],
            "payload": self._loads(row["payload_json"], {}),
            "commandId": row["command_id"],
            "createdAt": row["created_at"],
            "source": row["source"],
        }
