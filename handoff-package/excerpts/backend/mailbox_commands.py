"""Inbound client/operator command contracts for an Intern runtime."""

from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass
from datetime import datetime
from typing import Literal

from packages.intern.mailbox.validation import (
    canonical_json_sha256,
    immutable_json_mapping,
    mutable_json_mapping,
    require_nonempty,
    require_nonnegative,
    require_timezone_aware,
)


@dataclass(frozen=True, slots=True)
class InternMailboxRuntime:
    runtime_kind: Literal["sync", "async"]
    runtime_id: str

    def __post_init__(self) -> None:
        if self.runtime_kind not in {"sync", "async"}:
            raise ValueError("intern_mailbox_runtime_kind_invalid")
        require_nonempty(self.runtime_id, "intern_mailbox_runtime_id_missing")


@dataclass(frozen=True, slots=True)
class InternMailboxCommand:
    """One immutable semantic command addressed to exactly one runtime.

    ``semantic_fingerprint`` hashes a version marker, runtime identity,
    command identity, expected generation, command kind, and recursively
    canonical JSON payload. It intentionally excludes the idempotency key and
    ``submitted_at`` because those identify/adorn delivery rather than change
    command meaning.
    """

    runtime: InternMailboxRuntime
    command_id: str
    idempotency_key: str
    expected_generation: int
    command_kind: str
    payload: Mapping[str, object]
    submitted_at: datetime | None = None

    def __post_init__(self) -> None:
        if not isinstance(self.runtime, InternMailboxRuntime):
            raise ValueError("intern_mailbox_command_runtime_invalid")
        require_nonempty(self.command_id, "intern_mailbox_command_id_missing")
        require_nonempty(
            self.idempotency_key, "intern_mailbox_idempotency_key_missing"
        )
        require_nonnegative(
            self.expected_generation,
            "intern_mailbox_expected_generation_negative",
        )
        require_nonempty(self.command_kind, "intern_mailbox_command_kind_missing")
        if self.submitted_at is not None:
            require_timezone_aware(
                self.submitted_at, "intern_mailbox_submitted_at_timezone_missing"
            )
        object.__setattr__(self, "payload", immutable_json_mapping(self.payload))

    @property
    def semantic_fingerprint(self) -> str:
        payload = mutable_json_mapping(
            self.payload  # type: ignore[arg-type]
        )
        return canonical_json_sha256(
            {
                "schema": "smr.intern-mailbox-command.semantic.v1",
                "runtime_kind": self.runtime.runtime_kind,
                "runtime_id": self.runtime.runtime_id,
                "command_id": self.command_id,
                "expected_generation": self.expected_generation,
                "command_kind": self.command_kind,
                "payload": payload,
            }
        )

    def payload_dict(self) -> dict[str, object]:
        return mutable_json_mapping(
            self.payload  # type: ignore[arg-type]
        )


def commands_have_same_semantics(
    first: InternMailboxCommand, second: InternMailboxCommand
) -> bool:
    return first.semantic_fingerprint == second.semantic_fingerprint
