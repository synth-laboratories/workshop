"""Versioned product contracts for separate Sync and Async resources."""

from __future__ import annotations

from datetime import datetime
import json
from typing import Any, Literal

from pydantic import BaseModel, ConfigDict, Field, field_validator, model_validator

from packages.intern.async_.phase import (
    ResumeKind,
    RuntimePhaseWire,
    StopReason,
    WaitProducer,
)
from packages.intern.async_.state import AsyncInstructionKind, AsyncStatus
from packages.intern.core.model import (
    EvidenceReadiness,
    ExternalExecutionStatus,
    RuntimeOutcome,
)
from packages.intern.sync.state import SyncStatus
from packages.intern.sync.context import (
    SyncExecutionMode,
    VisualSelectionContextV1,
)
from packages.intern.mcp import (
    InternMcpActionKind,
    InternMcpActionStatus,
    InternMcpMethod,
)
from packages.intern.resources import (
    ProducedResourceKind as ProducedResourceKind,
    ProducedResourceReferenceV1,
    admit_produced_resource_citations,
)


class _StrictContract(BaseModel):
    model_config = ConfigDict(extra="forbid")


class RuntimeBinding(_StrictContract):
    factory_id: str | None = None
    project_id: str | None = None
    effort_id: str | None = None
    run_id: str | None = None


class SyncObjectiveBoundsV1(_StrictContract):
    """Operator-visible bounds enforced by a Sync task driver."""

    schema_version: Literal["smr.intern-sync-objective-bounds.v1"] = (
        "smr.intern-sync-objective-bounds.v1"
    )
    task_id: str | None = Field(default=None, min_length=1, max_length=512)
    candidate_timeout_seconds: int | None = Field(default=None, ge=1, le=86_400)


class InternRuntimeCommandRequest(_StrictContract):
    command_id: str = Field(min_length=1, max_length=512)
    idempotency_key: str = Field(min_length=1, max_length=512)
    expected_generation: int = Field(ge=0)
    command_kind: str = Field(min_length=1, max_length=128)
    payload: dict[str, Any] = Field(default_factory=dict)


class SyncRuntimeCommandRequest(_StrictContract):
    """Sync-only command envelope with typed semantic turn context."""

    command_id: str = Field(min_length=1, max_length=512)
    idempotency_key: str = Field(min_length=1, max_length=512)
    expected_generation: int = Field(ge=0)
    command_kind: str = Field(min_length=1, max_length=128)
    payload: dict[str, Any] = Field(default_factory=dict)
    execution_mode: SyncExecutionMode = SyncExecutionMode.STANDARD
    visual_selection: VisualSelectionContextV1 | None = None
    mode: Literal["sync", "async"] = "sync"
    evidence_refs: tuple[ProducedResourceReferenceV1, ...] = Field(
        default=(), max_length=128
    )

    @model_validator(mode="after")
    def selection_only_on_turns(self) -> "SyncRuntimeCommandRequest":
        if self.visual_selection is not None and self.command_kind not in {
            "submit_turn",
            "operator_message",
            "intervene",
            "answer_interaction",
        }:
            raise ValueError("visual selection is only valid on Sync turn commands")
        return self

    def to_runtime_command(self) -> InternRuntimeCommandRequest:
        payload = dict(self.payload)
        payload["execution_mode"] = self.execution_mode.value
        if self.visual_selection is not None:
            payload["visual_selection"] = self.visual_selection.model_dump(mode="json")
        payload["mode"] = self.mode
        payload["evidence_refs"] = [
            reference.model_dump(mode="json") for reference in self.evidence_refs
        ]
        return InternRuntimeCommandRequest(
            command_id=self.command_id,
            idempotency_key=self.idempotency_key,
            expected_generation=self.expected_generation,
            command_kind=self.command_kind,
            payload=payload,
        )


class AsyncInstructionPayload(_StrictContract):
    body: str | None = Field(default=None, max_length=20_000)
    context: dict[str, Any] = Field(default_factory=dict)


class AsyncInstructionCommandRequest(_StrictContract):
    """Typed envelope for the optional ``/async/messages`` command alias."""

    command_id: str = Field(min_length=1, max_length=512)
    idempotency_key: str = Field(min_length=1, max_length=512)
    expected_generation: int = Field(ge=0)
    command_kind: AsyncInstructionKind
    payload: AsyncInstructionPayload

    @model_validator(mode="after")
    def validate_instruction_body(self) -> "AsyncInstructionCommandRequest":
        body = self.payload.body.strip() if self.payload.body else None
        if self.command_kind is AsyncInstructionKind.REQUEST_CHECKPOINT:
            if body:
                raise ValueError("request_checkpoint does not accept a body")
        elif body is None:
            raise ValueError("instruction body is required")
        return self

    def to_runtime_command(self) -> InternRuntimeCommandRequest:
        return InternRuntimeCommandRequest(
            command_id=self.command_id,
            idempotency_key=self.idempotency_key,
            expected_generation=self.expected_generation,
            command_kind=self.command_kind.value,
            payload=self.payload.model_dump(),
        )


class InternRuntimeCommandReceipt(_StrictContract):
    schema_version: Literal["smr.intern-runtime-command-receipt.v1"] = (
        "smr.intern-runtime-command-receipt.v1"
    )
    command_id: str
    runtime_kind: Literal["sync", "async"]
    runtime_id: str
    status: Literal[
        "received",
        "delivered",
        "applied",
        "noop",
        "refused",
        "superseded",
        "conflict",
    ]
    previous_generation: int = Field(ge=0)
    state_generation: int = Field(ge=0)
    decision_code: str
    created_at: datetime
    # Durable actuation receipt (schema smr.intern-command-actuation.v1),
    # present once the command reached a final decision. Written in the same
    # transaction that committed the state transition.
    actuation: dict[str, Any] | None = None
    # True when this response replays an already-admitted command
    # (idempotent retry); the stored receipt/actuation is immutable.
    duplicate: bool = False


