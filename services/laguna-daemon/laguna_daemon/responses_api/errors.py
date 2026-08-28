from __future__ import annotations

from dataclasses import dataclass
from typing import Any


@dataclass(slots=True)
class ResponsesError(Exception):
    code: str
    message: str
    status_code: int = 400
    param: str | None = None
    error_type: str = "invalid_request_error"

    def payload(self) -> dict[str, Any]:
        return {
            "error": {
                "type": self.error_type,
                "code": self.code,
                "message": self.message,
                "param": self.param,
            }
        }


def invalid(message: str, *, param: str | None = None) -> ResponsesError:
    return ResponsesError("invalid_request", message, 400, param)


def unsupported(capability: str, *, param: str | None = None) -> ResponsesError:
    return ResponsesError(
        "unsupported_model_capability",
        f"The selected model does not support {capability}.",
        400,
        param,
    )
