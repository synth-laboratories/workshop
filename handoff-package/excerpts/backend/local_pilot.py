"""Fail-closed development-only lease and credential authority for Sync Local Pilot.

Local Pilot replaces only opaque agent-turn execution. Product operations still
enter through the normal Intern command/effect/MCP observation pipeline.

See: ``notes/specifications/tanha/current/systems/intern/runtime_authority.md``.
"""

from __future__ import annotations

from collections.abc import Mapping
from datetime import UTC, datetime, timedelta
import hashlib
import hmac
import ipaddress
import os
import secrets
from typing import Any

import sqlalchemy as sa
from fastapi import HTTPException
from sqlalchemy.ext.asyncio import AsyncSession

from core.data.db.models.smr import (
    SmrFile,
    SmrInternCapabilityGrant,
    SmrInternLocalPilotLease,
    SmrInternResourceJournalEntry,
    SmrInternRuntimeEffect,
    SmrInternRuntimeEvent,
    SmrInternSyncSession,
)
from packages.intern.runtime import InternEffectKind, InternEffectStatus
from packages.intern.smr_mcp import INTERN_SMR_MCP_TOOL_CAPABILITIES
from packages.intern.core.capabilities import CapabilityOperation

_LEASE_TTL = timedelta(minutes=5)
_TOKEN_PREFIX = "ilp_"
_LOCAL_ENVIRONMENTS = frozenset({"local", "development"})


def require_local_pilot_enabled(*, environ: Mapping[str, str] | None = None) -> None:
    """Require an explicit flag and an unambiguously local environment."""

    values = os.environ if environ is None else environ
    environment = (
        str(values.get("APP_ENVIRONMENT") or values.get("ENVIRONMENT") or "")
        .strip()
        .lower()
    )
    if values.get("INTERN_LOCAL_PILOT_ENABLED") != "1":
        raise HTTPException(status_code=404, detail="intern_local_pilot_disabled")
    if environment not in _LOCAL_ENVIRONMENTS or values.get("RAILWAY_ENVIRONMENT"):
        raise HTTPException(
            status_code=403, detail="intern_local_pilot_environment_forbidden"
        )


def require_loopback(peer_host: str | None) -> None:
    """Reject proxy/network peers; Local Pilot never trusts forwarded headers."""

    try:
        address = ipaddress.ip_address(str(peer_host or "").split("%", 1)[0])
    except ValueError as error:
        raise HTTPException(
            status_code=403, detail="intern_local_pilot_loopback_required"
        ) from error
    if not address.is_loopback:
        raise HTTPException(
            status_code=403, detail="intern_local_pilot_loopback_required"
        )


def _digest(token: str) -> str:
    return hashlib.sha256(token.encode("utf-8")).hexdigest()


def _event(
    db: AsyncSession,
    *,
    session: SmrInternSyncSession,
    event_kind: str,
    lease: SmrInternLocalPilotLease,
    detail: Mapping[str, Any] | None = None,
) -> None:
    now = datetime.now(UTC)
    sequence = int(session.last_event_sequence) + 1
    session.last_event_sequence = sequence
    session.updated_at = now
    db.add(
        SmrInternRuntimeEvent(
            event_id=f"sync:{session.sync_session_id}:{sequence}",
            org_id=str(session.org_id),
            research_intern_id=str(session.research_intern_id),
            runtime_kind="sync",
            runtime_id=str(session.sync_session_id),
            sequence=sequence,
            previous_state_generation=session.state_generation,
            state_generation=session.state_generation,
            event_kind=event_kind,
            command_id=f"local-pilot:{lease.lease_id}:{lease.fence_generation}:{sequence}",
            payload={
                "schema_version": "smr.intern-local-pilot-event.v1",
                "lease_id": str(lease.lease_id),
                "fence_generation": lease.fence_generation,
                "expires_at": lease.expires_at.isoformat(),
                **dict(detail or {}),
            },
            created_at=now,
        )
    )