class InternRuntimeEventResponse(_StrictContract):
    schema_version: Literal["smr.intern-runtime-event.v1"] = (
        "smr.intern-runtime-event.v1"
    )
    event_id: str
    runtime_kind: Literal["sync", "async"]
    runtime_id: str
    sequence: int = Field(ge=1)
    previous_state_generation: int = Field(ge=0)
    state_generation: int = Field(ge=0)
    event_kind: str
    command_id: str
    payload: dict[str, Any]
    created_at: datetime


class InternRuntimeEventStreamEnvelope(_StrictContract):
    schema_version: Literal["smr.intern-runtime-event-stream.v1"] = (
        "smr.intern-runtime-event-stream.v1"
    )
    event: InternRuntimeEventResponse


class SyncSessionCreateRequest(_StrictContract):
    # Empty until the first operator send_message; Intern MCP owns Effort objectives.
    objective: str = Field(default="", max_length=20_000)
    idempotency_key: str = Field(min_length=1, max_length=512)
    binding: RuntimeBinding = Field(default_factory=RuntimeBinding)
    metadata: dict[str, Any] = Field(default_factory=dict)
    execution_mode: SyncExecutionMode = SyncExecutionMode.STANDARD
    objective_bounds: SyncObjectiveBoundsV1 | None = None
    # Sync gates write operations on an operator decision by default because an
    # operator is present. Set False for an unattended Sync session (harness,
    # eval lane, CI) where nobody will ever decide and the gate can only stall.
    # Async is ungated by default and ignores this field.
    require_operator_approval: bool = True


class SyncDeployPacketEndpointV1(_StrictContract):
    purpose: Literal[
        "upload_files",
        "upload_workspace",
        "confirm_push",
        "assign_container",
        "container_health",
    ]
    method: Literal["GET", "POST"]
    path: str = Field(min_length=1, max_length=2_048)


class SyncDeployPacketV1(_StrictContract):
    """Secret-free, paste-ready handoff for a Mode-B coding agent."""

    schema_version: Literal["smr.intern-sync-deploy-packet.v1"] = (
        "smr.intern-sync-deploy-packet.v1"
    )
    sync_session_id: str
    project_id: str
    auth_scheme: Literal["bearer"] = "bearer"
    auth_env_var: Literal["SYNTH_API_KEY"] = "SYNTH_API_KEY"
    endpoints: tuple[SyncDeployPacketEndpointV1, ...]
    done_signal: Literal["workspace_confirm_push"] = "workspace_confirm_push"
    image_kind: Literal["project_default"] = "project_default"
    instructions: str = Field(min_length=1, max_length=20_000)


class SyncTracePublicationReceiptV1(_StrictContract):
    """Read-only receipt for the server-side terminal Trace V5 publication.

    The backend publishes the durable Sync event chain as genuine Trace V5 as
    an effect of the session reaching a terminal state. Clients never mint this
    record; they observe it on the session projection after close.
    """

    schema_version: Literal["smr.intern-sync-trace-publication.v1"] = (
        "smr.intern-sync-trace-publication.v1"
    )
    status: Literal["published", "failed"]
    sync_session_id: str
    runtime_kind: Literal["sync"] = "sync"
    state_generation: int = Field(ge=1)
    event_count: int | None = Field(default=None, ge=1)
    trace_id: str | None = None
    trace_digest: str | None = Field(default=None, pattern=r"^sha256:[0-9a-f]{64}$")
    capture_id: str | None = None
    bundle_id: str | None = None
    manifest_digest: str | None = Field(default=None, pattern=r"^sha256:[0-9a-f]{64}$")
    publication_id: str | None = None
    error_code: str | None = None
    attempted_at: datetime
    published_at: datetime | None = None

    @model_validator(mode="after")
    def require_receipt_or_typed_condition(
        self,
    ) -> "SyncTracePublicationReceiptV1":
        if self.status == "published":
            missing = [
                name
                for name in (
                    "event_count",
                    "trace_id",
                    "trace_digest",
                    "capture_id",
                    "bundle_id",
                    "manifest_digest",
                    "publication_id",
                    "published_at",
                )
                if getattr(self, name) is None
            ]
            if missing:
                raise ValueError(f"published trace receipt is missing {missing}")
            if self.error_code is not None:
                raise ValueError("published trace receipt cannot carry an error")
        elif self.error_code is None:
            raise ValueError("failed trace publication requires a typed error_code")
        return self


class SyncSessionVisualSummaryV1(_StrictContract):
    """Minimal pointer to one Visual bound to this Sync session (F3.3).

    Enough for the cockpit to find the session's score-series Visual without
    walking ``/smr/visuals``; the full content stays behind the registry read
    surface. ``series_point_count`` is only set for readable score-series
    Visuals.
    """

    schema_version: Literal["smr.intern-sync-session-visual.v1"] = (
        "smr.intern-sync-session-visual.v1"
    )
    visual_id: str = Field(min_length=1, max_length=255)
    kind: Literal["score_series", "visual"]
    title: str = Field(min_length=1, max_length=500)
    current_revision: int | None = Field(default=None, ge=1)
    series_point_count: int | None = Field(default=None, ge=0)
    updated_at: datetime


SyncWorkspaceSnapshotKind = Literal[
    "internal_git_head",
    "stored_file_digest_set",
    "internal_git_head_with_stored_files",
]


class SyncWorkspaceSnapshotIdentityV1(_StrictContract):
    """Provable workspace identity captured at run-trigger time (F5.4).

    ``internal_git_head`` cites the project's internal-git HEAD commit (the
    same identity workspace push confirmation freezes); it is preferred.
    ``stored_file_digest_set`` cites a digest over the model-visible,
    project-scope stored-file inventory when no internal-git identity exists.
    ``internal_git_head_with_stored_files`` binds both authorities so a file
    attached after the latest git commit cannot disappear from the receipt.
    """

    schema_version: Literal["smr.intern-sync-workspace-snapshot.v1"] = (
        "smr.intern-sync-workspace-snapshot.v1"
    )
    kind: SyncWorkspaceSnapshotKind
    commit_sha: str | None = Field(default=None, min_length=1, max_length=255)
    inventory_digest: str | None = Field(default=None, pattern=r"^sha256:[0-9a-f]{64}$")
    file_count: int | None = Field(default=None, ge=1)
    captured_at: datetime

    @model_validator(mode="after")
    def require_identity_per_kind(self) -> "SyncWorkspaceSnapshotIdentityV1":
        if self.kind == "internal_git_head" and not self.commit_sha:
            raise ValueError("internal_git_head snapshot requires commit_sha")
        if self.kind == "stored_file_digest_set" and (
            not self.inventory_digest or not self.file_count
        ):
            raise ValueError(
                "stored_file_digest_set snapshot requires the inventory digest "
                "and file count"
            )
        if self.kind == "internal_git_head_with_stored_files" and (
            not self.commit_sha or not self.inventory_digest or not self.file_count
        ):
            raise ValueError(
                "internal_git_head_with_stored_files snapshot requires commit_sha, "
                "the inventory digest, and file count"
            )
        return self


