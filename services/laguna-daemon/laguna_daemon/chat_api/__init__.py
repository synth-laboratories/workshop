"""The Chat Completions wire surface.

Chat request and response types exist in this package and nowhere else. It is a
peer of the native Responses surface: both compile onto the neutral turn core in
`responses_api.compiler` and execute on the shared `TurnRunner`. Neither surface
is implemented in terms of the other, and no request is ever lowered into the
other protocol's objects.
"""

from .renderer import ChatEventAssembler, chat_sse_frame
from .service import ChatService
from .validation import validate_chat_request

__all__ = [
    "ChatEventAssembler",
    "ChatService",
    "chat_sse_frame",
    "validate_chat_request",
]