async def attach(
    db: AsyncSession,
    *,
    org_id: str,
    user_id: str | None,
    sync_session_id: str,
) -> dict[str, Any]:
    require_local_pilot_enabled()
    now = datetime.now(UTC)
    session = (
        await db.execute(
            sa.select(SmrInternSyncSession)
            .where(
                SmrInternSyncSession.org_id == org_id,
                SmrInternSyncSession.sync_session_id == sync_session_id,
            )
            .with_for_update()
        )
    ).scalar_one_or_none()
    if session is None:
        raise HTTPException(status_code=404, detail="intern_sync_session_not_found")
    if session.status in {"closed", "failed"}:
        raise HTTPException(
            status_code=409, detail="intern_local_pilot_session_terminal"
        )
    live_host_effect = await db.scalar(
        sa.select(sa.literal(True)).where(
            sa.exists().where(
                SmrInternRuntimeEffect.runtime_kind == "sync",
                SmrInternRuntimeEffect.runtime_id == sync_session_id,
                SmrInternRuntimeEffect.effect_kind == InternEffectKind.AGENT_TURN.value,
                SmrInternRuntimeEffect.status == InternEffectStatus.CLAIMED.value,
                SmrInternRuntimeEffect.claim_expires_at > now,
            )
        )
    )
    if live_host_effect:
        raise HTTPException(
            status_code=409, detail="intern_local_pilot_host_turn_active"
        )
    lease = (
        await db.execute(
            sa.select(SmrInternLocalPilotLease)
            .where(SmrInternLocalPilotLease.sync_session_id == sync_session_id)
            .with_for_update()
        )
    ).scalar_one_or_none()
    reattaching = (
        lease is not None and lease.released_at is None and lease.expires_at > now
    )
    if reattaching and str(lease.attached_by_user_id or "") != str(user_id or ""):
        raise HTTPException(status_code=409, detail="intern_local_pilot_lease_active")
    token = _TOKEN_PREFIX + secrets.token_urlsafe(32)
    if lease is None:
        lease = SmrInternLocalPilotLease(
            sync_session_id=sync_session_id,
            org_id=org_id,
            token_digest=_digest(token),
            fence_generation=1,
            attached_by_user_id=user_id,
            acquired_at=now,
            renewed_at=now,
            expires_at=now + _LEASE_TTL,
            released_at=None,
        )
        db.add(lease)
        await db.flush()
    else:
        lease.token_digest = _digest(token)
        lease.fence_generation += 1
        lease.attached_by_user_id = user_id
        lease.acquired_at = now
        lease.renewed_at = now
        lease.expires_at = now + _LEASE_TTL
        lease.released_at = None
    _event(
        db,
        session=session,
        event_kind="local_pilot_reattached" if reattaching else "local_pilot_attached",
        lease=lease,
    )
    await db.flush()
    return {
        "lease_id": str(lease.lease_id),
        "sync_session_id": sync_session_id,
        "org_id": org_id,
        "fence_generation": lease.fence_generation,
        "expires_at": lease.expires_at,
        "capability_token": token,
    }


async def authenticate(
    db: AsyncSession,
    *,
    sync_session_id: str,
    token: str,
    fence_generation: int,
    renew: bool = True,
) -> tuple[SmrInternSyncSession, SmrInternLocalPilotLease]:
    require_local_pilot_enabled()
    now = datetime.now(UTC)
    session = (
        await db.execute(
            sa.select(SmrInternSyncSession)
            .where(SmrInternSyncSession.sync_session_id == sync_session_id)
            .with_for_update()
        )
    ).scalar_one_or_none()
    lease = (
        await db.execute(
            sa.select(SmrInternLocalPilotLease)
            .where(SmrInternLocalPilotLease.sync_session_id == sync_session_id)
            .with_for_update()
        )
    ).scalar_one_or_none()
    valid = (
        session is not None
        and lease is not None
        and str(lease.org_id) == str(session.org_id)
        and lease.released_at is None
        and lease.expires_at > now
        and lease.fence_generation == fence_generation
        and token.startswith(_TOKEN_PREFIX)
        and hmac.compare_digest(lease.token_digest, _digest(token))
    )
    if not valid:
        raise HTTPException(
            status_code=401, detail="intern_local_pilot_credential_invalid"
        )
    if renew:
        lease.renewed_at = now
        lease.expires_at = now + _LEASE_TTL
    return session, lease