class SyncWorkspaceRunReceiptV1(_StrictContract):
    """Durable proof of the project workspace used to trigger one run."""

    schema_version: Literal["smr.intern-sync-workspace-run.v1"] = (
        "smr.intern-sync-workspace-run.v1"
    )
    run_id: str = Field(min_length=1, max_length=255)
    sync_session_id: str
    project_id: str
    experiment_id: str | None = Field(default=None, min_length=1, max_length=255)
    workspace: SyncWorkspaceSnapshotIdentityV1
    recorded_at: datetime


SyncExperimentKind = Literal["baseline", "experiment"]

# Full vocabulary for the experiment ledger. ``proposed -> approved/rejected``
# is owned by the operator disposition gate; ``running`` is stamped by the run
# trigger; ``scored``/``promoted``/``stopped``/``superseded`` transitions are
# reserved for the swarm scoring loop (typed-unsupported in v1 -- no server
# path mints them yet and no client mutation exists).
SyncExperimentStatus = Literal[
    "proposed",
    "approved",
    "rejected",
    "superseded",
    "running",
    "scored",
    "promoted",
    "stopped",
]

SyncExperimentDispositionDecision = Literal["approve", "deny", "edit"]


class SyncExperimentAmendmentV1(_StrictContract):
    """Operator-amended proposal fields carried by an ``edit`` decision."""

    title: str | None = Field(default=None, min_length=1, max_length=500)
    hypothesis: str | None = Field(default=None, min_length=1, max_length=10_000)
    proposed_diff_ref: str | None = Field(default=None, min_length=1, max_length=1_024)

    def amended_fields(self) -> dict[str, str]:
        return {
            field_name: value
            for field_name, value in (
                ("title", self.title),
                ("hypothesis", self.hypothesis),
                ("proposed_diff_ref", self.proposed_diff_ref),
            )
            if value is not None
        }


class SyncExperimentDispositionV1(_StrictContract):
    """Durable record of the exact operator decision on one proposal.

    ``edit`` decisions record both the ``original`` proposal fields and the
    ``amendment`` that was applied (approved-with-amendment).
    """

    schema_version: Literal["smr.sync-experiment-disposition.v1"] = (
        "smr.sync-experiment-disposition.v1"
    )
    decision: SyncExperimentDispositionDecision
    decided_by: str = Field(min_length=1, max_length=255)
    comment: str | None = Field(default=None, max_length=4_000)
    approval_id: str = Field(min_length=1, max_length=255)
    decided_at: datetime
    original: dict[str, Any] = Field(default_factory=dict)
    amendment: dict[str, Any] = Field(default_factory=dict)


class SyncExperimentScoresV1(_StrictContract):
    """Server-computed score summary; populated by the scoring loop."""

    schema_version: Literal["smr.sync-experiment-scores.v1"] = (
        "smr.sync-experiment-scores.v1"
    )
    baseline: float | None = None
    result: float | None = None
    delta: float | None = None


SyncExperimentLifecycleAction = Literal["promote", "stop"]


class SyncExperimentWorkspacePinV1(_StrictContract):
    """Exact workspace identity a promotion or download is pinned to.

    Captured from the latest successful run-owned workspace archive at the
    moment the pin is minted; every field is copied from that immutable row so
    the pin stays meaningful after project pointers move on.
    """

    schema_version: Literal["smr.sync-experiment-workspace-pin.v1"] = (
        "smr.sync-experiment-workspace-pin.v1"
    )
    workspace_archive_id: str | None = Field(default=None, min_length=1, max_length=512)
    run_id: str | None = Field(default=None, min_length=1, max_length=255)
    commit_sha: str | None = Field(default=None, min_length=1, max_length=255)
    storage_bucket: str | None = Field(default=None, min_length=1, max_length=255)
    archive_key: str | None = Field(default=None, min_length=1, max_length=1024)
    digest: str | None = Field(default=None, pattern=r"^sha256:[0-9a-f]{64}$")
    size_bytes: int | None = Field(default=None, ge=0)


class SyncExperimentPromotionReceiptV1(_StrictContract):
    """Durable record of the operator promote/stop decision on one experiment.

    Promotion is an operator decision on their own session, not evidence
    minting: scores stay server-owned in ``result_summary``, so the receipt
    records the decision, the presence-checked operator, and workspace pin --
    never score values.
    """

    schema_version: Literal["smr.sync-experiment-promotion.v1"] = (
        "smr.sync-experiment-promotion.v1"
    )
    action: SyncExperimentLifecycleAction
    decided_by: str = Field(min_length=1, max_length=255)
    comment: str | None = Field(default=None, max_length=4_000)
    decided_at: datetime
    allow_unscored: bool = False
    scored: bool
    superseded_experiment_id: str | None = None
    workspace_pin: SyncExperimentWorkspacePinV1 | None = None


class SyncExperimentLifecycleRequestV1(_StrictContract):
    """Operator promote/stop request (presence-enforced on the session)."""

    action: SyncExperimentLifecycleAction
    # Promoting an experiment that has no server-owned scores yet is an
    # explicit operator call, never a default.
    allow_unscored: bool = False
    comment: str | None = Field(default=None, max_length=4_000)


