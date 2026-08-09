"""Immutable events in the authoritative Intern runtime ledger."""

from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass
from datetime import datetime

from packages.intern.mailbox.commands import InternMailboxRuntime
from packages.intern.mailbox.validation import (
    immutable_json_mapping,
    mutable_json_mapping,
    require_nonempty,
    require_nonnegative,
    require_timezone_aware,
)


@dataclass(frozen=True, slots=True)
class InternMailboxEvent:
    event_id: str
    runtime: InternMailboxRuntime
    sequence: int
    previous_state_generation: int
    state_generation: int
    event_kind: str
    command_id: str | None
    payload: Mapping[str, object]
    created_at: datetime

    def __post_init__(self) -> None:
        require_nonempty(self.event_id, "intern_mailbox_event_id_missing")
        if not isinstance(self.runtime, InternMailboxRuntime):
            raise ValueError("intern_mailbox_event_runtime_invalid")
        require_nonnegative(self.sequence, "intern_mailbox_event_sequence_negative")
        require_nonnegative(
            self.previous_state_generation,
            "intern_mailbox_event_previous_generation_negative",
        )
        require_nonnegative(
            self.state_generation,
            "intern_mailbox_event_state_generation_negative",
        )
        # Command transitions advance generation. Observation events (presence,
        # source refresh, projection reconciliation) share the same ordered
        # ledger without mutating runtime state, so equality is intentional.
        if self.state_generation < self.previous_state_generation:
            raise ValueError("intern_mailbox_event_generation_regressed")
        require_nonempty(self.event_kind, "intern_mailbox_event_kind_missing")
        if self.command_id is not None:
            require_nonempty(
                self.command_id, "intern_mailbox_event_command_id_empty"
            )
        require_timezone_aware(
            self.created_at, "intern_mailbox_event_created_at_timezone_missing"
        )
        object.__setattr__(self, "payload", immutable_json_mapping(self.payload))

    def payload_dict(self) -> dict[str, object]:
        return mutable_json_mapping(
            self.payload  # type: ignore[arg-type]
        )
