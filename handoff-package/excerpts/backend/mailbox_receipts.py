"""Durable Intern command receipt contracts.

The ``actuation`` payload is the durable proof that a command's state
transition committed (schema ``smr.intern-command-actuation.v1``). It is
written in the same database transaction as the transition itself and binds:
command id, runtime kind/id, event sequence, pre/post generation, transition
kind, timestamp, terminal-ness, and a canonical result digest. Replayed
admissions of an already-final command expose ``duplicate=True`` while the
stored actuation stays immutable.
"""

from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass, field
from datetime import datetime
from enum import Enum
from typing import Any

from packages.intern.mailbox.commands import InternMailboxRuntime
from packages.intern.mailbox.validation import (
    require_nonempty,
    require_nonnegative,
    require_timezone_aware,
)


class InternMailboxCommandStatus(str, Enum):
    RECEIVED = "received"
    DELIVERED = "delivered"
    """Durably available and delivery attempted; reducer acceptance is unknown."""

    APPLIED = "applied"
    NOOP = "noop"
    REFUSED = "refused"
    SUPERSEDED = "superseded"
    CONFLICT = "conflict"


PENDING_COMMAND_STATUSES = frozenset(
    {InternMailboxCommandStatus.RECEIVED, InternMailboxCommandStatus.DELIVERED}
)
FINAL_COMMAND_STATUSES = (
    frozenset(InternMailboxCommandStatus) - PENDING_COMMAND_STATUSES
)


@dataclass(frozen=True, slots=True)
class InternMailboxReceipt:
    runtime: InternMailboxRuntime
    command_id: str
    idempotency_key: str
    status: InternMailboxCommandStatus
    previous_generation: int
    state_generation: int
    decision_code: str
    created_at: datetime
    updated_at: datetime
    actuation: Mapping[str, Any] | None = None
    duplicate: bool = field(default=False)

    def __post_init__(self) -> None:
        if self.actuation is not None and not isinstance(self.actuation, Mapping):
            raise ValueError("intern_mailbox_receipt_actuation_invalid")
        if not isinstance(self.runtime, InternMailboxRuntime):
            raise ValueError("intern_mailbox_receipt_runtime_invalid")
        require_nonempty(self.command_id, "intern_mailbox_receipt_command_id_missing")
        require_nonempty(
            self.idempotency_key, "intern_mailbox_receipt_idempotency_key_missing"
        )
        if not isinstance(self.status, InternMailboxCommandStatus):
            raise ValueError("intern_mailbox_receipt_status_invalid")
        require_nonnegative(
            self.previous_generation,
            "intern_mailbox_receipt_previous_generation_negative",
        )
        require_nonnegative(
            self.state_generation,
            "intern_mailbox_receipt_state_generation_negative",
        )
        require_nonempty(
            self.decision_code, "intern_mailbox_receipt_decision_code_missing"
        )
        require_timezone_aware(
            self.created_at, "intern_mailbox_receipt_created_at_timezone_missing"
        )
        require_timezone_aware(
            self.updated_at, "intern_mailbox_receipt_updated_at_timezone_missing"
        )
        if self.updated_at < self.created_at:
            raise ValueError("intern_mailbox_receipt_updated_before_created")