class SyncExperimentV1(_StrictContract):
    """First-class Sync-facing experiment ledger object."""

    schema_version: Literal["smr.sync-experiment.v1"] = "smr.sync-experiment.v1"
    experiment_id: str
    project_id: str
    sync_session_id: str | None = None
    title: str
    hypothesis: str
    kind: SyncExperimentKind
    # Reference to the candidate under test (file path/SHA or content digest).
    # File bodies are never stored on the ledger record.
    proposed_diff_ref: str | None = None
    candidate_schema_family: str | None = None
    status: SyncExperimentStatus
    disposition: SyncExperimentDispositionV1 | None = None
    promotion: SyncExperimentPromotionReceiptV1 | None = None
    run_ids: tuple[str, ...] = Field(default=(), max_length=256)
    scores: SyncExperimentScoresV1 | None = None
    visual_ids: tuple[str, ...] = Field(default=(), max_length=256)
    trace_publication_ids: tuple[str, ...] = Field(default=(), max_length=256)
    created_at: datetime
    updated_at: datetime


class SyncExperimentSummaryV1(_StrictContract):
    """Ledger summary row projected onto the Sync session."""

    schema_version: Literal["smr.sync-experiment-summary.v1"] = (
        "smr.sync-experiment-summary.v1"
    )
    experiment_id: str
    title: str
    kind: SyncExperimentKind
    status: SyncExperimentStatus
    scores: SyncExperimentScoresV1 | None = None
    updated_at: datetime


class SyncExperimentCreateRequestV1(_StrictContract):
    """Proposal creation request (Intern runtime or operator)."""

    sync_session_id: str | None = Field(default=None, min_length=1, max_length=255)
    title: str = Field(min_length=1, max_length=500)
    hypothesis: str = Field(min_length=1, max_length=10_000)
    kind: SyncExperimentKind = "experiment"
    proposed_diff_ref: str | None = Field(default=None, min_length=1, max_length=1_024)
    candidate_schema_family: str | None = Field(
        default=None, min_length=1, max_length=255
    )
    idempotency_key: str | None = Field(default=None, min_length=1, max_length=128)


class HarnessBundleFileV1(_StrictContract):
    """One bundle member with its exact content digest."""

    path: str = Field(min_length=1, max_length=1024)
    sha256: str = Field(pattern=r"^[0-9a-f]{64}$")
    size_bytes: int = Field(ge=0)


class HarnessBundleManifestV1(_StrictContract):
    """Digest-parity anchor carried inside every harness/bulk bundle (F6.2).

    Deliberately timestamp-free: the manifest -- and therefore the byte-stable
    bundle digest -- is a pure function of the pin and the ledger state, so
    two builds of the same pin verify against the same digest everywhere
    (SDK/MCP/UI).
    """

    schema_version: Literal["smr.harness-bundle-manifest.v1"] = (
        "smr.harness-bundle-manifest.v1"
    )
    bundle_kind: Literal["harness_bundle", "project_bulk"]
    project_id: str
    sync_session_id: str | None = None
    pin: SyncExperimentWorkspacePinV1
    winner_experiment_id: str | None = None
    winner_source: Literal["promoted", "best_so_far"] | None = None
    candidate_source_path: str | None = None
    # Every bundle member except MANIFEST.json itself (a digest cannot
    # contain itself); the bundle's own digest travels in the response
    # headers.
    files: tuple[HarnessBundleFileV1, ...] = Field(default=(), max_length=100_000)


class SyncSessionResponse(_StrictContract):
    schema_version: Literal["smr.intern-sync-session.v1"] = "smr.intern-sync-session.v1"
    sync_session_id: str
    research_intern_id: str
    org_id: str
    objective: str
    status: SyncStatus
    state_generation: int = Field(ge=0)
    last_event_sequence: int = Field(ge=0)
    binding: RuntimeBinding
    metadata: dict[str, Any] = Field(default_factory=dict)
    objective_bounds: SyncObjectiveBoundsV1 | None = None
    pending_turn_id: str | None = None
    pending_action_id: str | None = None
    pending_interaction_id: str | None = None
    outcome: RuntimeOutcome | None = None
    failure_code: str | None = None
    execution_mode: SyncExecutionMode
    execution_profile_id: Literal["intern_sync"] = "intern_sync"
    temporal_workflow_id: str
    trace_publication: SyncTracePublicationReceiptV1 | None = None
    visuals: tuple[SyncSessionVisualSummaryV1, ...] = Field(default=(), max_length=64)
    workspace_run_receipts: tuple[SyncWorkspaceRunReceiptV1, ...] = Field(
        default=(), max_length=256
    )
    experiments: tuple[SyncExperimentSummaryV1, ...] = Field(default=(), max_length=256)
    # Coordinate-free download hint (F6): true when a pinned workspace archive
    # exists to assemble the take-home harness bundle from. Computed live on
    # the single-session read; never stored.
    harness_bundle_available: bool = False
    created_at: datetime
    updated_at: datetime
    closed_at: datetime | None = None


class SyncPresenceLeaseRequest(_StrictContract):
    connection_id: str = Field(min_length=1, max_length=512)
    connection_generation: int = Field(ge=1)


class SyncPresenceLeaseResponse(_StrictContract):
    schema_version: Literal["smr.intern-sync-presence.v1"] = (
        "smr.intern-sync-presence.v1"
    )
    lease_id: str
    sync_session_id: str
    org_id: str
    user_id: str
    connection_id: str
    connection_generation: int = Field(ge=1)
    acquired_at: datetime
    renewed_at: datetime
    expires_at: datetime
    interactive_approval_available: bool


class InternResourcePresentationV1(_StrictContract):
    """Non-mutating request for a frontend to focus an exact resource."""

    schema_version: Literal["smr.intern-resource-presentation.v1"] = (
        "smr.intern-resource-presentation.v1"
    )
    resource_kind: Literal[
        "experiment",
        "visual_revision",
        "run",
        "report",
        "evidence",
    ]
    project_id: str = Field(min_length=1, max_length=512)
    resource_id: str = Field(min_length=1, max_length=512)
    revision: int | None = Field(default=None, ge=1)

    @model_validator(mode="after")
    def exact_revision_only_for_visual(self) -> "InternResourcePresentationV1":
        if self.resource_kind == "visual_revision" and self.revision is None:
            raise ValueError("visual revision presentation requires revision")
        if self.resource_kind != "visual_revision" and self.revision is not None:
            raise ValueError("revision is only valid for visual revision presentation")
        return self


