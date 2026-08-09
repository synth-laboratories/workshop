"""Replayable Postgres event reads with Redis-assisted SSE tailing.

Redis only wakes the tail loop. Every event returned to a client is reread from
the authoritative PostgreSQL sequence, so stream trimming or total Redis loss
cannot create a permanent cursor gap.

See: ``notes/specifications/tanha/current/systems/intern/runtime_authority.md``.
"""

from __future__ import annotations

import asyncio
import json
import logging
from collections.abc import AsyncIterator
from typing import Literal, cast

import sqlalchemy as sa
from fastapi import HTTPException, Request
from fastapi.responses import StreamingResponse
from sqlalchemy.ext.asyncio import AsyncSession

from core.data.db.models.smr import (
    SmrInternAsyncAssignment,
    SmrInternRuntimeEvent,
    SmrInternSyncSession,
)
from core.data.keyvalue.redis import get_async_redis
from packages.intern.contracts import (
    InternRuntimeEventResponse,
    InternRuntimeEventStreamEnvelope,
)
from packages.intern.mailbox import InternEventCursor, InternMailboxRuntime
from services.intern.mailbox_repository import (
    PostgresInternMailboxRepository,
    event_row_to_mailbox_event,
    mailbox_event_to_response,
)


logger = logging.getLogger(__name__)
_DATABASE_BATCH_MAX = 500
_REDIS_BLOCK_MILLISECONDS = 5_000
_REDIS_STREAM_KEY = "intern:runtime-events"
_STREAM_HEARTBEAT_SECONDS = 15


def _validate_runtime_kind(runtime_kind: str) -> None:
    if runtime_kind not in {"sync", "async"}:
        raise HTTPException(
            status_code=422,
            detail={"error_code": "intern_runtime_kind_invalid"},
        )


def _runtime_kind(value: str) -> Literal["sync", "async"]:
    _validate_runtime_kind(value)
    return cast(Literal["sync", "async"], value)


async def require_runtime_access(
    db: AsyncSession,
    *,
    org_id: str,
    runtime_kind: str,
    runtime_id: str,
) -> None:
    """Require that the runtime belongs to the authenticated organization."""

    _validate_runtime_kind(runtime_kind)
    if runtime_kind == "sync":
        statement = sa.select(SmrInternSyncSession.sync_session_id).where(
            SmrInternSyncSession.org_id == org_id,
            SmrInternSyncSession.sync_session_id == runtime_id,
        )
    else:
        statement = sa.select(SmrInternAsyncAssignment.async_assignment_id).where(
            SmrInternAsyncAssignment.org_id == org_id,
            SmrInternAsyncAssignment.async_assignment_id == runtime_id,
        )
    if await db.scalar(statement) is None:
        raise HTTPException(
            status_code=404,
            detail={
                "error_code": "intern_runtime_not_found",
                "runtime_kind": runtime_kind,
                "runtime_id": runtime_id,
            },
        )


def runtime_event_response(row: SmrInternRuntimeEvent) -> InternRuntimeEventResponse:
    """Compatibility conversion for callers that still hold an ORM row."""

    return mailbox_event_to_response(event_row_to_mailbox_event(row))


async def list_runtime_events(
    db: AsyncSession,
    *,
    org_id: str,
    runtime_kind: str,
    runtime_id: str,
    after_sequence: int,
    limit: int,
) -> list[InternRuntimeEventResponse]:
    """Read one strictly ordered, bounded page from PostgreSQL."""

    if not 1 <= limit <= _DATABASE_BATCH_MAX:
        raise ValueError("intern_runtime_event_limit_out_of_bounds")
    await require_runtime_access(
        db,
        org_id=org_id,
        runtime_kind=runtime_kind,
        runtime_id=runtime_id,
    )
    runtime = InternMailboxRuntime(_runtime_kind(runtime_kind), runtime_id)
    page = await PostgresInternMailboxRepository(db, org_id=org_id).events(
        runtime,
        after=InternEventCursor(after_sequence),
        limit=limit,
    )
    return [mailbox_event_to_response(event) for event in page.events]


