"""Research Intern, Magi, Project Computer, and data revision routes."""

from __future__ import annotations

from uuid import NAMESPACE_URL, UUID, uuid5

import sqlalchemy as sa
from fastapi import APIRouter, Depends, Header, HTTPException, Path, Query, Request
from fastapi.responses import FileResponse, StreamingResponse
from sqlalchemy.ext.asyncio import AsyncSession

from core.data.db.session import session_context
from core.data.db.models.smr import SmrInternAsyncAssignment

from app.api.v1.managed_research import trace_stores as trace_store_routes
from app.core.auth import ValidatedAPIKey
from app.data.db.session import get_db
from services.smr.api_dependencies import require_operator
from services.smr.project_computers import api as project_computer_operations
from services.smr.research_intern import service
from services.intern import acceptance_fixture as intern_acceptance_fixture
from services.intern import metaharness as intern_metaharness
from services.intern import product as intern_product
from services.intern import acceptance_fixture as intern_acceptance_fixture
from services.intern import async_blockers as intern_async_blockers
from services.intern import runtime_events as intern_runtime_events
from services.intern import sync_presence as intern_sync_presence
from services.intern import local_pilot as intern_local_pilot
from services.intern import sync_refinement as intern_sync_refinement
from services.intern import harness_bundle as intern_harness_bundle
from services.intern import sync_approvals as intern_sync_approvals
from services.intern import sync_experiments as intern_sync_experiments
from services.intern import turn_projection as intern_turn_projection
from services.intern.workflow_client import (
    InternWorkflowNotRunning,
    InternWorkflowSignalError,
    ensure_runtime_started,
    signal_runtime_command,
)
from packages.intern.contracts import (
    AsyncAssignmentCreateRequest,
    AsyncAssignmentResponse,
    AsyncBlockerOpenSyncRequest,
    AsyncBlockerOpenSyncResponse,
    AsyncBlockerResponse,
    AsyncBlockerResolveRequest,
    AsyncBlockerResolveResponse,
    AsyncInstructionCommandRequest,
    AsyncRuntimeResponse,
    AsyncBlockerHandoffReceiptV1,
    AsyncRuntimeEnsureRequest,
    InternAcceptanceFixtureReceipt,
    InternAcceptanceFixtureRequest,
    InternRuntimeCommandReceipt,
    InternRuntimeCommandRequest,
    InternRuntimeEventResponse,
    InternRuntimeEventStreamEnvelope,
    InternMcpActionApprovalRequest,
    InternMcpActionResponse,
    RuntimeBinding,
    SyncRuntimeCommandRequest,
    SyncDeployPacketV1,
    SyncSessionCreateRequest,
    SyncSessionResponse,
    SyncTurnProjectionV1,
    SyncPresenceLeaseRequest,
    SyncPresenceLeaseResponse,
    SyncApprovalCardV1,
    SyncApprovalDecisionRequestV1,
    SyncExperimentLifecycleRequestV1,
    SyncExperimentV1,
)
from packages.intern.core.capabilities import CapabilityOperation
from packages.intern.sync.context import (
    VisualRepairRequestV1,
    VisualRefinementPatchV1,
    VisualRefinementResponseV1,
)
from packages.metaharness.api import (
    AsyncHandoffModelRequestV1,
    AsyncHandoffReviewRequestV1,
    CrossMetaThreadMessageCreateRequestV1,
    CrossMetaThreadMessageResponseV1,
    MetaHandoffContinueRequestV1,
    MetaHandoffResponseV1,
    MetaThreadDeliveryResponseV1,
    MetaThreadResponseV1,
    MetaThreadSegmentResponseV1,
    SyncBranchCloseRequestV1,
)
from smr.contracts.public_api.v1.project_computer_operations_api import (
    ProjectComputerExecuteRequest,
    ProjectComputerInspectRequest,
    ProjectComputerLeaseAcquireRequest,
    ProjectComputerLeaseReleaseRequest,
    ProjectComputerLeaseRenewRequest,
    ProjectComputerLeaseResponse,
    ProjectComputerOperationReconcileRequest,
)
from smr.contracts.public_api.v1.project_runtime_operations import (
    ProjectRuntimeOperationReceipt,
)
from smr.contracts.public_api.v1.research_intern import (
    DataBindingCreateRequest,
    DataBindingResponse,
    DatasetRevisionLifecycleRequest,
    DatasetRevisionResponse,
    FactoryRoleReceiptMintRequest,
    FactoryRoleReceiptResponse,
    MagiDecisionReceiptResponse,
    MagiDecisionRequest,
    ProjectComputerCleanupReceiptResponse,
    ProjectComputerCleanupRequest,
    ProjectComputerProvisionRequest,
    ProjectComputerReplaceRequest,
    ProjectComputerResponse,
    ResearchInternFactoryMembershipResponse,
    ResearchInternAcceptanceReceiptPublicationRequest,
    ResearchInternAcceptanceReceiptPublicationResponse,
    ResearchInternEventAppendRequest,
    ResearchInternEventResponse,
    ResearchInternEventStreamEnvelope,
    ResearchInternPatchRequest,
    ResearchInternProvisionRequest,
    ResearchInternResponse,
    ResearchInternSessionCloseRequest,
    ResearchInternSessionCreateRequest,
    ResearchInternSessionResponse,
    ResearchInternSessionSyncResponse,
    ResearchInternStatus,
    ResearchInternTracePublicationRequest,
    ResearchInternTracePublicationResponse,
    ResearchInternTurnRequest,
    ResearchInternTurnResponse,
)


router = APIRouter(prefix="/smr", tags=["smr", "research-intern"])


def _user_id(api_key: ValidatedAPIKey) -> str | None:
    value = getattr(api_key, "user_id", None)
    return str(value) if value else None


def _operation_principal_id(api_key: ValidatedAPIKey) -> str:
    return _user_id(api_key) or f"api-key-org:{api_key.org_id}"


async def _signal_admitted_command(
    *,
    workflow_id: str,
    command_id: str,
    expected_generation: int,
    command_kind: str,
    payload: dict[str, object],
) -> None:
    """Deliver the wakeup for a command whose durable receipt already committed.

    The receipt is the authority, so a failed wakeup is reported as its own
    typed condition instead of an opaque 500: the command remains admitted and
    reconciliation still delivers it.
    """

    try:
        await signal_runtime_command(
            workflow_id=workflow_id,
            command_id=command_id,
            expected_generation=expected_generation,
            command_kind=command_kind,
            payload=payload,
        )
    except InternWorkflowSignalError as error:
        raise HTTPException(
            status_code=409 if isinstance(error, InternWorkflowNotRunning) else 503,
            detail={
                "error_code": error.error_code,
                "command_id": error.command_id,
                "receipt_committed": error.receipt_committed,
            },
        ) from error