class InternDeliverableProjectionV1(_StrictContract):
    """Explicit request to cross channel 2 into a customer-visible WorkProduct."""

    schema_version: Literal["smr.intern-deliverable-projection.v1"] = (
        "smr.intern-deliverable-projection.v1"
    )
    kind: Literal["report"] = "report"
    title: str = Field(min_length=1, max_length=1_000)
    project_id: str | None = Field(default=None, min_length=1, max_length=512)
    run_id: str | None = Field(default=None, min_length=1, max_length=512)
    artifact_id: str | None = Field(default=None, min_length=1, max_length=512)
    readiness: Literal["viewable", "downloadable"] = "viewable"
    metadata: dict[str, Any] = Field(default_factory=dict)


_EVIDENCE_CONSULTED_MAX = 128


class InternWorkSummaryContentV1(_StrictContract):
    approach_summary: str = Field(min_length=1, max_length=2_000)
    evidence_consulted: tuple[ProducedResourceReferenceV1, ...] = Field(
        default=(), max_length=_EVIDENCE_CONSULTED_MAX
    )
    outcome: str | None = Field(default=None, max_length=4_000)
    limitations: tuple[str, ...] = Field(default=(), max_length=64)
    unresolved_questions: tuple[str, ...] = Field(default=(), max_length=64)
    suggested_next_step: str | None = Field(default=None, max_length=2_000)
    deliverable: InternDeliverableProjectionV1 | None = None

    @field_validator("evidence_consulted", mode="before")
    @classmethod
    def _admit_evidence_citations(cls, value: Any) -> Any:
        """Admit the actor's citations one at a time, not as an all-or-nothing list.

        This model is validated directly on both the Sync FINAL and the Async
        checkpoint paths, so it is the boundary an actor's evidence list crosses
        -- ``packages.intern.agent`` never sees ``evidence_consulted`` as a
        separate field. Delegating admission keeps one owner for the decision;
        the argument for refusing per element rather than failing the summary
        lives on ``admit_produced_resource_citations``.
        """

        if not isinstance(value, (list, tuple)):
            return value
        return admit_produced_resource_citations(
            value,
            field_name="evidence_consulted",
            maximum=_EVIDENCE_CONSULTED_MAX,
        )

    @model_validator(mode="after")
    def bound_product_summary(self) -> "InternWorkSummaryContentV1":
        if any(len(item) > 2_000 for item in self.limitations):
            raise ValueError("work summary limitation is too long")
        if any(len(item) > 2_000 for item in self.unresolved_questions):
            raise ValueError("work summary unresolved question is too long")
        encoded = json.dumps(
            self.model_dump(mode="json"),
            sort_keys=True,
            separators=(",", ":"),
            allow_nan=False,
        ).encode()
        if len(encoded) > 64 * 1_024:
            raise ValueError("work summary is too large")
        return self


class InternWorkSummaryV1(InternWorkSummaryContentV1):
    schema_version: Literal["smr.intern-work-summary.v1"] = "smr.intern-work-summary.v1"
    summary_id: str
    sync_session_id: str
    turn_id: str
    source_event_id: str | None = None
    source_invocation_id: str
    status: Literal["working", "completed", "blocked", "failed"]
    created_at: datetime
    completed_at: datetime | None = None


class InternToolCallReceiptV1(_StrictContract):
    schema_version: Literal["smr.intern-tool-call-receipt.v1"] = (
        "smr.intern-tool-call-receipt.v1"
    )
    group_id: str
    invocation_id: str
    action_id: str
    effect_id: str
    turn_id: str
    server_name: str
    method: str
    operation_name: str | None = None
    requested_capability: str
    arguments: dict[str, Any] | None = None
    argument_digest: str
    lifecycle: Literal[
        "proposed",
        "authorized",
        "running",
        "succeeded",
        "failed",
        "refused",
    ]
    produced_resources: tuple[ProducedResourceReferenceV1, ...] = ()
    application_receipt_ids: tuple[str, ...] = ()
    usage: dict[str, Any] = Field(default_factory=dict)
    result_code: str | None = None
    error_code: str | None = None
    idempotency_identity: str
    started_at: datetime | None = None
    completed_at: datetime | None = None


class InternToolCallGroupV1(_StrictContract):
    group_id: str
    turn_id: str
    actions: tuple[InternToolCallReceiptV1, ...]


class SyncTurnProjectionV1(_StrictContract):
    schema_version: Literal["smr.intern-sync-turn-projection.v1"] = (
        "smr.intern-sync-turn-projection.v1"
    )
    sync_session_id: str
    turn_id: str
    messages: tuple[InternRuntimeEventResponse, ...]
    work_summaries: tuple[InternWorkSummaryV1, ...]
    tool_call_groups: tuple[InternToolCallGroupV1, ...]
    next_cursor: str | None = None


class AsyncRuntimeBudget(_StrictContract):
    maximum_cost_cents: int | None = Field(default=None, ge=0)
    maximum_daily_cost_cents: int | None = Field(default=None, ge=0)
    maximum_monthly_cost_cents: int | None = Field(default=None, ge=0)
    maximum_cycles: int | None = Field(default=None, ge=1)
    maximum_concurrent_runs: int = Field(default=1, ge=1)