async def context(
    db: AsyncSession, *, session: SmrInternSyncSession, lease: SmrInternLocalPilotLease
) -> dict[str, Any]:
    events = list(
        (
            await db.execute(
                sa.select(SmrInternRuntimeEvent)
                .where(
                    SmrInternRuntimeEvent.runtime_kind == "sync",
                    SmrInternRuntimeEvent.runtime_id == session.sync_session_id,
                )
                .order_by(SmrInternRuntimeEvent.sequence.desc())
                .limit(64)
            )
        ).scalars()
    )
    grant = (
        await db.execute(
            sa.select(SmrInternCapabilityGrant)
            .where(
                SmrInternCapabilityGrant.runtime_kind == "sync",
                SmrInternCapabilityGrant.runtime_id == session.sync_session_id,
                SmrInternCapabilityGrant.revoked_at.is_(None),
            )
            .order_by(SmrInternCapabilityGrant.grant_version.desc())
            .limit(1)
        )
    ).scalar_one_or_none()
    resources = list(
        (
            await db.execute(
                sa.select(SmrInternResourceJournalEntry)
                .where(
                    SmrInternResourceJournalEntry.runtime_kind == "sync",
                    SmrInternResourceJournalEntry.runtime_id == session.sync_session_id,
                    SmrInternResourceJournalEntry.retired_at.is_(None),
                )
                .order_by(SmrInternResourceJournalEntry.recorded_at.desc())
                .limit(64)
            )
        ).scalars()
    )
    allowed = set(grant.allowed_operations if grant is not None else ())
    project_id = str(session.binding.get("project_id") or "").strip() or None
    file_authorized = bool(
        grant is not None and CapabilityOperation.FILE_READ.value in allowed
    )
    project_file_authorized = bool(
        grant is not None
        and CapabilityOperation.PROJECT_FILE_READ.value in allowed
        and project_id is not None
        and (
            not grant.allowed_project_ids
            or project_id in set(grant.allowed_project_ids)
        )
    )
    attachment_inventory = await _attachment_inventory(
        db,
        org_id=str(session.org_id),
        sync_session_id=str(session.sync_session_id),
        project_id=project_id,
        run_id=str(session.binding.get("run_id") or "").strip() or None,
        file_authorized=file_authorized,
        project_file_authorized=project_file_authorized,
    )
    catalog = [
        {"server_name": "smr", "operation_name": name, "capability": capability.value}
        for name, capability in INTERN_SMR_MCP_TOOL_CAPABILITIES.items()
        if capability.value in allowed
    ]
    return {
        "schema_version": "smr.intern-local-pilot-context.v1",
        "sync_session_id": str(session.sync_session_id),
        "org_id": str(session.org_id),
        "research_intern_id": str(session.research_intern_id),
        "state_generation": session.state_generation,
        "status": session.status,
        "objective": session.objective,
        "binding": dict(session.binding),
        "runtime_state": dict(session.runtime_state),
        "capability_policy": {
            "grant_version": grant.grant_version if grant is not None else None,
            "allowed_operations": list(
                grant.allowed_operations if grant is not None else ()
            ),
            "approval_required_operations": list(
                grant.approval_required_operations if grant is not None else ()
            ),
        },
        "lease": {
            "lease_id": str(lease.lease_id),
            "fence_generation": lease.fence_generation,
            "expires_at": lease.expires_at,
        },
        "mcp_catalog": catalog,
        "files": attachment_inventory["files"],
        "attachments": attachment_inventory,
        "recent_resources": [
            {
                "operation_id": row.operation_id,
                "resource_kind": row.resource_kind,
                "resource_id": row.resource_id,
                "revision": row.resource_revision,
                "receipt_id": row.resource_receipt_id,
                "owner": row.owner,
                "factory_id": row.factory_id,
                "project_id": row.project_id,
                "recorded_generation": row.recorded_generation,
            }
            for row in resources
        ],
        "recent_events": [
            {
                "sequence": row.sequence,
                "event_kind": row.event_kind,
                "payload": dict(row.payload),
                "created_at": row.created_at,
            }
            for row in reversed(events)
        ],
    }


def _file_identity(row: Any) -> dict[str, Any]:
    """Return credential-free file identity, never storage or file contents."""

    return {
        "file_id": str(row.file_id),
        "path": str(row.path),
        "logical_name": str(row.logical_name or "").strip() or None,
        "content_type": str(row.content_type or "").strip() or None,
        "content_sha256": str(row.content_sha256 or "").strip() or None,
        "content_bytes": int(row.content_bytes)
        if row.content_bytes is not None
        else None,
        "updated_at": row.updated_at,
    }


def _session_file_identity(row: SmrFile) -> dict[str, Any]:
    """Return the standard content-free File representation."""

    return {
        "file_id": str(row.file_id),
        "name": str(row.name),
        "sync_session_id": (
            str(row.sync_session_id) if row.sync_session_id is not None else None
        ),
        "project_id": str(row.project_id) if row.project_id is not None else None,
        "content_type": str(row.content_type or "").strip() or None,
        "content_sha256": str(row.content_sha256),
        "content_bytes": int(row.content_bytes),
        "metadata": dict(row.metadata_ or {}),
        "created_at": row.created_at,
        "updated_at": row.updated_at,
    }


