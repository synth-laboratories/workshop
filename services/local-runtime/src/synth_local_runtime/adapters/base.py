from __future__ import annotations

from abc import ABC, abstractmethod
from typing import Any, TYPE_CHECKING

if TYPE_CHECKING:
    from ..service import RuntimeService


class RuntimeAdapter(ABC):
    def __init__(self, service: "RuntimeService") -> None:
        self.service = service

    def prepare_session(self, session: dict[str, Any]) -> dict[str, Any]:
        return session

    @abstractmethod
    def send_message(self, session: dict[str, Any], run: dict[str, Any], body: str) -> None:
        raise NotImplementedError

    @abstractmethod
    def control(
        self,
        session: dict[str, Any],
        kind: str,
        payload: dict[str, Any],
    ) -> dict[str, Any]:
        raise NotImplementedError

    def resume_existing(self, session: dict[str, Any]) -> None:
        return None