class AsyncRuntimeSpend(_StrictContract):
    """Observed Async burn against the ceilings in ``AsyncRuntimeBudget``.

    # See: intern_async_24_7_change_scope.md (WP2 S1/S2, Downstream acceptance)

    ``AsyncRuntimeBudget`` states only what the org is *allowed* to spend. A
    caller watching for the day/month trip to ``BlockAsync`` needs the other
    half — what has actually been spent — and needs it in the same projection,
    because the trip is a comparison of the two. Totals include both billed LLM
    spend and the ``intern_sticky_host`` wall-time idle burn (WP2 S2), which is
    the component most likely to exhaust a ceiling while nobody is watching.
    """

    lifetime_cents: int = Field(default=0, ge=0)
    daily_cents: int = Field(default=0, ge=0)
    monthly_cents: int = Field(default=0, ge=0)
    lifetime_ceiling_cents: int | None = Field(default=None, ge=0)
    daily_ceiling_cents: int | None = Field(default=None, ge=0)
    monthly_ceiling_cents: int | None = Field(default=None, ge=0)
    #: ``None`` when the corresponding ceiling is unset (no ceiling, no remainder).
    daily_remaining_cents: int | None = None
    monthly_remaining_cents: int | None = None
    daily_exhausted: bool = False
    monthly_exhausted: bool = False
    #: The ``BlockAsync`` code this burn already trips, if any — the same
    #: vocabulary ``async_spend_ceiling_blocker_code`` maps capability denials to.
    blocked_reason: str | None = None


class AsyncRuntimeHostLease(_StrictContract):
    """Sticky exe.dev host binding for the org's Async Intern (WP2 S2).

    Reported because the lease is what *accrues* the idle burn above: a lease
    held across a quiet night is the usual explanation for a ceiling trip that
    no single cycle appears to account for.
    """

    bound: bool = False
    lease_id: str | None = None
    #: Monotonic per-``sticky_key`` bind counter (DB check: ``generation >= 1``).
    #: Together with ``lease_id`` this is the *only* honest pause/resume
    #: discriminator. The exe.dev VM name is a deterministic sha256 of the
    #: sticky session key
    #: (``packages/persistent_compute/exe_dev/client.py::vm_name_from_session``),
    #: so a correctly reacquired host keeps the *same* ``host_id`` — an
    #: acceptance check that demands a changed ``host_id`` fails a correct
    #: system. ``None`` only when no lease row exists for the org.
    generation: int | None = Field(default=None, ge=1)
    sticky_key: str | None = None
    host_kind: str | None = None
    host_id: str | None = None
    status: str | None = None
    acquired_at: datetime | None = None
    renewed_at: datetime | None = None
    expires_at: datetime | None = None
    released_at: datetime | None = None
    release_reason: str | None = None
    reused: bool = False
    cents_per_hour: int = Field(default=0, ge=0)


class AsyncRuntimeEnsureRequest(_StrictContract):
    # Empty until the first async_.send; Effort objectives live on Intern MCP.
    objective: str = Field(default="", max_length=20_000)
    idempotency_key: str = Field(min_length=1, max_length=512)
    binding: RuntimeBinding = Field(default_factory=RuntimeBinding)
    budget: AsyncRuntimeBudget = Field(default_factory=AsyncRuntimeBudget)
    metadata: dict[str, Any] = Field(default_factory=dict)
    # Bounded wait for the explicit Factory-ready condition before binding.
    # 0 (default) refuses immediately with a typed readiness report.
    factory_ready_wait_seconds: int = Field(default=0, ge=0, le=60)


AsyncAssignmentBudget = AsyncRuntimeBudget
AsyncAssignmentCreateRequest = AsyncRuntimeEnsureRequest


class AsyncCheckpointResponse(_StrictContract):
    checkpoint_id: str
    summary: str
    evidence_refs: list[ProducedResourceReferenceV1]
    unresolved_questions: list[str]
    next_action: str | None = None
    created_at: datetime
    research_records: list[dict[str, Any]] = Field(default_factory=list)


class AsyncBlockerResponse(_StrictContract):
    schema_version: Literal["smr.intern-async-blocker.v1"] = (
        "smr.intern-async-blocker.v1"
    )
    blocker_id: str | None = None
    code: str
    message: str
    retryable: bool
    next_retry_at: datetime | None = None
    operator_action_required: bool = False
    status: Literal["open", "handoff_opened", "resolved", "superseded"] | None = None
    binding: RuntimeBinding | None = None
    action_schema_version: str | None = None
    action_kind: str | None = None
    action_digest: str | None = Field(default=None, pattern=r"^[0-9a-f]{64}$")
    summary: str | None = Field(default=None, max_length=2_000)
    rationale: str | None = Field(default=None, max_length=4_000)
    preauthorization_rule: str | None = None
    evidence_resources: tuple[ProducedResourceReferenceV1, ...] = ()
    required_operator_capability: str | None = None
    sync_session_id: str | None = None
    resolution_receipt: dict[str, Any] | None = None
    created_at: datetime | None = None
    updated_at: datetime | None = None
    resolved_at: datetime | None = None


class ResumeConditionResponse(_StrictContract):
    """Wire form of packages.intern.async_.phase.ResumeCondition.

    # See: INTERN_ASYNC_RUNTIME_PHASE_REFACTOR_HANDOFF_2026-08-08.md §6.3
    """

    kind: ResumeKind
    wake_at: datetime | None = None
    watchdog_at: datetime | None = None
    interaction_ids: tuple[str, ...] = ()
    effort_ids: tuple[str, ...] = ()
    external_ref: str | None = None
    producer: WaitProducer | None = None


class AsyncBlockerOpenSyncRequest(_StrictContract):
    idempotency_key: str = Field(min_length=1, max_length=512)


class AsyncBlockerHandoffContextV1(_StrictContract):
    schema_version: Literal["smr.intern-async-blocker-handoff-context.v1"] = (
        "smr.intern-async-blocker-handoff-context.v1"
    )
    blocker_id: str
    async_assignment_id: str
    binding: RuntimeBinding
    action_schema_version: str
    action_kind: str
    action_digest: str = Field(pattern=r"^[0-9a-f]{64}$")
    summary: str = Field(min_length=1, max_length=2_000)
    rationale: str = Field(min_length=1, max_length=4_000)
    preauthorization_rule: str
    evidence_resources: tuple[ProducedResourceReferenceV1, ...] = ()
    required_operator_capability: str


class AsyncBlockerHandoffReceiptV1(_StrictContract):
    schema_version: Literal["smr.intern-async-blocker-handoff-receipt.v1"] = (
        "smr.intern-async-blocker-handoff-receipt.v1"
    )
    handoff_receipt_id: str
    blocker_id: str
    org_id: str
    sync_session_id: str
    opened_by_user_id: str
    idempotency_key: str
    context: AsyncBlockerHandoffContextV1
    context_digest: str = Field(pattern=r"^[0-9a-f]{64}$")
    created_at: datetime


