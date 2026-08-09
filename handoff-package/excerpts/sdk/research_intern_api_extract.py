"""Research Intern Sync/Async customer SDK (plus legacy Magi decision ledger)."""

from __future__ import annotations

import asyncio
import builtins
import os
import time
import warnings
from collections.abc import AsyncIterator, Iterator
from dataclasses import dataclass
from typing import Literal, cast

from synth_ai.core.contracts.json_value import JsonObject, JsonValue
from synth_ai.core.errors import TimeoutError as SynthTimeoutError
from synth_ai.core.errors import TransientServiceError
from synth_ai.core.http.async_transport import AsyncHttpTransport
from synth_ai.core.http.request import HttpRequest
from synth_ai.core.http.streaming import SseEvent
from synth_ai.core.http.transport import HttpTransport
from synth_ai.sdk.research.contracts._wire import array_value
from synth_ai.sdk.research.contracts.common import FactoryId, ProjectId
from synth_ai.sdk.research.contracts.dataset_revisions import (
    DatasetRevisionFinalizeRequest,
    DatasetRevisionFinalizeResponse,
    DatasetRevisionPreparationResponse,
    DatasetRevisionPrepareRequest,
)
from synth_ai.sdk.research.contracts.factory_role_receipts import (
    FactoryRoleReceiptMintRequest,
    FactoryRoleReceiptResponse,
)
from synth_ai.sdk.research.contracts.project_runtime import (
    ProjectComputerExecuteRequest,
    ProjectComputerInspectRequest,
    ProjectComputerLeaseAcquireRequest,
    ProjectComputerLeaseReleaseRequest,
    ProjectComputerLeaseRenewRequest,
    ProjectComputerLeaseResponse,
    ProjectComputerOperationReconcileRequest,
    ProjectRuntimeOperation,
    ProjectRuntimeOperationReceipt,
)
from synth_ai.sdk.research.contracts.research_intern import (
    DataBindingCreateRequest,
    DataBindingResponse,
    DatasetRevisionCreateRequest,
    DatasetRevisionLifecycleRequest,
    DatasetRevisionResponse,
    InternAsyncCommandKind,
    InternAsyncCommandReceipt,
    InternAsyncCommandRequest,
    InternAsyncEnsureRequest,
    InternAsyncEvent,
    InternAsyncEventPage,
    InternAsyncEventStreamEnvelope,
    InternAsyncHandoffModelRequest,
    InternAsyncHandoffReviewRequest,
    InternAsyncInstructionKind,
    InternAsyncInstructionRequest,
    InternAsyncRuntime,
    InternAsyncRuntimeBudget,
    InternCrossMetaThreadMessage,
    InternCrossMetaThreadMessageCreateRequest,
    InternMetaHandoff,
    InternMetaHandoffContinueRequest,
    InternMetaThread,
    InternMetaThreadKind,
    InternMetaThreadSegment,
    InternRuntimeOutcome,
    InternSyncCommandKind,
    InternSyncCommandReceipt,
    InternSyncCommandRequest,
    InternSyncDeployPacket,
    InternSyncEvent,
    InternSyncEventPage,
    InternSyncEventStreamEnvelope,
    InternSyncSession,
    InternSyncSessionCreateRequest,
    MagiDecisionKind,
    MagiDecisionReceiptResponse,
    MagiDecisionRequest,
    MagiMode,
    ProjectComputerCleanupReceiptResponse,
    ProjectComputerCleanupRequest,
    ProjectComputerProvisionRequest,
    ProjectComputerReplaceRequest,
    ProjectComputerResponse,
    ResearchInternAcceptanceReceiptPublicationRequest,
    ResearchInternAcceptanceReceiptPublicationResponse,
    ResearchInternEventAppendRequest,
    ResearchInternEventKind,
    ResearchInternEventResponse,
    ResearchInternEventStreamEnvelope,
    ResearchInternEventStreamEvent,
    ResearchInternEventStreamHeartbeat,
    ResearchInternEventStreamPayload,
    ResearchInternFactoryMembershipResponse,
    ResearchInternPatchRequest,
    ResearchInternProvisionRequest,
    ResearchInternResponse,
    ResearchInternSessionCloseRequest,
    ResearchInternSessionCreateRequest,
    ResearchInternSessionResponse,
    ResearchInternSessionSyncResponse,
    ResearchInternTracePublicationRequest,
    ResearchInternTracePublicationResponse,
    ResearchInternTurnControl,
    ResearchInternTurnRequest,
    ResearchInternTurnResponse,
    ResearchInternTurnStatus,
)
from synth_ai.sdk.research.intern_program import (
    AsyncInternProgramAPI,
    InternProgramAPI,
)
from synth_ai.sdk.research.operations import (
    dataset_revision_publication_operation,
    research_operation,
)

