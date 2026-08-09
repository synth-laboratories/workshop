from __future__ import annotations

import threading
import time
from collections import defaultdict
from typing import Callable


class EventBroker:
    """A wake-up hint over SQLite; the database remains the replay authority."""

    def __init__(self) -> None:
        self._conditions: dict[str, threading.Condition] = defaultdict(threading.Condition)

    def notify(self, session_id: str) -> None:
        condition = self._conditions[session_id]
        with condition:
            condition.notify_all()

    def wait_for(
        self,
        session_id: str,
        predicate: Callable[[], bool],
        *,
        timeout: float,
    ) -> bool:
        deadline = time.monotonic() + timeout
        condition = self._conditions[session_id]
        with condition:
            while not predicate():
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    return False
                condition.wait(timeout=remaining)
            return True