class AsyncBlockerOpenSyncResponse(_StrictContract):
    schema_version: Literal["smr.intern-async-blocker-open-sync.v1"] = (
        "smr.intern-async-blocker-open-sync.v1"
    )
    blocker: AsyncBlockerResponse
    sync_session: SyncSessionResponse
    handoff_receipt: AsyncBlockerHandoffReceiptV1


class AsyncBlockerResolveRequest(_StrictContract):
    """Operator disposition for one exact Async-to-Sync handoff."""

    idempotency_key: str = Field(min_length=1, max_length=512)
    outcome: Literal["completed", "denied", "superseded"]
    comment: str | None = Field(default=None, max_length=4_000)
    supporting_receipt_ids: tuple[str, ...] = Field(default=(), max_length=128)

    @model_validator(mode="after")
    def validate_supporting_receipts(self) -> "AsyncBlockerResolveRequest":
        if any(
            not value.strip() or len(value) > 512
            for value in self.supporting_receipt_ids
        ):
            raise ValueError(
                "supporting receipt IDs must be non-empty and at most 512 characters"
            )
        if len(set(self.supporting_receipt_ids)) != len(self.supporting_receipt_ids):
            raise ValueError("supporting receipt IDs must be unique")
        return self


class AsyncBlockerResolveResponse(_StrictContract):
    schema_version: Literal["smr.intern-async-blocker-resolution.v1"] = (
        "smr.intern-async-blocker-resolution.v1"
    )
    blocker: AsyncBlockerResponse
    continuation_command: InternRuntimeCommandReceipt


class AsyncJudgmentItemResponse(_StrictContract):
    """One open ask-and-continue judgment item (does not freeze org Async)."""

    schema_version: Literal["smr.intern-async-judgment.v1"] = (
        "smr.intern-async-judgment.v1"
    )
    interaction_id: str
    effort_id: str
    prompt: str
    created_generation: int = Field(ge=0)
    context: dict[str, Any] = Field(default_factory=dict)


class AsyncEffortWorkSummary(_StrictContract):
    effort_id: str
    status: Literal["runnable", "awaiting_input"]
    open_interaction_id: str | None = None
    # Fairness cursor for the AC3 round-robin fan-out: the cycle in which this
    # Effort last held a cycle slot (0 = never advanced).
    last_advanced_cycle: int = Field(default=0, ge=0)


class AsyncRuntimeResponse(_StrictContract):
    schema_version: Literal["smr.intern-async-runtime.v1"] = (
        "smr.intern-async-runtime.v1"
    )
    async_runtime_id: str
    async_assignment_id: str = Field(json_schema_extra={"deprecated": True})
    cardinality: Literal["one_per_organization"] = "one_per_organization"
    instance_kind: Literal["organization_async_intern"] = "organization_async_intern"
    research_intern_id: str
    org_id: str
    objective: str
    status: AsyncStatus
    state_generation: int = Field(ge=0)
    last_event_sequence: int = Field(ge=0)
    cycle_number: int = Field(ge=0)
    plan: dict[str, Any] = Field(default_factory=dict)
    pending_interaction_id: str | None = None
    pending_action_id: str | None = None
    pending_actor_message_id: str | None = None
    awaiting_actor_reply_message_id: str | None = None
    awaiting_actor_reply_thread_id: str | None = None
    pending_instruction_count: int = Field(default=0, ge=0, le=32)
    open_judgment_items: tuple[AsyncJudgmentItemResponse, ...] = ()
    effort_work: tuple[AsyncEffortWorkSummary, ...] = ()
    binding: RuntimeBinding
    external_execution_status: ExternalExecutionStatus
    evidence_readiness: EvidenceReadiness
    next_wake_at: datetime | None = None
    checkpoint: AsyncCheckpointResponse | None = None
    budget: AsyncRuntimeBudget
    # Ceilings (``budget``) are meaningless to a caller without the burn they
    # bound and the lease that accrues it. Defaulted so every existing
    # construction site keeps working; the read path fills them in.
    spend: AsyncRuntimeSpend = Field(default_factory=AsyncRuntimeSpend)
    host_lease: AsyncRuntimeHostLease = Field(default_factory=AsyncRuntimeHostLease)
    blocker: AsyncBlockerResponse | None = None
    temporal_workflow_id: str
    # Additive ambient phase projection. ``phase`` is derived (never a DB
    # column); ``leave_safe`` is honest rather than compile-time True.
    # See: INTERN_ASYNC_RUNTIME_PHASE_REFACTOR_HANDOFF_2026-08-08.md §6.3
    phase: RuntimePhaseWire = RuntimePhaseWire.LIFECYCLE_NOT_STARTED
    stop_reason: StopReason | None = None
    resume: ResumeConditionResponse | None = None
    active_effort_id: str | None = None
    outcome: RuntimeOutcome | None = None
    leave_safe: bool = False
    created_at: datetime
    updated_at: datetime
    closed_at: datetime | None = None

    @model_validator(mode="after")
    def require_evidence_phase_after_execution_terminal(
        self,
    ) -> "AsyncRuntimeResponse":
        if self.async_runtime_id != self.async_assignment_id:
            raise ValueError("Async runtime compatibility identity mismatch")
        if (
            self.external_execution_status is ExternalExecutionStatus.TERMINAL
            and self.status is AsyncStatus.COMPLETED
            and self.evidence_readiness is not EvidenceReadiness.READY
        ):
            raise ValueError("completed Async assignment requires ready evidence")
        if self.status is AsyncStatus.COMPLETED and (
            self.awaiting_actor_reply_message_id is not None
            or self.awaiting_actor_reply_thread_id is not None
        ):
            raise ValueError("completed Async assignment cannot await an actor reply")
        return self


# Compatibility import for code that predates the org-singleton decision.
AsyncAssignmentResponse = AsyncRuntimeResponse