_MONOTONIC = time.monotonic
_RESEARCH_INTERN_EVENT_SEQUENCE_MAX = 2**31 - 1


def _request(
    operation_id: str,
    path: str,
    *,
    query: JsonObject | None = None,
    body: JsonObject | None = None,
) -> HttpRequest:
    return HttpRequest(
        research_operation(operation_id),
        path,
        query=query or {},
        body=body,
    )


def _dataset_publication_request(
    operation_id: str,
    path: str,
    *,
    body: JsonObject,
) -> HttpRequest:
    return HttpRequest(
        dataset_revision_publication_operation(operation_id),
        path,
        body=body,
    )


def _validate_operation_receipt(
    receipt: ProjectRuntimeOperationReceipt,
    request: (
        ProjectComputerInspectRequest
        | ProjectComputerExecuteRequest
        | ProjectComputerOperationReconcileRequest
    ),
    *,
    operation: ProjectRuntimeOperation | None,
) -> ProjectRuntimeOperationReceipt:
    if (
        receipt.operation_id != request.operation_id
        or receipt.idempotency_key != request.idempotency_key
        or receipt.resource_generation != request.expected_generation
    ):
        raise ValueError("Project Computer operation receipt identity drifted")
    if operation is not None and receipt.operation is not operation:
        raise ValueError("Project Computer operation receipt kind drifted")
    return receipt


def _memberships(value: object) -> tuple[ResearchInternFactoryMembershipResponse, ...]:
    return tuple(
        ResearchInternFactoryMembershipResponse.from_wire(item)
        for item in array_value(
            cast(JsonValue, value),
            operation_id="list_research_intern_factories",
        )
    )


def _decisions(value: object) -> tuple[MagiDecisionReceiptResponse, ...]:
    return tuple(
        MagiDecisionReceiptResponse.from_wire(item)
        for item in array_value(
            cast(JsonValue, value),
            operation_id="list_magi_decisions",
        )
    )


def _async_ensure_request_with_budget_overrides(
    request: InternAsyncEnsureRequest,
    *,
    maximum_daily_cost_cents: int | None = None,
    maximum_monthly_cost_cents: int | None = None,
) -> InternAsyncEnsureRequest:

# --- EXTRACT: Sync/Async runtime API classes (sliced for handoff) ---
class ResearchInternAPI:
    """One durable organization Intern and its many Factory memberships."""

    def __init__(
        self,
        transport: HttpTransport,
        *,
        allow_legacy_intern_sessions: bool = False,
    ) -> None:
        self._transport = transport
        self.meta_threads = ResearchInternMetaThreadsAPI(transport)
        self.sync_ = ResearchInternSyncRuntimeAPI(transport, self.meta_threads)
        self.async_ = ResearchInternAsyncRuntimeAPI(transport)
        self.program = InternProgramAPI(transport)
        self.factories = ResearchInternFactoriesAPI(transport)
        self.decisions = ResearchInternDecisionsAPI(transport)
        self.sessions = ResearchInternSessionsAPI(
            transport,
            self,
            allow_legacy=allow_legacy_intern_sessions,
        )
        self.acceptance_receipts = ResearchInternAcceptanceReceiptsAPI(transport)

    def provision(
        self,
        request: ResearchInternProvisionRequest | None = None,
    ) -> ResearchInternResponse:
        """Provision or retrieve the one Intern for the authenticated organization."""
        body = (request or ResearchInternProvisionRequest()).to_wire()
        return ResearchInternResponse.from_wire(
            self._transport.execute(
                _request(
                    "provision_research_intern",
                    "/smr/research-intern",
                    body=cast(JsonObject, body),
                )
            )
        )

    def retrieve(self) -> ResearchInternResponse:
        """Retrieve the authenticated organization's Research Intern."""
        return ResearchInternResponse.from_wire(
            self._transport.execute(_request("get_research_intern", "/smr/research-intern"))
        )

    def update(self, request: ResearchInternPatchRequest) -> ResearchInternResponse:
        """Update mutable Intern policy, attribution, state, or display fields."""
        return ResearchInternResponse.from_wire(
            self._transport.execute(
                _request(
                    "patch_research_intern",
                    "/smr/research-intern",
                    body=cast(JsonObject, request.to_wire()),
                )
            )
        )