def _resume_sequence(after_sequence: int, last_event_id: str | None) -> int:
    if after_sequence < 0:
        raise HTTPException(
            status_code=422, detail="after_sequence must be nonnegative"
        )
    if last_event_id is None or not last_event_id.strip():
        return after_sequence
    try:
        event_sequence = int(last_event_id.strip())
    except ValueError as error:
        raise HTTPException(
            status_code=400,
            detail={"error_code": "intern_runtime_event_cursor_invalid"},
        ) from error
    if event_sequence < 0:
        raise HTTPException(
            status_code=400,
            detail={"error_code": "intern_runtime_event_cursor_invalid"},
        )
    return max(after_sequence, event_sequence)


def _server_sent_event(event: InternRuntimeEventResponse) -> str:
    envelope = InternRuntimeEventStreamEnvelope(event=event)
    data = json.dumps(envelope.model_dump(mode="json"), separators=(",", ":"))
    return f"id: {event.sequence}\nevent: {event.event_kind}\ndata: {data}\n\n"


async def _event_generator(
    *,
    request: Request,
    org_id: str,
    runtime_kind: str,
    runtime_id: str,
    resume_sequence: int,
) -> AsyncIterator[str]:
    # This loop is intentionally connection-long. PostgreSQL and Redis reads,
    # batch sizes, Redis blocking, and heartbeat intervals are all bounded.
    from core.data.db.session import get_async_session_factory

    session_factory = get_async_session_factory()
    sequence = resume_sequence
    heartbeat_elapsed_seconds = 0
    while not await request.is_disconnected():
        async with session_factory() as db:
            events = await list_runtime_events(
                db,
                org_id=org_id,
                runtime_kind=runtime_kind,
                runtime_id=runtime_id,
                after_sequence=sequence,
                limit=_DATABASE_BATCH_MAX,
            )
        if events:
            for event in events:
                if event.sequence <= sequence:
                    raise RuntimeError("intern_runtime_event_sequence_not_monotonic")
                sequence = event.sequence
                yield _server_sent_event(event)
            heartbeat_elapsed_seconds = 0
            continue
        try:
            redis = get_async_redis(decode_responses=True)
            await redis.xread(
                streams={_REDIS_STREAM_KEY: "$"},
                count=_DATABASE_BATCH_MAX,
                block=_REDIS_BLOCK_MILLISECONDS,
            )
        except asyncio.CancelledError:
            raise
        except Exception:
            logger.warning(
                "intern.runtime_event_redis_tail_degraded "
                "runtime_kind=%s runtime_id=%s sequence=%s",
                runtime_kind,
                runtime_id,
                sequence,
                exc_info=True,
            )
            await asyncio.sleep(1)
            heartbeat_elapsed_seconds += 1
        else:
            heartbeat_elapsed_seconds += _REDIS_BLOCK_MILLISECONDS // 1_000
        if heartbeat_elapsed_seconds >= _STREAM_HEARTBEAT_SECONDS:
            yield ": heartbeat\n\n"
            heartbeat_elapsed_seconds = 0


def stream_runtime_events(
    *,
    request: Request,
    org_id: str,
    runtime_kind: str,
    runtime_id: str,
    after_sequence: int,
    last_event_id: str | None,
) -> StreamingResponse:
    """Return replay-then-tail SSE with PostgreSQL cursor recovery."""

    sequence = _resume_sequence(after_sequence, last_event_id)
    return StreamingResponse(
        _event_generator(
            request=request,
            org_id=org_id,
            runtime_kind=runtime_kind,
            runtime_id=runtime_id,
            resume_sequence=sequence,
        ),
        media_type="text/event-stream",
        headers={
            "Cache-Control": "no-cache",
            "X-Accel-Buffering": "no",
        },
    )