async def _attachment_inventory(
    db: AsyncSession,
    *,
    org_id: str,
    sync_session_id: str,
    project_id: str | None,
    run_id: str | None,
    file_authorized: bool,
    project_file_authorized: bool,
) -> dict[str, Any]:
    """Separate mutable project inventory from one active run's frozen inputs.

    Project uploads are intentionally read fresh for each Local Pilot context.
    They inform later reasoning and future run admission, but are never exposed
    as though they had rematerialized an already-running workspace.
    """

    files: list[dict[str, Any]] = []
    if file_authorized:
        from services.smr import files as smr_files

        rows = await smr_files.list_files(
            db,
            org_id=org_id,
            sync_session_id=sync_session_id,
            project_id=None,
            limit=100,
        )
        files = [_session_file_identity(row) for row in rows]
    if project_id is None:
        return {
            "schema_version": "smr.intern-files.v1",
            "files": files,
            "project_inventory": [],
            "active_run_workspace": None,
            "freshness": "live" if file_authorized else "authorization_denied",
            "required_capability": (
                None if file_authorized else CapabilityOperation.FILE_READ.value
            ),
        }
    if not project_file_authorized:
        return {
            "schema_version": "smr.intern-files.v1",
            "files": files,
            "project_id": project_id,
            "project_inventory": [],
            "active_run_workspace": None,
            "freshness": "authorization_denied",
            "required_capability": CapabilityOperation.PROJECT_FILE_READ.value,
        }
    from services.smr.persistence import resource_bindings
    from services.smr.resource_catalog import (
        FILE_SCOPE_PROJECT,
        FILE_SCOPE_RUN,
        FILE_VISIBILITY_MODEL,
    )

    project_rows = await resource_bindings.list_stored_files(
        db,
        org_id=org_id,
        project_id=project_id,
        run_id=None,
        scope_kind=FILE_SCOPE_PROJECT,
        visibility=FILE_VISIBILITY_MODEL,
        limit=resource_bindings.RESOURCE_LIST_MAX_LIMIT,
    )
    project_inventory = [
        {
            **_file_identity(row),
            "scope": "project",
            "mutable_catalog_entry": True,
            "visible_to_active_run_context": True,
            "materialized_in_active_run": False,
            "materialization_effect": "future_runs_only",
        }
        for row in project_rows
    ]
    active_run_workspace: dict[str, Any] | None = None
    if run_id is not None:
        run_rows = await resource_bindings.list_stored_files(
            db,
            org_id=org_id,
            project_id=project_id,
            run_id=run_id,
            scope_kind=FILE_SCOPE_RUN,
            visibility=FILE_VISIBILITY_MODEL,
            limit=resource_bindings.RESOURCE_LIST_MAX_LIMIT,
        )
        frozen_files: list[dict[str, Any]] = []
        for row in run_rows:
            metadata = row.metadata_ if isinstance(row.metadata_, Mapping) else {}
            raw_receipt = metadata.get("smr_run_snapshot")
            receipt = dict(raw_receipt) if isinstance(raw_receipt, Mapping) else None
            frozen_files.append(
                {
                    **_file_identity(row),
                    "scope": "run",
                    "immutable_snapshot": True,
                    "snapshot_receipt": receipt,
                    "snapshot_receipt_status": "recorded" if receipt else "missing",
                }
            )
        active_run_workspace = {
            "run_id": run_id,
            "materialization": "frozen_at_run_admission",
            "live_project_file_remount": False,
            "files": frozen_files,
        }
    return {
        "schema_version": "smr.intern-local-pilot-attachments.v1",
        "files": files,
        "project_id": project_id,
        "freshness": "live_project_catalog",
        "project_inventory": project_inventory,
        "active_run_workspace": active_run_workspace,
        "boundary": (
            "Uploads after run start are visible to later Intern context but do "
            "not mutate the active materialized run. Start a new run to admit them."
        ),
    }


async def audit(
    db: AsyncSession,
    *,
    session: SmrInternSyncSession,
    lease: SmrInternLocalPilotLease,
    event_kind: str,
    detail: Mapping[str, Any] | None = None,
) -> None:
    _event(db, session=session, event_kind=event_kind, lease=lease, detail=detail)


async def detach(
    db: AsyncSession, *, session: SmrInternSyncSession, lease: SmrInternLocalPilotLease
) -> None:
    now = datetime.now(UTC)
    lease.released_at = now
    lease.expires_at = now
    lease.renewed_at = now
    _event(db, session=session, event_kind="local_pilot_detached", lease=lease)


async def has_active_lease(
    db: AsyncSession, *, sync_session_id: str, now: datetime
) -> bool:
    return bool(
        await db.scalar(
            sa.select(sa.literal(True)).where(
                sa.exists().where(
                    SmrInternLocalPilotLease.sync_session_id == sync_session_id,
                    SmrInternLocalPilotLease.released_at.is_(None),
                    SmrInternLocalPilotLease.expires_at > now,
                )
            )
        )
    )


__all__ = [
    "attach",
    "audit",
    "authenticate",
    "context",
    "detach",
    "has_active_lease",
    "require_local_pilot_enabled",
    "require_loopback",
]