class AsyncResearchInternAsyncRuntimeAPI:
    """Native async transport for the organization's singleton Async Intern."""

    _PATH = "/smr/research-intern/async"

    def __init__(self, transport: AsyncHttpTransport) -> None:
        self._transport = transport

    async def ensure(
        self,
        request: InternAsyncEnsureRequest,
        *,
        maximum_daily_cost_cents: int | None = None,
        maximum_monthly_cost_cents: int | None = None,
    ) -> InternAsyncRuntime:
        """Ensure the org Async Intern. Day/month kwargs override ``request.budget``."""

        ensure_request = _async_ensure_request_with_budget_overrides(
            request,
            maximum_daily_cost_cents=maximum_daily_cost_cents,
            maximum_monthly_cost_cents=maximum_monthly_cost_cents,
        )
        return InternAsyncRuntime.from_wire(
            await self._transport.execute(
                _request(
                    "ensure_intern_async_runtime",
                    self._PATH,
                    body=cast(JsonObject, ensure_request.to_wire()),
                )
            )
        )

    async def get(self) -> InternAsyncRuntime:
        return InternAsyncRuntime.from_wire(
            await self._transport.execute(_request("get_intern_async_runtime", self._PATH))
        )

    async def command(self, request: InternAsyncCommandRequest) -> InternAsyncCommandReceipt:
        receipt = InternAsyncCommandReceipt.from_wire(
            await self._transport.execute(
                _request(
                    "command_intern_async_runtime",
                    f"{self._PATH}/commands",
                    body=cast(JsonObject, request.to_wire()),
                )
            )
        )
        if receipt.command_id != request.command_id:
            raise ValueError("Async Intern command receipt identity drifted")
        return receipt

    async def handoff_model(
        self, request: InternAsyncHandoffModelRequest
    ) -> InternAsyncCommandReceipt:
        """Change Async model/effort via spine handoff (no meta-thread id)."""

        receipt = InternAsyncCommandReceipt.from_wire(
            await self._transport.execute(
                _request(
                    "handoff_intern_async_model",
                    f"{self._PATH}/handoff-model",
                    body=cast(JsonObject, request.to_wire()),
                )
            )
        )
        if receipt.command_id != request.command_id:
            raise ValueError("Async Intern handoff-model receipt identity drifted")
        return receipt

    async def seal_handoff_for_review(
        self, request: InternAsyncHandoffReviewRequest
    ) -> InternMetaHandoff:
        """Attended seal: park model/effort switch at needs_review."""

        return InternMetaHandoff.from_wire(
            await self._transport.execute(
                _request(
                    "seal_intern_async_handoff_for_review",
                    f"{self._PATH}/handoffs/review",
                    body=cast(JsonObject, request.to_wire()),
                )
            )
        )

    async def list_handoffs(self) -> tuple[InternMetaHandoff, ...]:
        return tuple(
            InternMetaHandoff.from_wire(item)
            for item in array_value(
                cast(
                    JsonValue,
                    await self._transport.execute(
                        _request("list_intern_async_handoffs", f"{self._PATH}/handoffs")
                    ),
                ),
                operation_id="list_intern_async_handoffs",
            )
        )

    async def approve_handoff(self, handoff_id: str) -> InternMetaHandoff:
        return InternMetaHandoff.from_wire(
            await self._transport.execute(
                _request(
                    "approve_intern_async_handoff",
                    f"{self._PATH}/handoffs/{handoff_id}/approve",
                    body=cast(JsonObject, {}),
                )
            )
        )

    async def reject_handoff(self, handoff_id: str) -> InternMetaHandoff:
        return InternMetaHandoff.from_wire(
            await self._transport.execute(
                _request(
                    "reject_intern_async_handoff",
                    f"{self._PATH}/handoffs/{handoff_id}/reject",
                    body=cast(JsonObject, {}),
                )
            )
        )

    async def continue_handoff(
        self,
        handoff_id: str,
        request: InternMetaHandoffContinueRequest | None = None,
    ) -> InternMetaHandoff:
        body = cast(JsonObject, request.to_wire()) if request is not None else cast(JsonObject, {})
        return InternMetaHandoff.from_wire(
            await self._transport.execute(
                _request(
                    "continue_intern_async_handoff",
                    f"{self._PATH}/handoffs/{handoff_id}/continue",
                    body=body,
                )
            )
        )

    async def send(self, request: InternAsyncInstructionRequest) -> InternAsyncCommandReceipt:
        return await self.command(request.to_command())

    async def pause(
        self,
        *,
        command_id: str,
        idempotency_key: str,
        expected_generation: int,
        reason: str,
    ) -> InternAsyncCommandReceipt:
        """Pause Async work and free the sticky host **lease** (resume reacquires).

        The shared org exe.dev VM is retained until filestore backup exists;
        pause does not wipe Sync/Async guest workspaces on that box.
        """

        return await self.command(
            InternAsyncCommandRequest(
                command_id=command_id,
                idempotency_key=idempotency_key,
                expected_generation=expected_generation,
                command_kind=InternAsyncCommandKind.PAUSE,
                payload={"reason": reason},
            )
        )

    async def resume(
        self,
        *,
        command_id: str,
        idempotency_key: str,
        expected_generation: int,
    ) -> InternAsyncCommandReceipt:
        return await self.command(
            InternAsyncCommandRequest(
                command_id=command_id,
                idempotency_key=idempotency_key,
                expected_generation=expected_generation,
                command_kind=InternAsyncCommandKind.RESUME,
            )
        )

    async def cancel(
        self,
        *,
        command_id: str,
        idempotency_key: str,
        expected_generation: int,
        reason: str,
    ) -> InternAsyncCommandReceipt:
        return await self.command(
            InternAsyncCommandRequest(
                command_id=command_id,
                idempotency_key=idempotency_key,
                expected_generation=expected_generation,
                command_kind=InternAsyncCommandKind.CANCEL,
                payload={"reason": reason},
            )
        )

    async def provide_input(
        self,
        *,
        command_id: str,
        idempotency_key: str,
        expected_generation: int,
        interaction_id: str,
        body: str,
        context: dict[str, JsonValue] | None = None,
    ) -> InternAsyncCommandReceipt:
        return await self.command(
            InternAsyncCommandRequest(
                command_id=command_id,
                idempotency_key=idempotency_key,
                expected_generation=expected_generation,
                command_kind=InternAsyncCommandKind.PROVIDE_INPUT,
                payload={
                    "interaction_id": interaction_id,
                    "body": body,
                    "context": context or {},
                },
            )
        )

    async def intervene(
        self,
        *,
        command_id: str,
        idempotency_key: str,
        expected_generation: int,
        body: str,
        context: dict[str, JsonValue] | None = None,
    ) -> InternAsyncCommandReceipt:
        return await self.send(
            InternAsyncInstructionRequest(
                command_id=command_id,
                idempotency_key=idempotency_key,
                expected_generation=expected_generation,
                instruction_kind=InternAsyncInstructionKind.INTERVENE,
                body=body,
                context=context or {},
            )
        )

    async def redirect_objective(
        self,
        *,
        command_id: str,
        idempotency_key: str,
        expected_generation: int,
        objective: str,
        context: dict[str, JsonValue] | None = None,
    ) -> InternAsyncCommandReceipt:
        return await self.send(
            InternAsyncInstructionRequest(
                command_id=command_id,
                idempotency_key=idempotency_key,
                expected_generation=expected_generation,
                instruction_kind=InternAsyncInstructionKind.REDIRECT_OBJECTIVE,
                body=objective,
                context=context or {},
            )
        )

    async def request_checkpoint(
        self,
        *,
        command_id: str,
        idempotency_key: str,
        expected_generation: int,
        context: dict[str, JsonValue] | None = None,
    ) -> InternAsyncCommandReceipt:
        return await self.send(
            InternAsyncInstructionRequest(
                command_id=command_id,
                idempotency_key=idempotency_key,
                expected_generation=expected_generation,
                instruction_kind=InternAsyncInstructionKind.REQUEST_CHECKPOINT,
                context=context or {},
            )
        )

    async def events(
        self,
        *,
        after_sequence: int = 0,
        limit: int = 100,
    ) -> InternAsyncEventPage:
        if after_sequence < 0:
            raise ValueError("after_sequence must be non-negative")
        return _intern_async_event_page(
            await self._transport.execute(
                _request(
                    "list_intern_async_runtime_events",
                    f"{self._PATH}/events",
                    query={
                        "after_sequence": after_sequence,
                        "limit": _bounded_limit(limit),
                    },
                )
            ),
            after_sequence=after_sequence,
        )

    async def stream_events(
        self,
        *,
        after_sequence: int = 0,
        timeout_seconds: float = 30.0,
    ) -> AsyncIterator[InternAsyncEvent]:
        if after_sequence < 0:
            raise ValueError("after_sequence must be non-negative")
        expected_sequence = after_sequence + 1
        runtime_id: str | None = None
        async for frame in self._transport.stream_sse(
            f"{self._PATH}/events/stream",
            params={"after_sequence": after_sequence},
            last_event_id=str(after_sequence) if after_sequence else None,
            timeout_seconds=_stream_timeout_seconds(timeout_seconds),
            operation_id="stream_intern_async_runtime_events",
        ):
            event = _intern_async_stream_event(
                frame,
                expected_sequence=expected_sequence,
                runtime_id=runtime_id,
            )
            runtime_id = event.runtime_id
            expected_sequence = event.sequence + 1
            yield event

    async def tail(
        self,
        *,
        after_sequence: int = 0,
        event_count_max: int = 1,
        timeout_seconds: float = 30.0,
    ) -> InternAsyncEventPage:
        _stream_bound(event_count_max, name="event_count_max", maximum=500)
        events: list[InternAsyncEvent] = []
        async for event in self.stream_events(
            after_sequence=after_sequence,
            timeout_seconds=timeout_seconds,
        ):
            events.append(event)
            if len(events) >= event_count_max:
                break
        return InternAsyncEventPage(
            events=tuple(events),
            next_sequence=events[-1].sequence if events else after_sequence,
        )