class InternMcpActionResponse(_StrictContract):
    schema_version: Literal["smr.intern-mcp-action.v1"] = "smr.intern-mcp-action.v1"
    action_id: str
    runtime_kind: Literal["sync", "async"]
    runtime_id: str
    source_generation: int = Field(ge=1)
    effect_id: str
    action_kind: InternMcpActionKind
    server_name: str
    method: InternMcpMethod
    operation_name: str | None = None
    resource_uri: str | None = None
    requested_capability_operation: str
    arguments: dict[str, Any]
    rationale: str
    status: InternMcpActionStatus
    approval_status: Literal["not_required", "pending", "approved", "refused"]
    approval_id: str | None = None
    result: dict[str, Any]
    receipt: dict[str, Any]
    error_code: str | None = None
    created_at: datetime
    updated_at: datetime
    started_at: datetime | None = None
    completed_at: datetime | None = None


class SyncApprovalActionV1(_StrictContract):
    schema_version: Literal["smr.sync-approval-action.v1"] = (
        "smr.sync-approval-action.v1"
    )
    action_kind: str = Field(min_length=1, max_length=128)
    target_resources: tuple[ProducedResourceReferenceV1, ...] = Field(
        default=(), max_length=64
    )
    parameters: dict[str, Any] = Field(default_factory=dict)
    preconditions: dict[str, Any] = Field(default_factory=dict)
    budget_effect: dict[str, Any] = Field(default_factory=dict)
    capability_operation: str = Field(min_length=1, max_length=255)
    rationale: str = Field(min_length=1, max_length=4_000)
    evidence_references: tuple[ProducedResourceReferenceV1, ...] = Field(
        default=(), max_length=128
    )
    expires_at: datetime
    idempotency_key: str = Field(min_length=1, max_length=512)

    @model_validator(mode="after")
    def bound_action(self) -> "SyncApprovalActionV1":
        encoded = json.dumps(
            self.model_dump(mode="json"),
            sort_keys=True,
            separators=(",", ":"),
            allow_nan=False,
        ).encode()
        if len(encoded) > 64 * 1_024:
            raise ValueError("approval action is too large")
        return self


class SyncApprovalApplicationReceiptV1(_StrictContract):
    schema_version: Literal["smr.approval-application-receipt.v1"] = (
        "smr.approval-application-receipt.v1"
    )
    receipt_id: str
    attempt_number: int = Field(ge=1)
    application_state: Literal["applying", "applied", "failed", "stale"]
    precondition_results: dict[str, Any] = Field(default_factory=dict)
    result: dict[str, Any] = Field(default_factory=dict)
    produced_resources: tuple[ProducedResourceReferenceV1, ...] = ()
    failure_code: str | None = None
    created_at: datetime


class SyncApprovalCardV1(_StrictContract):
    schema_version: Literal["smr.sync-approval-card.v1"] = "smr.sync-approval-card.v1"
    approval_id: str
    org_id: str
    project_id: str | None
    scope_kind: Literal[
        "sync_session",
        "project",
        "factory",
        "effort",
        "run",
        "experiment",
        "visual",
        "report",
    ]
    scope_id: str
    sync_session_id: str
    operator_user_id: str
    presence_lease_id: str
    decision_state: Literal["pending", "approved", "denied", "expired"]
    application_state: Literal["not_started", "applying", "applied", "failed", "stale"]
    action: SyncApprovalActionV1
    allowed_decisions: tuple[Literal["approve", "deny", "edit"], ...] = ()
    title: str | None = None
    body: str | None = None
    created_at: datetime
    resolved_at: datetime | None = None
    resolved_by_user_id: str | None = None
    resolution_comment: str | None = None
    application_receipts: tuple[SyncApprovalApplicationReceiptV1, ...] = ()


class SyncApprovalDecisionRequestV1(_StrictContract):
    decision: Literal["approve", "deny", "edit"]
    comment: str | None = Field(default=None, max_length=4_000)
    # ``edit`` decisions on experiment-disposition approvals carry the amended
    # proposal fields; any other decision must omit the amendment.
    amendment: SyncExperimentAmendmentV1 | None = None


class InternMcpActionApprovalRequest(_StrictContract):
    decision: Literal["approve", "refuse", "edit"]
    comment: str | None = Field(default=None, max_length=4_000)


class InternAcceptanceFixtureRequest(_StrictContract):
    """Bootstrap one disposable Factory/Project/Effort/Run acceptance fixture.

    No tribal ids: the backend provisions the whole chain through the
    canonical creation authorities and binds the organization Intern to it.
    Idempotent within one test attempt via ``idempotency_key``.

    ``task_template`` is a caller-supplied label recorded on the fixture
    receipt; eval-specific task content (objectives, rubrics, required
    project files) lives with the eval harness, never in production.
    """

    idempotency_key: str = Field(min_length=1, max_length=180)
    task_template: str = Field(
        default="acceptance_default", min_length=1, max_length=128
    )
    # A fixture Run is an inert, pre-terminalized row that satisfies binding
    # validation without dispatching compute. Disable for chain-only fixtures.
    include_run: bool = True
    metadata: dict[str, Any] = Field(default_factory=dict)


class InternAcceptanceFixtureResourceV1(_StrictContract):
    resource_kind: str
    resource_id: str | None = None
    status: str
    error_code: str | None = None


class InternAcceptanceFixtureReceipt(_StrictContract):
    """Durable, validator-readable evidence for one acceptance fixture."""

    schema_version: Literal["smr.intern-acceptance-fixture.v1"] = (
        "smr.intern-acceptance-fixture.v1"
    )
    fixture_id: str
    org_id: str
    research_intern_id: str
    task_template: str
    status: Literal["ready", "torn_down"]
    binding: RuntimeBinding
    resources: list[InternAcceptanceFixtureResourceV1] = Field(default_factory=list)
    factory_readiness: dict[str, Any] = Field(default_factory=dict)
    replayed: bool = False
    created_at: datetime
    torn_down_at: datetime | None = None
    # Evidence stays readable until this instant; teardown never destroys it
    # before the validation window ends.
    retention_expires_at: datetime | None = None