@router.post("/research-intern", response_model=ResearchInternResponse)
async def provision_research_intern(
    request: ResearchInternProvisionRequest,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> ResearchInternResponse:
    await require_operator(db, api_key)
    response = await service.provision_intern(
        db,
        org_id=str(api_key.org_id),
        user_id=_user_id(api_key),
        request=request,
    )
    await db.commit()
    return response


@router.get("/research-intern", response_model=ResearchInternResponse)
async def get_research_intern(
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> ResearchInternResponse:
    await require_operator(db, api_key)
    return await service.get_intern(db, org_id=str(api_key.org_id))


@router.patch("/research-intern", response_model=ResearchInternResponse)
async def patch_research_intern(
    request: ResearchInternPatchRequest,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> ResearchInternResponse:
    await require_operator(db, api_key)
    org_id = str(api_key.org_id)
    # WP3 O3: paused/archived must hard-stop Async (cancel/pause WF, release
    # sticky lease, revoke grants) — not a status-only cosmetic patch.
    hard_stop_status: ResearchInternStatus | None = None
    if (
        "status" in request.model_fields_set
        and request.status
        in {ResearchInternStatus.PAUSED, ResearchInternStatus.ARCHIVED}
    ):
        current = await service.get_intern(db, org_id=org_id)
        if current.status != request.status:
            hard_stop_status = request.status
    response = await service.patch_intern(
        db,
        org_id=org_id,
        request=request,
    )
    hard_stop_signal = None
    if hard_stop_status is not None:
        hard_stop_signal = await intern_product.hard_stop_intern_on_deactivate(
            db,
            org_id=org_id,
            intern_id=response.research_intern_id,
            status=(
                "archived"
                if hard_stop_status is ResearchInternStatus.ARCHIVED
                else "paused"
            ),
        )
    await db.commit()
    if hard_stop_signal is not None:
        await _signal_admitted_command(
            workflow_id=hard_stop_signal.workflow_id,
            command_id=hard_stop_signal.command_id,
            expected_generation=hard_stop_signal.expected_generation,
            command_kind=hard_stop_signal.command_kind,
            payload=dict(hard_stop_signal.payload),
        )
    return response


@router.post(
    "/research-intern/factories/{factory_id}",
    response_model=ResearchInternFactoryMembershipResponse,
)
async def attach_research_intern_factory(
    factory_id: str,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> ResearchInternFactoryMembershipResponse:
    await require_operator(db, api_key)
    response = await service.attach_factory(
        db,
        org_id=str(api_key.org_id),
        user_id=_user_id(api_key),
        factory_id=factory_id,
    )
    await db.commit()
    return response


@router.delete(
    "/research-intern/factories/{factory_id}",
    operation_id="detach_research_intern_factory",
)
async def detach_research_intern_factory(
    factory_id: str,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> dict:
    await require_operator(db, api_key)
    response = await service.detach_factory(
        db,
        org_id=str(api_key.org_id),
        factory_id=factory_id,
    )
    await db.commit()
    return response


@router.get(
    "/research-intern/factories",
    response_model=list[ResearchInternFactoryMembershipResponse],
)
async def list_research_intern_factories(
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> list[ResearchInternFactoryMembershipResponse]:
    await require_operator(db, api_key)
    return await service.list_factories(db, org_id=str(api_key.org_id))


@router.post(
    "/research-intern/decisions",
    response_model=MagiDecisionReceiptResponse,
)
async def record_magi_decision(
    request: MagiDecisionRequest,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> MagiDecisionReceiptResponse:
    await require_operator(db, api_key)
    response = await service.record_decision(
        db,
        org_id=str(api_key.org_id),
        user_id=_user_id(api_key),
        request=request,
    )
    await db.commit()
    return response


@router.get(
    "/research-intern/decisions",
    response_model=list[MagiDecisionReceiptResponse],
)
async def list_magi_decisions(
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
    limit: int = Query(default=100, ge=1, le=500),
) -> list[MagiDecisionReceiptResponse]:
    await require_operator(db, api_key)
    return await service.list_decisions(
        db,
        org_id=str(api_key.org_id),
        limit=limit,
    )


@router.get(
    "/research-intern/decisions/{receipt_id}",
    response_model=MagiDecisionReceiptResponse,
)
async def get_magi_decision(
    receipt_id: str,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> MagiDecisionReceiptResponse:
    await require_operator(db, api_key)
    return await service.get_decision(
        db,
        org_id=str(api_key.org_id),
        receipt_id=receipt_id,
    )


@router.get(
    "/research-intern/decision-receipts/{digest_hex}",
    response_model=MagiDecisionReceiptResponse,
)
async def get_public_magi_decision(
    digest_hex: str = Path(pattern=r"^[0-9a-f]{64}$"),
    db: AsyncSession = Depends(get_db),
) -> MagiDecisionReceiptResponse:
    return await service.get_public_decision(db, digest_hex=digest_hex)


@router.post(
    "/research-intern/sync-sessions",
    response_model=SyncSessionResponse,
    status_code=202,
)
async def create_intern_sync_session(
    request: SyncSessionCreateRequest,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> SyncSessionResponse:
    """Create a durable Sync resource, then idempotently start its workflow.

    Callers may bind an existing canonical Factory/Project/Effort/Run graph.
    Eval-specific setup belongs to the eval harness and uses the ordinary
    project, Factory, file, and session APIs before starting the walkthrough.
    """

    await require_operator(db, api_key)
    response = await intern_product.create_sync_session(
        db,
        org_id=str(api_key.org_id),
        user_id=_user_id(api_key),
        request=request,
    )
    await intern_product.admit_bootstrap_start(
        db,
        org_id=response.org_id,
        runtime_kind="sync",
        runtime_id=response.sync_session_id,
    )
    await db.commit()
    await ensure_runtime_started(
        runtime_kind="sync",
        runtime_id=response.sync_session_id,
        intern_id=response.research_intern_id,
        org_id=response.org_id,
        workflow_id=response.temporal_workflow_id,
    )
    return response


@router.get(
    "/research-intern/sync-sessions",
    response_model=list[SyncSessionResponse],
)
async def list_intern_sync_sessions(
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
    limit: int = Query(default=100, ge=1, le=500),
) -> list[SyncSessionResponse]:
    await require_operator(db, api_key)
    return await intern_product.list_sync_sessions(
        db,
        org_id=str(api_key.org_id),
        limit=limit,
    )


@router.post(
    "/research-intern/sync-sessions/{sync_session_id}/close",
    response_model=InternRuntimeCommandReceipt,
    status_code=202,
)
async def close_intern_sync_branch(
    sync_session_id: str,
    request: SyncBranchCloseRequestV1,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> InternRuntimeCommandReceipt:
    """Admit a typed close whose runtime transition seals and merges the branch."""

    await require_operator(db, api_key)
    runtime_request = InternRuntimeCommandRequest(
        command_id=request.command_id,
        idempotency_key=request.idempotency_key,
        expected_generation=request.expected_generation,
        command_kind="close",
        payload={
            "outcome": request.outcome,
            "reason": request.summary,
            "evidence_refs": [
                reference.model_dump(mode="json")
                for reference in request.evidence_references
            ],
        },
    )
    receipt, workflow_id, should_signal = await intern_product.admit_runtime_command(
        db,
        org_id=str(api_key.org_id),
        runtime_kind="sync",
        runtime_id=sync_session_id,
        request=runtime_request,
        user_id=_user_id(api_key),
    )
    await db.commit()
    if should_signal:
        await _signal_admitted_command(
            workflow_id=workflow_id,
            command_id=runtime_request.command_id,
            expected_generation=runtime_request.expected_generation,
            command_kind=runtime_request.command_kind,
            payload=runtime_request.payload,
        )
    return receipt


@router.get(
    "/research-intern/meta-threads",
    response_model=list[MetaThreadResponseV1],
)
async def list_intern_meta_threads(
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> list[MetaThreadResponseV1]:
    await require_operator(db, api_key)
    response = await intern_metaharness.list_meta_threads(
        db, org_id=str(api_key.org_id)
    )
    await db.commit()
    return response


@router.get(
    "/research-intern/meta-threads/{meta_thread_id}",
    response_model=MetaThreadResponseV1,
)
async def get_intern_meta_thread(
    meta_thread_id: str,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> MetaThreadResponseV1:
    await require_operator(db, api_key)
    return await intern_metaharness.get_meta_thread(
        db, org_id=str(api_key.org_id), meta_thread_id=meta_thread_id
    )


@router.get(
    "/research-intern/meta-threads/{meta_thread_id}/segments",
    response_model=list[MetaThreadSegmentResponseV1],
)
async def list_intern_meta_thread_segments(
    meta_thread_id: str,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> list[MetaThreadSegmentResponseV1]:
    await require_operator(db, api_key)
    return await intern_metaharness.list_segments(
        db, org_id=str(api_key.org_id), meta_thread_id=meta_thread_id
    )


@router.get(
    "/research-intern/meta-threads/{meta_thread_id}/handoffs",
    response_model=list[MetaHandoffResponseV1],
)
async def list_intern_meta_thread_handoffs(
    meta_thread_id: str,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> list[MetaHandoffResponseV1]:
    await require_operator(db, api_key)
    return await intern_metaharness.list_handoffs(
        db, org_id=str(api_key.org_id), meta_thread_id=meta_thread_id
    )


@router.post(
    "/research-intern/meta-threads/{meta_thread_id}/handoffs/{handoff_id}/approve",
    response_model=MetaHandoffResponseV1,
)
async def approve_intern_meta_thread_handoff(
    meta_thread_id: str,
    handoff_id: str,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> MetaHandoffResponseV1:
    """Attended approve: needs_review → approved (head unchanged)."""

    await require_operator(db, api_key)
    response = await intern_metaharness.approve_meta_handoff(
        db,
        org_id=str(api_key.org_id),
        meta_thread_id=meta_thread_id,
        handoff_id=handoff_id,
    )
    await db.commit()
    return response


@router.post(
    "/research-intern/meta-threads/{meta_thread_id}/handoffs/{handoff_id}/reject",
    response_model=MetaHandoffResponseV1,
)
async def reject_intern_meta_thread_handoff(
    meta_thread_id: str,
    handoff_id: str,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> MetaHandoffResponseV1:
    """Attended reject: needs_review → rejected (head unchanged)."""

    await require_operator(db, api_key)
    response = await intern_metaharness.reject_meta_handoff(
        db,
        org_id=str(api_key.org_id),
        meta_thread_id=meta_thread_id,
        handoff_id=handoff_id,
    )
    await db.commit()
    return response


@router.post(
    "/research-intern/meta-threads/{meta_thread_id}/handoffs/{handoff_id}/continue",
    response_model=MetaHandoffResponseV1,
)
async def continue_intern_meta_thread_handoff(
    meta_thread_id: str,
    handoff_id: str,
    request: MetaHandoffContinueRequestV1,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> MetaHandoffResponseV1:
    """Attended continue: approve if needed, then advance Async spine."""

    await require_operator(db, api_key)
    response = await intern_metaharness.continue_meta_handoff(
        db,
        org_id=str(api_key.org_id),
        meta_thread_id=meta_thread_id,
        handoff_id=handoff_id,
        request=request,
    )
    await db.commit()
    return response


@router.get(
    "/research-intern/meta-threads/{meta_thread_id}/deliveries",
    response_model=list[MetaThreadDeliveryResponseV1],
)
async def list_intern_meta_thread_deliveries(
    meta_thread_id: str,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> list[MetaThreadDeliveryResponseV1]:
    await require_operator(db, api_key)
    return await intern_metaharness.list_deliveries(
        db, org_id=str(api_key.org_id), meta_thread_id=meta_thread_id
    )


@router.get(
    "/research-intern/meta-threads/{meta_thread_id}/messages",
    response_model=list[CrossMetaThreadMessageResponseV1],
)
async def list_intern_meta_thread_messages(
    meta_thread_id: str,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
    limit: int = Query(default=200, ge=1, le=500),
) -> list[CrossMetaThreadMessageResponseV1]:
    await require_operator(db, api_key)
    return await intern_metaharness.list_messages(
        db,
        org_id=str(api_key.org_id),
        meta_thread_id=meta_thread_id,
        limit=limit,
    )


@router.post(
    "/research-intern/meta-threads/messages",
    response_model=CrossMetaThreadMessageResponseV1,
    status_code=201,
)
async def create_intern_meta_thread_message(
    request: CrossMetaThreadMessageCreateRequestV1,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> CrossMetaThreadMessageResponseV1:
    await require_operator(db, api_key)
    response = await intern_metaharness.create_message(
        db, org_id=str(api_key.org_id), request=request
    )
    await db.commit()
    return response


@router.get(
    "/research-intern/sync-sessions/{sync_session_id}",
    response_model=SyncSessionResponse,
)
async def get_intern_sync_session(
    sync_session_id: str,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> SyncSessionResponse:
    await require_operator(db, api_key)
    return await intern_product.get_sync_session(
        db,
        org_id=str(api_key.org_id),
        sync_session_id=sync_session_id,
    )


@router.get(
    "/research-intern/sync-sessions/{sync_session_id}/deploy-packet",
    response_model=SyncDeployPacketV1,
)
async def get_intern_sync_deploy_packet(
    sync_session_id: str,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> SyncDeployPacketV1:
    """Return a secret-free paste-ready Mode-B coding-agent handoff."""

    await require_operator(db, api_key)
    session = await intern_product.get_sync_session(
        db,
        org_id=str(api_key.org_id),
        sync_session_id=sync_session_id,
    )
    from services.intern.deploy_packet import assemble_deploy_packet

    return assemble_deploy_packet(session)


@router.get("/research-intern/sync-sessions/{sync_session_id}/usage")
async def get_intern_sync_session_usage(
    sync_session_id: str,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> dict:
    """Session resource/usage panel projection (receipts only)."""
    await require_operator(db, api_key)
    from packages.intern.usage import OriginRuntimeKind
    from services.smr.usage_projections import project_session_usage

    projection = await project_session_usage(
        db,
        org_id=str(api_key.org_id),
        origin_runtime_kind=OriginRuntimeKind.SYNC.value,
        origin_runtime_id=sync_session_id,
    )
    return projection.to_payload()


@router.get("/research-intern/async-assignments/{assignment_id}/usage")
async def get_intern_async_assignment_usage(
    assignment_id: str,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> dict:
    await require_operator(db, api_key)
    from packages.intern.usage import OriginRuntimeKind
    from services.smr.usage_projections import project_session_usage

    projection = await project_session_usage(
        db,
        org_id=str(api_key.org_id),
        origin_runtime_kind=OriginRuntimeKind.ASYNC.value,
        origin_runtime_id=assignment_id,
    )
    return projection.to_payload()


@router.get("/research-intern/usage/by-session")
async def get_org_usage_by_session(
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
    limit: int = Query(default=50, ge=1, le=200),
) -> dict:
    await require_operator(db, api_key)
    from services.smr.usage_projections import project_org_usage_by_session

    projection = await project_org_usage_by_session(
        db,
        org_id=str(api_key.org_id),
        limit=limit,
    )
    return projection.to_payload()


@router.post(
    "/research-intern/sync-sessions/{sync_session_id}/commands",
    response_model=InternRuntimeCommandReceipt,
    status_code=202,
)
async def command_intern_sync_session(
    sync_session_id: str,
    request: SyncRuntimeCommandRequest,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> InternRuntimeCommandReceipt:
    await require_operator(db, api_key)
    runtime_request = request.to_runtime_command()
    receipt, workflow_id, should_signal = await intern_product.admit_runtime_command(
        db,
        org_id=str(api_key.org_id),
        runtime_kind="sync",
        runtime_id=sync_session_id,
        request=runtime_request,
        user_id=_user_id(api_key),
    )
    await db.commit()
    if should_signal:
        await _signal_admitted_command(
            workflow_id=workflow_id,
            command_id=runtime_request.command_id,
            expected_generation=runtime_request.expected_generation,
            command_kind=runtime_request.command_kind,
            payload=runtime_request.payload,
        )
    return receipt


@router.put(
    "/research-intern/sync-sessions/{sync_session_id}/presence",
    response_model=SyncPresenceLeaseResponse,
)
async def acquire_or_renew_intern_sync_presence(
    sync_session_id: str,
    request: SyncPresenceLeaseRequest,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> SyncPresenceLeaseResponse:
    await require_operator(db, api_key)
    response = await intern_sync_presence.acquire_or_renew_presence(
        db,
        org_id=str(api_key.org_id),
        user_id=_user_id(api_key),
        sync_session_id=sync_session_id,
        request=request,
    )
    await db.commit()
    return response


@router.post(
    "/research-intern/sync-sessions/{sync_session_id}/presence/release",
    response_model=SyncPresenceLeaseResponse,
)
async def release_intern_sync_presence(
    sync_session_id: str,
    request: SyncPresenceLeaseRequest,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> SyncPresenceLeaseResponse:
    await require_operator(db, api_key)
    response = await intern_sync_presence.release_presence(
        db,
        org_id=str(api_key.org_id),
        user_id=_user_id(api_key),
        sync_session_id=sync_session_id,
        request=request,
    )
    await db.commit()
    return response


def _pilot_token(authorization: str | None) -> str:
    prefix = "Bearer "
    if not authorization or not authorization.startswith(prefix):
        raise HTTPException(
            status_code=401, detail="intern_local_pilot_credential_missing"
        )
    return authorization[len(prefix) :].strip()


@router.post("/research-intern/sync-sessions/{sync_session_id}/local-pilot/attach")
async def attach_intern_sync_local_pilot(
    sync_session_id: str,
    request: Request,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> dict:
    """Mint one short-lived org/session-bound Local Pilot capability."""
    intern_local_pilot.require_loopback(request.client.host if request.client else None)
    await require_operator(db, api_key)
    response = await intern_local_pilot.attach(
        db,
        org_id=str(api_key.org_id),
        user_id=_user_id(api_key),
        sync_session_id=sync_session_id,
    )
    await db.commit()
    return response


@router.get("/research-intern/sync-sessions/{sync_session_id}/local-pilot/context")
async def get_intern_sync_local_pilot_context(
    sync_session_id: str,
    request: Request,
    authorization: str | None = Header(default=None),
    x_intern_pilot_fence: int = Header(alias="X-Intern-Pilot-Fence"),
    db: AsyncSession = Depends(get_db),
) -> dict:
    intern_local_pilot.require_loopback(request.client.host if request.client else None)
    session, lease = await intern_local_pilot.authenticate(
        db,
        sync_session_id=sync_session_id,
        token=_pilot_token(authorization),
        fence_generation=x_intern_pilot_fence,
    )
    response = await intern_local_pilot.context(db, session=session, lease=lease)
    await intern_local_pilot.audit(
        db, session=session, lease=lease, event_kind="local_pilot_context_returned"
    )
    await db.commit()
    return response


@router.post(
    "/research-intern/sync-sessions/{sync_session_id}/local-pilot/commands",
    status_code=202,
)
async def command_intern_sync_local_pilot(
    sync_session_id: str,
    command: InternRuntimeCommandRequest,
    request: Request,
    authorization: str | None = Header(default=None),
    x_intern_pilot_fence: int = Header(alias="X-Intern-Pilot-Fence"),
    db: AsyncSession = Depends(get_db),
) -> InternRuntimeCommandReceipt:
    """Admit a model-authored result through the ordinary durable mailbox."""
    intern_local_pilot.require_loopback(request.client.host if request.client else None)
    session, lease = await intern_local_pilot.authenticate(
        db,
        sync_session_id=sync_session_id,
        token=_pilot_token(authorization),
        fence_generation=x_intern_pilot_fence,
    )
    allowed = {
        "propose_mcp_action",
        "complete_turn",
        "request_interaction",
        "fail_turn",
    }
    if command.command_kind not in allowed:
        await intern_local_pilot.audit(
            db,
            session=session,
            lease=lease,
            event_kind="local_pilot_command_failed",
            detail={
                "command_id": command.command_id,
                "error_code": "intern_local_pilot_command_not_allowed",
            },
        )
        await db.commit()
        raise HTTPException(
            status_code=422, detail="intern_local_pilot_command_not_allowed"
        )
    if command.expected_generation != session.state_generation:
        await intern_local_pilot.audit(
            db,
            session=session,
            lease=lease,
            event_kind="local_pilot_command_failed",
            detail={
                "command_id": command.command_id,
                "error_code": "intern_local_pilot_generation_stale",
            },
        )
        await db.commit()
        raise HTTPException(
            status_code=409, detail="intern_local_pilot_generation_stale"
        )
    if command.command_kind == "propose_mcp_action":
        from packages.intern.mcp import InternMcpActionProposal
        from packages.intern.smr_mcp import validate_intern_smr_tool

        try:
            proposal = InternMcpActionProposal.from_mapping(
                command.payload.get("action") or {}
            )
            if proposal.action_id != f"{command.command_id}:mcp":
                raise ValueError("intern_local_pilot_action_identity_invalid")
            validate_intern_smr_tool(
                action_kind=proposal.action_kind,
                operation_name=proposal.operation_name or "",
                requested_capability=proposal.requested_capability_operation,
            )
        except ValueError as error:
            await intern_local_pilot.audit(
                db,
                session=session,
                lease=lease,
                event_kind="local_pilot_command_failed",
                detail={"command_id": command.command_id, "error_code": str(error)},
            )
            await db.commit()
            raise HTTPException(status_code=422, detail=str(error)) from error
    from services.intern.mailbox_repository import (
        PostgresInternMailboxRepository,
        mailbox_receipt_to_response,
    )

    repository = PostgresInternMailboxRepository(db, org_id=str(session.org_id))
    receipt = await repository.admit_internal(
        runtime_kind="sync",
        runtime_id=sync_session_id,
        command_id=command.command_id,
        command_kind=command.command_kind,
        expected_generation=command.expected_generation,
        payload=command.payload,
        idempotency_key=command.idempotency_key,
    )
    await intern_local_pilot.audit(
        db,
        session=session,
        lease=lease,
        event_kind="local_pilot_command_admitted",
        detail={"command_id": command.command_id, "command_kind": command.command_kind},
    )
    await db.commit()
    try:
        await _signal_admitted_command(
            workflow_id=session.temporal_workflow_id,
            command_id=command.command_id,
            expected_generation=command.expected_generation,
            command_kind=command.command_kind,
            payload=command.payload,
        )
    except HTTPException:
        # The request transaction was committed before Temporal delivery. The
        # dependency session cannot open another transaction inside its closed
        # context-manager transaction, so delivery failure auditing uses a
        # fresh ordinary database session.
        async with session_context() as audit_db:
            session, lease = await intern_local_pilot.authenticate(
                audit_db,
                sync_session_id=sync_session_id,
                token=_pilot_token(authorization),
                fence_generation=x_intern_pilot_fence,
            )
            await intern_local_pilot.audit(
                audit_db,
                session=session,
                lease=lease,
                event_kind="local_pilot_command_delivery_failed",
                detail={"command_id": command.command_id},
            )
            await audit_db.commit()
        raise
    return mailbox_receipt_to_response(receipt)


@router.post(
    "/research-intern/sync-sessions/{sync_session_id}/local-pilot/detach",
    status_code=204,
)
async def detach_intern_sync_local_pilot(
    sync_session_id: str,
    request: Request,
    authorization: str | None = Header(default=None),
    x_intern_pilot_fence: int = Header(alias="X-Intern-Pilot-Fence"),
    db: AsyncSession = Depends(get_db),
) -> None:
    intern_local_pilot.require_loopback(request.client.host if request.client else None)
    session, lease = await intern_local_pilot.authenticate(
        db,
        sync_session_id=sync_session_id,
        token=_pilot_token(authorization),
        fence_generation=x_intern_pilot_fence,
        renew=False,
    )
    await intern_local_pilot.detach(db, session=session, lease=lease)
    await db.commit()


@router.post(
    "/research-intern/sync-sessions/{sync_session_id}/visual-refinements",
    response_model=VisualRefinementResponseV1,
)
async def refine_intern_sync_visual(
    sync_session_id: str,
    request: VisualRefinementPatchV1,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> VisualRefinementResponseV1:
    await require_operator(db, api_key)
    response = await intern_sync_refinement.refine_visual(
        db,
        org_id=str(api_key.org_id),
        user_id=_user_id(api_key),
        sync_session_id=sync_session_id,
        request=request,
    )
    await db.commit()
    return response


@router.post(
    "/research-intern/sync-sessions/{sync_session_id}/visual-repairs",
    response_model=InternRuntimeCommandReceipt,
    status_code=202,
)
async def request_intern_sync_visual_repair(
    sync_session_id: str,
    request: VisualRepairRequestV1,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> InternRuntimeCommandReceipt:
    """Ask Sync Intern to repair one exact failed Visual revision."""

    await require_operator(db, api_key)
    session = await intern_product.get_sync_session(
        db,
        org_id=str(api_key.org_id),
        sync_session_id=sync_session_id,
    )
    await intern_sync_refinement.validate_visual_repair_request(
        db,
        org_id=str(api_key.org_id),
        user_id=_user_id(api_key),
        request=request,
    )
    await intern_product.ensure_runtime_capability_operations(
        db,
        org_id=str(api_key.org_id),
        intern_id=session.research_intern_id,
        runtime_kind="sync",
        runtime_id=session.sync_session_id,
        required_operations=frozenset({CapabilityOperation.VISUAL_REPAIR}),
    )
    command_id = "visual-repair:" + str(
        uuid5(NAMESPACE_URL, f"smr:visual-repair-request:{request.idempotency_key}")
    )
    runtime_request = SyncRuntimeCommandRequest(
        command_id=command_id,
        idempotency_key=request.idempotency_key,
        expected_generation=request.expected_generation,
        command_kind="operator_message",
        execution_mode=request.execution_mode,
        visual_selection=request.target,
        payload={
            "turn_id": command_id,
            "body": (
                f"Repair failed Visual {request.target.visual_id} revision "
                f"{request.target.visual_revision}. Inspect the typed renderer "
                "diagnostic, propose a bounded presentation-only fix, and use "
                "the registered visual repair tool to create a new draft revision."
            ),
            "context": {
                "source": "visual_repair_request",
                "diagnostic": request.diagnostic.model_dump(mode="json"),
            },
        },
    ).to_runtime_command()
    receipt, workflow_id, should_signal = await intern_product.admit_runtime_command(
        db,
        org_id=str(api_key.org_id),
        runtime_kind="sync",
        runtime_id=sync_session_id,
        request=runtime_request,
        user_id=_user_id(api_key),
    )
    await db.commit()
    if should_signal:
        await _signal_admitted_command(
            workflow_id=workflow_id,
            command_id=runtime_request.command_id,
            expected_generation=runtime_request.expected_generation,
            command_kind=runtime_request.command_kind,
            payload=runtime_request.payload,
        )
    return receipt


@router.get(
    "/research-intern/sync-sessions/{sync_session_id}/turns/{turn_id}",
    response_model=SyncTurnProjectionV1,
)
async def get_intern_sync_turn_projection(
    sync_session_id: str,
    turn_id: str,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
    cursor: str | None = Query(default=None),
    limit: int = Query(default=50, ge=1, le=100),
) -> SyncTurnProjectionV1:
    await require_operator(db, api_key)
    return await intern_turn_projection.read_sync_turn_projection(
        db,
        org_id=str(api_key.org_id),
        sync_session_id=sync_session_id,
        turn_id=turn_id,
        cursor=cursor,
        limit=limit,
    )


@router.post(
    "/research-intern/async",
    response_model=AsyncRuntimeResponse,
    status_code=202,
)
async def ensure_intern_async_runtime(
    request: AsyncRuntimeEnsureRequest,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> AsyncRuntimeResponse:
    """Create the organization's one Async Intern or replay its first request."""

    await require_operator(db, api_key)
    response = await intern_product.create_async_assignment(
        db,
        org_id=str(api_key.org_id),
        user_id=_user_id(api_key),
        request=request,
        return_existing_singleton=True,
    )
    start_generation = int(response.state_generation or 0)
    start_command_id = f"{response.async_runtime_id}:start:g{start_generation}"
    await intern_product.admit_bootstrap_start(
        db,
        org_id=response.org_id,
        runtime_kind="async",
        runtime_id=response.async_runtime_id,
        start_command_id=start_command_id,
        expected_generation=start_generation,
    )
    await db.commit()
    await ensure_runtime_started(
        runtime_kind="async",
        runtime_id=response.async_runtime_id,
        intern_id=response.research_intern_id,
        org_id=response.org_id,
        workflow_id=response.temporal_workflow_id,
        start_command_id=start_command_id,
        start_expected_generation=start_generation,
    )
    return response


@router.post(
    "/research-intern/async/ensure",
    response_model=AsyncRuntimeResponse,
    status_code=202,
)
async def ensure_intern_async_runtime_alias(
    request: AsyncRuntimeEnsureRequest,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> AsyncRuntimeResponse:
    """Named ensure alias over the same organization-singleton operation."""

    return await ensure_intern_async_runtime(request, api_key, db)


@router.post(
    "/research-intern/fixtures",
    response_model=InternAcceptanceFixtureReceipt,
    status_code=201,
)
async def create_research_intern_acceptance_fixture(
    request: InternAcceptanceFixtureRequest,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> InternAcceptanceFixtureReceipt:
    """Provision a disposable, ready Factory/Project/Effort/Run fixture.

    No tribal ids: one command yields a bound, Factory-ready chain with the
    organization Intern attached, idempotent per idempotency_key within one
    test attempt. The receipt is the durable evidence of what exists.
    """

    await require_operator(db, api_key)
    receipt = await intern_acceptance_fixture.create_acceptance_fixture(
        db,
        org_id=str(api_key.org_id),
        user_id=_user_id(api_key),
        api_key=api_key,
        request=request,
    )
    await db.commit()
    return receipt


@router.get(
    "/research-intern/fixtures/{fixture_id}",
    response_model=InternAcceptanceFixtureReceipt,
)
async def get_research_intern_acceptance_fixture(
    fixture_id: str,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> InternAcceptanceFixtureReceipt:
    """Read one fixture receipt (evidence stays readable through retention)."""

    await require_operator(db, api_key)
    return await intern_acceptance_fixture.get_acceptance_fixture(
        db,
        org_id=str(api_key.org_id),
        fixture_id=fixture_id,
    )


@router.post(
    "/research-intern/fixtures/{fixture_id}:teardown",
    response_model=InternAcceptanceFixtureReceipt,
)
async def teardown_research_intern_acceptance_fixture(
    fixture_id: str,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> InternAcceptanceFixtureReceipt:
    """Explicitly tear down a fixture while preserving its evidence window."""

    await require_operator(db, api_key)
    receipt = await intern_acceptance_fixture.teardown_acceptance_fixture(
        db,
        org_id=str(api_key.org_id),
        fixture_id=fixture_id,
    )
    await db.commit()
    return receipt


@router.get(
    "/research-intern/async",
    response_model=AsyncRuntimeResponse,
)
async def get_intern_async_runtime(
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> AsyncRuntimeResponse:
    """Get the organization's one Async Intern without an assignment id."""

    await require_operator(db, api_key)
    return await intern_product.get_async_runtime(
        db,
        org_id=str(api_key.org_id),
    )


@router.post(
    "/research-intern/async/blockers/{blocker_id}/open-sync",
    response_model=AsyncBlockerOpenSyncResponse,
    status_code=202,
)
@router.post(
    "/research-intern/async/blockers/{blocker_id}:open-sync",
    response_model=AsyncBlockerOpenSyncResponse,
    status_code=202,
    include_in_schema=False,
)
async def open_intern_async_blocker_sync(
    blocker_id: str,
    request: AsyncBlockerOpenSyncRequest,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> AsyncBlockerOpenSyncResponse:
    """Open one exact-context Sync handoff without creating an approval."""

    await require_operator(db, api_key)
    response = await intern_async_blockers.open_sync_for_blocker(
        db,
        org_id=str(api_key.org_id),
        user_id=_user_id(api_key),
        blocker_id=blocker_id,
        request=request,
    )
    await intern_product.admit_bootstrap_start(
        db,
        org_id=response.sync_session.org_id,
        runtime_kind="sync",
        runtime_id=response.sync_session.sync_session_id,
    )
    await db.commit()
    await ensure_runtime_started(
        runtime_kind="sync",
        runtime_id=response.sync_session.sync_session_id,
        intern_id=response.sync_session.research_intern_id,
        org_id=response.sync_session.org_id,
        workflow_id=response.sync_session.temporal_workflow_id,
    )
    return response


@router.get(
    "/research-intern/async/blockers/{blocker_id}",
    response_model=AsyncBlockerResponse,
)
async def get_intern_async_blocker(
    blocker_id: str,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> AsyncBlockerResponse:
    """Read one exact blocker and its durable terminal resolution receipt."""

    await require_operator(db, api_key)
    return await intern_async_blockers.get_async_blocker(
        db,
        org_id=str(api_key.org_id),
        blocker_id=blocker_id,
    )


@router.post(
    "/research-intern/async/blockers/{blocker_id}/resolve",
    response_model=AsyncBlockerResolveResponse,
)
@router.post(
    "/research-intern/async/blockers/{blocker_id}:resolve",
    response_model=AsyncBlockerResolveResponse,
    include_in_schema=False,
)
async def resolve_intern_async_blocker(
    blocker_id: str,
    request: AsyncBlockerResolveRequest,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> AsyncBlockerResolveResponse:
    """Record explicit Sync disposition and durably continue Async work."""

    await require_operator(db, api_key)
    (
        response,
        workflow_id,
        should_signal,
        continuation,
    ) = await intern_async_blockers.resolve_async_blocker(
        db,
        org_id=str(api_key.org_id),
        user_id=_user_id(api_key),
        blocker_id=blocker_id,
        request=request,
    )
    await db.commit()
    if should_signal:
        await _signal_admitted_command(
            workflow_id=workflow_id,
            command_id=continuation.command_id,
            expected_generation=continuation.expected_generation,
            command_kind=continuation.command_kind,
            payload=continuation.payload,
        )
    return response


@router.post(
    "/research-intern/async/commands",
    response_model=InternRuntimeCommandReceipt,
    status_code=202,
)
async def command_intern_async_runtime(
    request: InternRuntimeCommandRequest,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> InternRuntimeCommandReceipt:
    """Admit a control command to the organization's singleton Async inbox."""

    await require_operator(db, api_key)
    runtime = await intern_product.get_async_runtime(
        db,
        org_id=str(api_key.org_id),
    )
    receipt, workflow_id, should_signal = await intern_product.admit_runtime_command(
        db,
        org_id=str(api_key.org_id),
        runtime_kind="async",
        runtime_id=runtime.async_runtime_id,
        request=request,
    )
    await db.commit()
    if should_signal:
        await _signal_admitted_command(
            workflow_id=workflow_id,
            command_id=request.command_id,
            expected_generation=request.expected_generation,
            command_kind=request.command_kind,
            payload=request.payload,
        )
    return receipt


@router.post(
    "/research-intern/async/handoff-model",
    response_model=InternRuntimeCommandReceipt,
    status_code=202,
)
async def handoff_intern_async_model(
    request: AsyncHandoffModelRequestV1,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> InternRuntimeCommandReceipt:
    """Product verb: change Async model/effort via spine handoff (no MT id).

    See: ``notes/plans/smr/intern_metaharness_control_plane.md``.
    """

    await require_operator(db, api_key)
    runtime = await intern_product.get_async_runtime(
        db,
        org_id=str(api_key.org_id),
    )
    runtime_request = InternRuntimeCommandRequest(
        command_id=request.command_id,
        idempotency_key=request.idempotency_key,
        expected_generation=request.expected_generation,
        command_kind="request_spine_handoff",
        payload={
            "summary": request.summary,
            "agent_config": request.agent_config.model_dump(mode="json"),
            "evidence_refs": [
                reference.model_dump(mode="json")
                for reference in request.evidence_references
            ],
            "require_review": request.require_review,
        },
    )
    receipt, workflow_id, should_signal = await intern_product.admit_runtime_command(
        db,
        org_id=str(api_key.org_id),
        runtime_kind="async",
        runtime_id=runtime.async_runtime_id,
        request=runtime_request,
    )
    await db.commit()
    if should_signal:
        await _signal_admitted_command(
            workflow_id=workflow_id,
            command_id=runtime_request.command_id,
            expected_generation=runtime_request.expected_generation,
            command_kind=runtime_request.command_kind,
            payload=runtime_request.payload,
        )
    return receipt


@router.post(
    "/research-intern/async/handoffs/review",
    response_model=MetaHandoffResponseV1,
    status_code=201,
)
async def seal_intern_async_handoff_for_review(
    request: AsyncHandoffReviewRequestV1,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> MetaHandoffResponseV1:
    """Attended product verb: seal Async model switch at needs_review (no MT id)."""

    await require_operator(db, api_key)
    runtime = await intern_product.get_async_runtime(
        db,
        org_id=str(api_key.org_id),
    )
    assignment = (
        await db.execute(
            sa.select(SmrInternAsyncAssignment).where(
                SmrInternAsyncAssignment.async_assignment_id
                == runtime.async_runtime_id,
                SmrInternAsyncAssignment.org_id == str(api_key.org_id),
            )
        )
    ).scalar_one()
    from packages.metaharness.contracts import AgentConfig

    handoff = await intern_metaharness.advance_async_spine_handoff(
        db,
        assignment=assignment,
        agent_config=AgentConfig(**request.agent_config.model_dump()),
        summary=request.summary,
        evidence_references=tuple(
            item.model_dump(mode="json") for item in request.evidence_references
        ),
        require_review=True,
    )
    await intern_metaharness.record_lane_delivery(
        db,
        org_id=str(api_key.org_id),
        research_intern_id=str(assignment.research_intern_id),
        meta_thread_id=str(assignment.meta_thread_id),
        kind="force_handoff",
        body=request.summary,
        runtime_command_id=handoff.handoff_id,
        idempotency_key=f"delivery:review:{request.idempotency_key}",
    )
    await db.commit()
    return handoff


@router.get(
    "/research-intern/async/handoffs",
    response_model=list[MetaHandoffResponseV1],
)
async def list_intern_async_handoffs(
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> list[MetaHandoffResponseV1]:
    """List Async spine handoffs for the org singleton (no MT id)."""

    await require_operator(db, api_key)
    runtime = await intern_product.get_async_runtime(
        db,
        org_id=str(api_key.org_id),
    )
    assignment = (
        await db.execute(
            sa.select(SmrInternAsyncAssignment).where(
                SmrInternAsyncAssignment.async_assignment_id
                == runtime.async_runtime_id,
                SmrInternAsyncAssignment.org_id == str(api_key.org_id),
            )
        )
    ).scalar_one()
    return await intern_metaharness.list_handoffs(
        db,
        org_id=str(api_key.org_id),
        meta_thread_id=str(assignment.meta_thread_id),
    )


@router.post(
    "/research-intern/async/handoffs/{handoff_id}/approve",
    response_model=MetaHandoffResponseV1,
)
async def approve_intern_async_handoff(
    handoff_id: str,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> MetaHandoffResponseV1:
    await require_operator(db, api_key)
    runtime = await intern_product.get_async_runtime(
        db,
        org_id=str(api_key.org_id),
    )
    assignment = (
        await db.execute(
            sa.select(SmrInternAsyncAssignment).where(
                SmrInternAsyncAssignment.async_assignment_id
                == runtime.async_runtime_id,
                SmrInternAsyncAssignment.org_id == str(api_key.org_id),
            )
        )
    ).scalar_one()
    response = await intern_metaharness.approve_meta_handoff(
        db,
        org_id=str(api_key.org_id),
        meta_thread_id=str(assignment.meta_thread_id),
        handoff_id=handoff_id,
    )
    await db.commit()
    return response


@router.post(
    "/research-intern/async/handoffs/{handoff_id}/reject",
    response_model=MetaHandoffResponseV1,
)
async def reject_intern_async_handoff(
    handoff_id: str,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> MetaHandoffResponseV1:
    await require_operator(db, api_key)
    runtime = await intern_product.get_async_runtime(
        db,
        org_id=str(api_key.org_id),
    )
    assignment = (
        await db.execute(
            sa.select(SmrInternAsyncAssignment).where(
                SmrInternAsyncAssignment.async_assignment_id
                == runtime.async_runtime_id,
                SmrInternAsyncAssignment.org_id == str(api_key.org_id),
            )
        )
    ).scalar_one()
    response = await intern_metaharness.reject_meta_handoff(
        db,
        org_id=str(api_key.org_id),
        meta_thread_id=str(assignment.meta_thread_id),
        handoff_id=handoff_id,
    )
    await db.commit()
    return response


@router.post(
    "/research-intern/async/handoffs/{handoff_id}/continue",
    response_model=MetaHandoffResponseV1,
)
async def continue_intern_async_handoff(
    handoff_id: str,
    request: MetaHandoffContinueRequestV1,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> MetaHandoffResponseV1:
    await require_operator(db, api_key)
    runtime = await intern_product.get_async_runtime(
        db,
        org_id=str(api_key.org_id),
    )
    assignment = (
        await db.execute(
            sa.select(SmrInternAsyncAssignment).where(
                SmrInternAsyncAssignment.async_assignment_id
                == runtime.async_runtime_id,
                SmrInternAsyncAssignment.org_id == str(api_key.org_id),
            )
        )
    ).scalar_one()
    response = await intern_metaharness.continue_meta_handoff(
        db,
        org_id=str(api_key.org_id),
        meta_thread_id=str(assignment.meta_thread_id),
        handoff_id=handoff_id,
        request=request,
    )
    await db.commit()
    return response


@router.post(
    "/research-intern/async/messages",
    response_model=InternRuntimeCommandReceipt,
    status_code=202,
)
async def message_intern_async_runtime(
    request: AsyncInstructionCommandRequest,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> InternRuntimeCommandReceipt:
    """Typed instruction alias over the singleton PostgreSQL command inbox."""

    return await command_intern_async_runtime(
        request.to_runtime_command(),
        api_key,
        db,
    )


@router.get(
    "/research-intern/async/events",
    response_model=list[InternRuntimeEventResponse],
)
async def list_intern_async_runtime_events(
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
    after_sequence: int = Query(default=0, ge=0),
    limit: int = Query(default=100, ge=1, le=500),
) -> list[InternRuntimeEventResponse]:
    """Replay the singleton Async event ledger without a runtime path id."""

    await require_operator(db, api_key)
    runtime = await intern_product.get_async_runtime(
        db,
        org_id=str(api_key.org_id),
    )
    return await intern_runtime_events.list_runtime_events(
        db,
        org_id=str(api_key.org_id),
        runtime_kind="async",
        runtime_id=runtime.async_runtime_id,
        after_sequence=after_sequence,
        limit=limit,
    )


@router.get(
    "/research-intern/async/events/stream",
    response_class=StreamingResponse,
    response_model=InternRuntimeEventStreamEnvelope,
    responses={
        200: {
            "description": "Replay-then-tail SSE over the singleton Async ledger.",
            "content": {
                "text/event-stream": {
                    "schema": {
                        "$ref": "#/components/schemas/InternRuntimeEventStreamEnvelope"
                    }
                }
            },
        }
    },
)
async def stream_intern_async_runtime_events(
    request: Request,
    api_key: ValidatedAPIKey,
    after_sequence: int = Query(default=0, ge=0),
    last_event_id: str | None = Header(default=None, alias="Last-Event-ID"),
) -> StreamingResponse:
    """Tail the singleton Async event ledger without a runtime path id."""

    async with session_context(
        "interactive", tag="intern_async_event_stream.preflight"
    ) as db:
        await require_operator(db, api_key)
        runtime = await intern_product.get_async_runtime(
            db,
            org_id=str(api_key.org_id),
        )
    return intern_runtime_events.stream_runtime_events(
        request=request,
        org_id=str(api_key.org_id),
        runtime_kind="async",
        runtime_id=runtime.async_runtime_id,
        after_sequence=after_sequence,
        last_event_id=last_event_id,
    )


@router.post(
    "/research-intern/async-assignments",
    response_model=AsyncAssignmentResponse,
    status_code=202,
    deprecated=True,
)
async def create_intern_async_assignment(
    request: AsyncAssignmentCreateRequest,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> AsyncAssignmentResponse:
    """Create leave-safe Async work before starting durable orchestration."""

    await require_operator(db, api_key)
    response = await intern_product.create_async_assignment(
        db,
        org_id=str(api_key.org_id),
        user_id=_user_id(api_key),
        request=request,
    )
    start_generation = int(response.state_generation or 0)
    start_command_id = f"{response.async_assignment_id}:start:g{start_generation}"
    await intern_product.admit_bootstrap_start(
        db,
        org_id=response.org_id,
        runtime_kind="async",
        runtime_id=response.async_assignment_id,
        start_command_id=start_command_id,
        expected_generation=start_generation,
    )
    await db.commit()
    await ensure_runtime_started(
        runtime_kind="async",
        runtime_id=response.async_assignment_id,
        intern_id=response.research_intern_id,
        org_id=response.org_id,
        workflow_id=response.temporal_workflow_id,
        start_command_id=start_command_id,
        start_expected_generation=start_generation,
    )
    return response


@router.get(
    "/research-intern/async-assignments",
    response_model=list[AsyncAssignmentResponse],
    deprecated=True,
)
async def list_intern_async_assignments(
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
    limit: int = Query(default=100, ge=1, le=500),
) -> list[AsyncAssignmentResponse]:
    await require_operator(db, api_key)
    return await intern_product.list_async_assignments(
        db,
        org_id=str(api_key.org_id),
        limit=limit,
    )


@router.get(
    "/research-intern/async-assignments/{assignment_id}",
    response_model=AsyncAssignmentResponse,
    deprecated=True,
)
async def get_intern_async_assignment(
    assignment_id: str,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> AsyncAssignmentResponse:
    await require_operator(db, api_key)
    return await intern_product.get_async_assignment(
        db,
        org_id=str(api_key.org_id),
        assignment_id=assignment_id,
    )


@router.post(
    "/research-intern/async-assignments/{assignment_id}/commands",
    response_model=InternRuntimeCommandReceipt,
    status_code=202,
    deprecated=True,
)
async def command_intern_async_assignment(
    assignment_id: str,
    request: InternRuntimeCommandRequest,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> InternRuntimeCommandReceipt:
    await require_operator(db, api_key)
    receipt, workflow_id, should_signal = await intern_product.admit_runtime_command(
        db,
        org_id=str(api_key.org_id),
        runtime_kind="async",
        runtime_id=assignment_id,
        request=request,
    )
    await db.commit()
    if should_signal:
        await _signal_admitted_command(
            workflow_id=workflow_id,
            command_id=request.command_id,
            expected_generation=request.expected_generation,
            command_kind=request.command_kind,
            payload=request.payload,
        )
    return receipt


@router.get(
    "/research-intern/runtimes/{runtime_kind}/{runtime_id}/events",
    response_model=list[InternRuntimeEventResponse],
)
async def list_intern_runtime_events(
    runtime_kind: str,
    runtime_id: str,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
    after_sequence: int = Query(default=0, ge=0),
    limit: int = Query(default=100, ge=1, le=500),
) -> list[InternRuntimeEventResponse]:
    await require_operator(db, api_key)
    return await intern_runtime_events.list_runtime_events(
        db,
        org_id=str(api_key.org_id),
        runtime_kind=runtime_kind,
        runtime_id=runtime_id,
        after_sequence=after_sequence,
        limit=limit,
    )


@router.get(
    "/research-intern/runtimes/{runtime_kind}/{runtime_id}/events/stream",
    response_class=StreamingResponse,
    response_model=InternRuntimeEventStreamEnvelope,
    responses={
        200: {
            "description": "Replay-then-tail SSE over durable Intern events.",
            "content": {
                "text/event-stream": {
                    "schema": {
                        "$ref": "#/components/schemas/InternRuntimeEventStreamEnvelope"
                    }
                }
            },
        }
    },
)
async def stream_intern_runtime_events(
    runtime_kind: str,
    runtime_id: str,
    request: Request,
    api_key: ValidatedAPIKey,
    after_sequence: int = Query(default=0, ge=0),
    last_event_id: str | None = Header(default=None, alias="Last-Event-ID"),
) -> StreamingResponse:
    async with session_context(
        "interactive", tag="intern_runtime_event_stream.preflight"
    ) as db:
        await require_operator(db, api_key)
        await intern_runtime_events.require_runtime_access(
            db,
            org_id=str(api_key.org_id),
            runtime_kind=runtime_kind,
            runtime_id=runtime_id,
        )
    return intern_runtime_events.stream_runtime_events(
        request=request,
        org_id=str(api_key.org_id),
        runtime_kind=runtime_kind,
        runtime_id=runtime_id,
        after_sequence=after_sequence,
        last_event_id=last_event_id,
    )


@router.get(
    "/research-intern/runtimes/{runtime_kind}/{runtime_id}/mcp-actions",
    response_model=list[InternMcpActionResponse],
)
async def list_intern_runtime_mcp_actions(
    runtime_kind: str,
    runtime_id: str,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
    limit: int = Query(default=100, ge=1, le=500),
) -> list[InternMcpActionResponse]:
    await require_operator(db, api_key)
    return await intern_product.list_mcp_actions(
        db,
        org_id=str(api_key.org_id),
        runtime_kind=runtime_kind,
        runtime_id=runtime_id,
        limit=limit,
    )


@router.post(
    "/research-intern/runtimes/{runtime_kind}/{runtime_id}/mcp-actions/"
    "{action_id}/approval",
    response_model=InternMcpActionResponse,
)
async def decide_intern_runtime_mcp_action_approval(
    runtime_kind: str,
    runtime_id: str,
    action_id: str,
    request: InternMcpActionApprovalRequest,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> InternMcpActionResponse:
    await require_operator(db, api_key)
    response = await intern_product.decide_mcp_action_approval(
        db,
        org_id=str(api_key.org_id),
        user_id=_user_id(api_key),
        runtime_kind=runtime_kind,
        runtime_id=runtime_id,
        action_id=action_id,
        request=request,
    )
    await db.commit()
    return response


@router.get(
    "/research-intern/sync-sessions/{sync_session_id}/async-blocker-handoff",
    response_model=AsyncBlockerHandoffReceiptV1,
)
async def get_sync_async_blocker_handoff(
    sync_session_id: str,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> AsyncBlockerHandoffReceiptV1:
    await require_operator(db, api_key)
    return await intern_async_blockers.handoff_receipt_for_sync_session(
        db,
        org_id=str(api_key.org_id),
        sync_session_id=sync_session_id,
    )


@router.get(
    "/research-intern/sync-sessions/{sync_session_id}/approvals",
    response_model=list[SyncApprovalCardV1],
)
async def list_sync_approval_cards(
    sync_session_id: str,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> list[SyncApprovalCardV1]:
    await require_operator(db, api_key)
    return await intern_sync_approvals.list_sync_approval_cards(
        db,
        org_id=str(api_key.org_id),
        sync_session_id=sync_session_id,
    )


@router.get(
    "/research-intern/sync-approvals/{approval_id}",
    response_model=SyncApprovalCardV1,
)
async def get_sync_approval_card(
    approval_id: str,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> SyncApprovalCardV1:
    await require_operator(db, api_key)
    return await intern_sync_approvals.approval_card(
        db,
        org_id=str(api_key.org_id),
        approval_id=approval_id,
    )


@router.post(
    "/research-intern/sync-approvals/{approval_id}/decision",
    response_model=SyncApprovalCardV1,
)
async def decide_sync_approval_card(
    approval_id: str,
    request: SyncApprovalDecisionRequestV1,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> SyncApprovalCardV1:
    await require_operator(db, api_key)
    card = await intern_sync_approvals.decide_sync_approval(
        db,
        org_id=str(api_key.org_id),
        user_id=_user_id(api_key),
        approval_id=approval_id,
        decision=request.decision,
        comment=request.comment,
        amendment=request.amendment,
    )
    await db.commit()
    return card


@router.get(
    "/research-intern/sync-sessions/{sync_session_id}/experiments",
    response_model=list[SyncExperimentV1],
)
async def list_sync_session_experiments(
    sync_session_id: str,
    api_key: ValidatedAPIKey,
    limit: int = Query(default=100, ge=1, le=256),
    db: AsyncSession = Depends(get_db),
) -> list[SyncExperimentV1]:
    """List the experiment ledger scoped to one Sync session."""

    await require_operator(db, api_key)
    return await intern_sync_experiments.list_session_experiments(
        db,
        org_id=str(api_key.org_id),
        sync_session_id=sync_session_id,
        limit=limit,
    )


@router.post(
    "/research-intern/sync-sessions/{sync_session_id}/experiments/"
    "{experiment_id}/lifecycle",
    response_model=SyncExperimentV1,
)
async def transition_sync_session_experiment(
    sync_session_id: str,
    experiment_id: str,
    request: SyncExperimentLifecycleRequestV1,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> SyncExperimentV1:
    """Operator promote/stop decision on one session-scoped experiment.

    Presence-enforced like sync approvals. ``promote`` keeps exactly one
    promoted experiment per session (the previous one is marked
    ``superseded``); ``stop`` records the honest no-lift terminal. Typed
    conditions: ``intern_experiment_not_found``,
    ``intern_experiment_wrong_session``, ``intern_experiment_not_scored``
    (promote without ``allow_unscored``), ``intern_experiment_not_promotable``
    and ``intern_experiment_not_stoppable``.
    """

    await require_operator(db, api_key)
    response = await intern_sync_experiments.apply_lifecycle_transition(
        db,
        org_id=str(api_key.org_id),
        user_id=_user_id(api_key),
        sync_session_id=sync_session_id,
        experiment_id=experiment_id,
        action=request.action,
        allow_unscored=request.allow_unscored,
        comment=request.comment,
    )
    await db.commit()
    return response


@router.get(
    "/research-intern/sync-sessions/{sync_session_id}/harness-bundle",
    response_class=FileResponse,
)
async def download_sync_session_harness_bundle(
    sync_session_id: str,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> FileResponse:
    """Download the take-home harness bundle zip for one Sync session (F6.1).

    Streams a byte-stable zip (workspace tree at the pinned snapshot, winning
    candidate, experiment ledger, result summary, README, MANIFEST) whose
    sha256 digest is served in the ``X-Harness-Bundle-Digest`` header.
    Bundles are S3-backed and require no open session, so closed sessions
    read back identically (F6.4).
    """

    await require_operator(db, api_key)
    bundle = await intern_harness_bundle.assemble_session_harness_bundle(
        db,
        org_id=str(api_key.org_id),
        sync_session_id=sync_session_id,
    )
    return intern_harness_bundle.bundle_file_response(bundle)


@router.post(
    "/research-intern/sessions",
    response_model=ResearchInternSessionResponse,
    deprecated=True,
)
async def create_research_intern_session(
    request: ResearchInternSessionCreateRequest,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> ResearchInternSessionResponse:
    await require_operator(db, api_key)
    response = await service.create_session(
        db,
        org_id=str(api_key.org_id),
        user_id=_user_id(api_key),
        request=request,
    )
    await db.commit()
    return response


@router.get(
    "/research-intern/sessions",
    response_model=list[ResearchInternSessionResponse],
    deprecated=True,
)
async def list_research_intern_sessions(
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
    limit: int = Query(default=100, ge=1, le=500),
) -> list[ResearchInternSessionResponse]:
    await require_operator(db, api_key)
    return await service.list_sessions(
        db,
        org_id=str(api_key.org_id),
        limit=limit,
    )


@router.get(
    "/research-intern/sessions/{session_id}",
    response_model=ResearchInternSessionResponse,
    deprecated=True,
)
async def get_research_intern_session(
    session_id: str,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> ResearchInternSessionResponse:
    await require_operator(db, api_key)
    return await service.get_session(
        db,
        org_id=str(api_key.org_id),
        session_id=session_id,
    )


@router.post(
    "/research-intern/sessions/{session_id}/events",
    response_model=ResearchInternEventResponse,
    deprecated=True,
)
async def append_research_intern_session_event(
    session_id: str,
    request: ResearchInternEventAppendRequest,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> ResearchInternEventResponse:
    await require_operator(db, api_key)
    response = await service.append_session_event(
        db,
        org_id=str(api_key.org_id),
        user_id=_user_id(api_key),
        session_id=session_id,
        request=request,
    )
    await db.commit()
    return response


@router.get(
    "/research-intern/sessions/{session_id}/events",
    response_model=list[ResearchInternEventResponse],
    deprecated=True,
)
async def list_research_intern_session_events(
    session_id: str,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
    after_sequence: int = Query(
        default=0,
        ge=0,
        le=service.RESEARCH_INTERN_EVENT_SEQUENCE_MAX,
    ),
    limit: int = Query(default=100, ge=1, le=500),
) -> list[ResearchInternEventResponse]:
    await require_operator(db, api_key)
    return await service.list_session_events(
        db,
        org_id=str(api_key.org_id),
        session_id=session_id,
        after_sequence=after_sequence,
        limit=limit,
    )


@router.get(
    "/research-intern/sessions/{session_id}/events/stream",
    response_class=StreamingResponse,
    response_model=ResearchInternEventStreamEnvelope,
    responses={
        200: {
            "description": (
                "Replay-then-tail SSE stream over durable Research Intern events."
            ),
            "content": {
                "text/event-stream": {
                    "schema": {
                        "$ref": (
                            "#/components/schemas/ResearchInternEventStreamEnvelope"
                        )
                    }
                }
            },
        }
    },
    deprecated=True,
)
async def stream_research_intern_session_events(
    session_id: str,
    request: Request,
    api_key: ValidatedAPIKey,
    after_sequence: int = Query(
        default=0,
        ge=0,
        le=service.RESEARCH_INTERN_EVENT_SEQUENCE_MAX,
    ),
    last_event_id: str | None = Header(default=None, alias="Last-Event-ID"),
) -> StreamingResponse:
    """Read-only stream; the durable writer pushes projection and `/sync` reconciles."""

    return await service.stream_session_events(
        session_id=session_id,
        request=request,
        api_key=api_key,
        after_sequence=after_sequence,
        last_event_id=last_event_id,
    )


@router.post(
    "/research-intern/sessions/{session_id}/sync",
    response_model=ResearchInternSessionSyncResponse,
    deprecated=True,
)
async def sync_research_intern_session(
    session_id: str,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
    limit: int = Query(default=200, ge=1, le=500),
) -> ResearchInternSessionSyncResponse:
    await require_operator(db, api_key)
    response = await service.sync_session_from_runtime(
        db,
        org_id=str(api_key.org_id),
        session_id=session_id,
        limit=limit,
    )
    await db.commit()
    return response


@router.post(
    "/research-intern/sessions/{session_id}/turns",
    response_model=ResearchInternTurnResponse,
    deprecated=True,
)
async def create_research_intern_session_turn(
    session_id: str,
    request: ResearchInternTurnRequest,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> ResearchInternTurnResponse:
    await require_operator(db, api_key)
    # Establish a clean runtime cursor before accepting a new operator turn.
    # Any projected backlog commits independently, so optimistic concurrency
    # correctly asks stale clients to refresh instead of misattributing an old
    # assistant completion to the new turn.
    while True:
        synchronized = await service.sync_session_from_runtime(
            db,
            org_id=str(api_key.org_id),
            session_id=session_id,
            limit=500,
        )
        await db.commit()
        if not synchronized.has_more:
            break
    begun = await service.begin_session_turn(
        db,
        org_id=str(api_key.org_id),
        user_id=_user_id(api_key),
        session_id=session_id,
        request=request,
    )
    # The operator event, optional Magi actuation receipt, and ManderQueue
    # delivery are durable before the bounded reply wait begins.
    await db.commit()
    return await service.wait_for_session_turn(
        db,
        org_id=str(api_key.org_id),
        session_id=session_id,
        request=request,
        begun=begun,
    )


@router.post(
    "/research-intern/sessions/{session_id}/close",
    response_model=ResearchInternSessionResponse,
    deprecated=True,
)
async def close_research_intern_session(
    session_id: str,
    request: ResearchInternSessionCloseRequest,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> ResearchInternSessionResponse:
    await require_operator(db, api_key)
    response = await service.close_session(
        db,
        org_id=str(api_key.org_id),
        user_id=_user_id(api_key),
        session_id=session_id,
        request=request,
    )
    await db.commit()
    return response


@router.post(
    "/research-intern/sessions/{session_id}/trace:publish",
    response_model=ResearchInternTracePublicationResponse,
    deprecated=True,
)
async def publish_research_intern_session_trace(
    session_id: str,
    http_request: Request,
    request: ResearchInternTracePublicationRequest,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> ResearchInternTracePublicationResponse:
    """Promote a terminal Intern event chain through Factory Trace Store authority."""

    await require_operator(db, api_key)
    try:
        response = await service.publish_session_trace(
            db,
            org_id=str(api_key.org_id),
            session_id=session_id,
            request=request,
            trace_store=trace_store_routes.resolve_trace_store_service(http_request),
        )
        await db.commit()
        return response
    except trace_store_routes.TRACE_STORE_OPERATION_ERRORS as error:
        raise trace_store_routes.translate_trace_store_error(error) from error


@router.put(
    "/research-intern/acceptance-receipts/{digest_hex}",
    response_model=ResearchInternAcceptanceReceiptPublicationResponse,
    deprecated=True,
)
async def publish_research_intern_acceptance_receipt(
    request: ResearchInternAcceptanceReceiptPublicationRequest,
    api_key: ValidatedAPIKey,
    digest_hex: str = Path(pattern=r"^[0-9a-f]{64}$"),
    db: AsyncSession = Depends(get_db),
) -> ResearchInternAcceptanceReceiptPublicationResponse:
    await require_operator(db, api_key)
    response = await service.publish_acceptance_receipt(
        db,
        org_id=str(api_key.org_id),
        digest_hex=digest_hex,
        request=request,
    )
    await db.commit()
    return response


@router.get(
    "/research-intern/acceptance-receipts",
    response_model=list[ResearchInternAcceptanceReceiptPublicationResponse],
    deprecated=True,
)
async def list_research_intern_acceptance_receipts(
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
    candidate_id: str | None = Query(default=None, min_length=1),
    lane: str | None = Query(default=None, pattern=r"^[a-z0-9][a-z0-9._-]{0,127}$"),
    limit: int = Query(default=100, ge=1, le=500),
) -> list[ResearchInternAcceptanceReceiptPublicationResponse]:
    await require_operator(db, api_key)
    return await service.list_acceptance_receipts(
        db,
        org_id=str(api_key.org_id),
        candidate_id=candidate_id,
        lane=lane,
        limit=limit,
    )


@router.get(
    "/research-intern/acceptance-receipts/{digest_hex}",
    response_model=ResearchInternAcceptanceReceiptPublicationResponse,
    deprecated=True,
)
async def get_public_research_intern_acceptance_receipt(
    digest_hex: str = Path(pattern=r"^[0-9a-f]{64}$"),
    db: AsyncSession = Depends(get_db),
) -> ResearchInternAcceptanceReceiptPublicationResponse:
    return await service.get_public_acceptance_receipt(
        db,
        digest_hex=digest_hex,
    )


@router.post(
    "/projects/{project_id}/computer",
    response_model=ProjectComputerResponse,
)
async def provision_project_computer(
    project_id: str,
    request: ProjectComputerProvisionRequest,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> ProjectComputerResponse:
    await require_operator(db, api_key)
    response = await service.provision_project_computer(
        db,
        org_id=str(api_key.org_id),
        project_id=project_id,
        request=request,
    )
    await db.commit()
    return response


@router.get(
    "/projects/{project_id}/computer",
    response_model=ProjectComputerResponse,
)
async def get_project_computer(
    project_id: str,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
    factory_id: str = Query(min_length=1),
) -> ProjectComputerResponse:
    await require_operator(db, api_key)
    return await service.get_project_computer(
        db,
        org_id=str(api_key.org_id),
        factory_id=factory_id,
        project_id=project_id,
    )


@router.post(
    "/projects/{project_id}/computer/inspect",
    response_model=ProjectRuntimeOperationReceipt,
)
async def inspect_project_computer(
    project_id: str,
    request: ProjectComputerInspectRequest,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> ProjectRuntimeOperationReceipt:
    await require_operator(db, api_key)
    return await project_computer_operations.inspect_project_computer(
        db,
        org_id=str(api_key.org_id),
        project_id=project_id,
        principal_id=_operation_principal_id(api_key),
        request=request,
    )


@router.post(
    "/projects/{project_id}/computer/execute",
    response_model=ProjectRuntimeOperationReceipt,
)
async def execute_project_computer(
    project_id: str,
    request: ProjectComputerExecuteRequest,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> ProjectRuntimeOperationReceipt:
    await require_operator(db, api_key)
    return await project_computer_operations.execute_project_computer(
        db,
        org_id=str(api_key.org_id),
        project_id=project_id,
        principal_id=_operation_principal_id(api_key),
        request=request,
    )


@router.post(
    "/projects/{project_id}/computer/operations/reconcile",
    response_model=ProjectRuntimeOperationReceipt,
)
async def reconcile_project_computer_operation(
    project_id: str,
    request: ProjectComputerOperationReconcileRequest,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> ProjectRuntimeOperationReceipt:
    await require_operator(db, api_key)
    return await project_computer_operations.reconcile_project_computer_operation(
        db,
        org_id=str(api_key.org_id),
        project_id=project_id,
        principal_id=_operation_principal_id(api_key),
        request=request,
    )


@router.post(
    "/projects/{project_id}/computer/leases",
    response_model=ProjectComputerLeaseResponse,
)
async def acquire_project_computer_lease(
    project_id: str,
    request: ProjectComputerLeaseAcquireRequest,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> ProjectComputerLeaseResponse:
    await require_operator(db, api_key)
    return await project_computer_operations.acquire_project_computer_lease(
        db,
        org_id=str(api_key.org_id),
        project_id=project_id,
        principal_id=_operation_principal_id(api_key),
        request=request,
    )


@router.post(
    "/projects/{project_id}/computer/leases/{lease_id}/renew",
    response_model=ProjectComputerLeaseResponse,
)
async def renew_project_computer_lease(
    project_id: str,
    lease_id: str,
    request: ProjectComputerLeaseRenewRequest,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> ProjectComputerLeaseResponse:
    await require_operator(db, api_key)
    return await project_computer_operations.renew_project_computer_lease(
        db,
        org_id=str(api_key.org_id),
        project_id=project_id,
        lease_id=lease_id,
        principal_id=_operation_principal_id(api_key),
        request=request,
    )


@router.post(
    "/projects/{project_id}/computer/leases/{lease_id}/release",
    response_model=ProjectComputerLeaseResponse,
)
async def release_project_computer_lease(
    project_id: str,
    lease_id: str,
    request: ProjectComputerLeaseReleaseRequest,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> ProjectComputerLeaseResponse:
    await require_operator(db, api_key)
    return await project_computer_operations.release_project_computer_lease(
        db,
        org_id=str(api_key.org_id),
        project_id=project_id,
        lease_id=lease_id,
        principal_id=_operation_principal_id(api_key),
        request=request,
    )


@router.post(
    "/projects/{project_id}/computer/replace",
    response_model=ProjectComputerResponse,
)
async def replace_project_computer(
    project_id: str,
    request: ProjectComputerReplaceRequest,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> ProjectComputerResponse:
    await require_operator(db, api_key)
    response = await service.replace_project_computer(
        db,
        org_id=str(api_key.org_id),
        project_id=project_id,
        request=request,
    )
    await db.commit()
    return response


@router.delete(
    "/projects/{project_id}/computer",
    response_model=ProjectComputerResponse,
)
async def retire_project_computer(
    project_id: str,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
    factory_id: str = Query(min_length=1),
) -> ProjectComputerResponse:
    await require_operator(db, api_key)
    response = await service.retire_project_computer(
        db,
        org_id=str(api_key.org_id),
        factory_id=factory_id,
        project_id=project_id,
    )
    await db.commit()
    return response


@router.post(
    "/research-intern/factories/{factory_id}/project-computers/cleanup",
    response_model=ProjectComputerCleanupReceiptResponse,
)
async def cleanup_factory_project_computers(
    factory_id: str,
    request: ProjectComputerCleanupRequest,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> ProjectComputerCleanupReceiptResponse:
    await require_operator(db, api_key)
    response = await service.cleanup_factory_project_computers(
        db,
        org_id=str(api_key.org_id),
        factory_id=factory_id,
        request=request,
    )
    await db.commit()
    return response


@router.post(
    "/research-intern/factories/{factory_id}/role-receipts",
    response_model=FactoryRoleReceiptResponse,
)
async def mint_factory_role_receipt(
    factory_id: str,
    request: FactoryRoleReceiptMintRequest,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> FactoryRoleReceiptResponse:
    await require_operator(db, api_key)
    response = await service.mint_factory_role_receipt(
        db,
        org_id=str(api_key.org_id),
        factory_id=factory_id,
        request=request,
    )
    await db.commit()
    return response


@router.get(
    "/research-intern/factories/{factory_id}/role-receipts",
    response_model=list[FactoryRoleReceiptResponse],
)
async def list_factory_role_receipts(
    factory_id: str,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
    candidate_id: str | None = Query(default=None, min_length=1),
) -> list[FactoryRoleReceiptResponse]:
    await require_operator(db, api_key)
    return await service.list_factory_role_receipts(
        db,
        org_id=str(api_key.org_id),
        factory_id=factory_id,
        candidate_id=candidate_id,
    )


@router.post(
    "/projects/{project_id}/data-bindings",
    response_model=DataBindingResponse,
)
async def create_data_binding(
    project_id: str,
    request: DataBindingCreateRequest,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> DataBindingResponse:
    await require_operator(db, api_key)
    response = await service.create_data_binding(
        db,
        org_id=str(api_key.org_id),
        project_id=project_id,
        request=request,
    )
    await db.commit()
    return response


@router.get(
    "/projects/{project_id}/data-bindings",
    response_model=list[DataBindingResponse],
)
async def list_data_bindings(
    project_id: str,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> list[DataBindingResponse]:
    await require_operator(db, api_key)
    return await service.list_data_bindings(
        db,
        org_id=str(api_key.org_id),
        project_id=project_id,
    )


@router.post(
    "/projects/{project_id}/data-bindings/{data_binding_id}/revisions",
    response_model=DatasetRevisionResponse,
)
async def create_dataset_revision(
    project_id: str,
    data_binding_id: UUID,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> DatasetRevisionResponse:
    await require_operator(db, api_key)
    raise HTTPException(
        status_code=410,
        detail=(
            "Direct DatasetRevision sealing is retired. Use revisions:prepare "
            "and revision-preparations/{preparation_id}:finalize."
        ),
    )


@router.post(
    (
        "/projects/{project_id}/data-bindings/{data_binding_id}/revisions/"
        "{dataset_revision_id}/lifecycle"
    ),
    response_model=DatasetRevisionResponse,
)
async def transition_dataset_revision_lifecycle(
    project_id: str,
    data_binding_id: UUID,
    dataset_revision_id: UUID,
    request: DatasetRevisionLifecycleRequest,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> DatasetRevisionResponse:
    await require_operator(db, api_key)
    response = await service.transition_dataset_revision_lifecycle(
        db,
        org_id=str(api_key.org_id),
        project_id=project_id,
        data_binding_id=str(data_binding_id),
        dataset_revision_id=str(dataset_revision_id),
        request=request,
    )
    await db.commit()
    return response


@router.get(
    "/projects/{project_id}/data-bindings/{data_binding_id}/revisions",
    response_model=list[DatasetRevisionResponse],
)
async def list_dataset_revisions(
    project_id: str,
    data_binding_id: UUID,
    api_key: ValidatedAPIKey,
    db: AsyncSession = Depends(get_db),
) -> list[DatasetRevisionResponse]:
    await require_operator(db, api_key)
    return await service.list_dataset_revisions(
        db,
        org_id=str(api_key.org_id),
        project_id=project_id,
        data_binding_id=str(data_binding_id),
    )


__all__ = ["router"]
