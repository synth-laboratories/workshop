from __future__ import annotations

import asyncio
import json
import sqlite3
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any


@dataclass(frozen=True, slots=True)
class StoredResponse:
    response: dict[str, Any]
    request: dict[str, Any]
    context_items: list[dict[str, Any]]


class SQLiteResponseStore:
    """SQLite WAL store with one ordered asynchronous writer queue."""

    def __init__(self, path: Path) -> None:
        self.path = path
        self._queue: asyncio.Queue[tuple[str, tuple[Any, ...], asyncio.Future[Any]]] = asyncio.Queue()
        self._writer: asyncio.Task[None] | None = None
        self._start_lock = asyncio.Lock()

    async def start(self) -> None:
        if self._writer is not None:
            return
        async with self._start_lock:
            if self._writer is not None:
                return
            self.path.parent.mkdir(parents=True, exist_ok=True)
            await asyncio.to_thread(self._initialize)
            self._writer = asyncio.create_task(self._writer_loop(), name="responses-sqlite-writer")

    def _initialize(self) -> None:
        with sqlite3.connect(self.path) as connection:
            connection.execute("PRAGMA journal_mode=WAL")
            connection.execute("PRAGMA synchronous=NORMAL")
            connection.execute(
                """
                CREATE TABLE IF NOT EXISTS responses (
                    id TEXT PRIMARY KEY,
                    response_json TEXT NOT NULL,
                    request_json TEXT NOT NULL,
                    context_json TEXT NOT NULL,
                    status TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL
                )
                """
            )

    async def _writer_loop(self) -> None:
        connection = sqlite3.connect(self.path)
        connection.execute("PRAGMA journal_mode=WAL")
        try:
            while True:
                operation, args, future = await self._queue.get()
                try:
                    if operation == "put":
                        connection.execute(
                            """
                            INSERT INTO responses (
                                id, response_json, request_json, context_json,
                                status, created_at, updated_at
                            ) VALUES (?, ?, ?, ?, ?, ?, ?)
                            ON CONFLICT(id) DO UPDATE SET
                                response_json=excluded.response_json,
                                request_json=excluded.request_json,
                                context_json=excluded.context_json,
                                status=excluded.status,
                                updated_at=excluded.updated_at
                            """,
                            args,
                        )
                    elif operation == "delete":
                        cursor = connection.execute("DELETE FROM responses WHERE id = ?", args)
                        future.set_result(cursor.rowcount > 0)
                    connection.commit()
                    if not future.done():
                        future.set_result(None)
                except BaseException as exc:
                    connection.rollback()
                    if not future.done():
                        future.set_exception(exc)
        except asyncio.CancelledError:
            pass
        finally:
            connection.close()

    async def put(
        self,
        response: dict[str, Any],
        request: dict[str, Any],
        context_items: list[dict[str, Any]],
    ) -> None:
        await self.start()
        now = int(time.time())
        future: asyncio.Future[Any] = asyncio.get_running_loop().create_future()
        await self._queue.put(
            (
                "put",
                (
                    response["id"],
                    json.dumps(response, ensure_ascii=False, separators=(",", ":")),
                    json.dumps(request, ensure_ascii=False, separators=(",", ":")),
                    json.dumps(context_items, ensure_ascii=False, separators=(",", ":")),
                    response["status"],
                    int(response["created_at"]),
                    now,
                ),
                future,
            )
        )
        await future

    async def get(self, response_id: str) -> StoredResponse | None:
        await self.start()
        def read() -> tuple[str, str, str] | None:
            with sqlite3.connect(self.path) as connection:
                row = connection.execute(
                    "SELECT response_json, request_json, context_json FROM responses WHERE id = ?",
                    (response_id,),
                ).fetchone()
                return row if row else None

        row = await asyncio.to_thread(read)
        if row is None:
            return None
        return StoredResponse(
            response=json.loads(row[0]),
            request=json.loads(row[1]),
            context_items=json.loads(row[2]),
        )

    async def delete(self, response_id: str) -> bool:
        await self.start()
        future: asyncio.Future[Any] = asyncio.get_running_loop().create_future()
        await self._queue.put(("delete", (response_id,), future))
        return bool(await future)

    async def close(self) -> None:
        if self._writer is not None:
            self._writer.cancel()
            try:
                await self._writer
            except asyncio.CancelledError:
                pass
            self._writer = None
