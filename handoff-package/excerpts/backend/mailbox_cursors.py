"""Cursor validation for exact ordered Intern event replay."""

from __future__ import annotations

from dataclasses import dataclass

from packages.intern.mailbox.commands import InternMailboxRuntime
from packages.intern.mailbox.events import InternMailboxEvent
from packages.intern.mailbox.validation import require_nonnegative


@dataclass(frozen=True, slots=True)
class InternEventCursor:
    after_sequence: int = 0

    def __post_init__(self) -> None:
        require_nonnegative(
            self.after_sequence, "intern_mailbox_cursor_sequence_negative"
        )


@dataclass(frozen=True, slots=True)
class InternEventPage:
    runtime: InternMailboxRuntime
    events: tuple[InternMailboxEvent, ...]
    next_cursor: InternEventCursor
    requested_cursor: InternEventCursor = InternEventCursor()

    def __post_init__(self) -> None:
        if not isinstance(self.runtime, InternMailboxRuntime):
            raise ValueError("intern_mailbox_event_page_runtime_invalid")
        events = tuple(self.events)
        object.__setattr__(self, "events", events)
        previous_sequence = self.requested_cursor.after_sequence
        for event in events:
            if event.runtime != self.runtime:
                raise ValueError("intern_mailbox_event_page_runtime_mixed")
            if event.sequence <= previous_sequence:
                raise ValueError("intern_mailbox_event_page_sequence_not_increasing")
            previous_sequence = event.sequence
        expected_next = (
            events[-1].sequence
            if events
            else self.requested_cursor.after_sequence
        )
        if self.next_cursor.after_sequence != expected_next:
            raise ValueError("intern_mailbox_event_page_next_cursor_inconsistent")

    @classmethod
    def from_events(
        cls,
        runtime: InternMailboxRuntime,
        *,
        after: InternEventCursor,
        events: tuple[InternMailboxEvent, ...],
    ) -> "InternEventPage":
        next_cursor = InternEventCursor(
            events[-1].sequence if events else after.after_sequence
        )
        return cls(runtime, events, next_cursor, after)
