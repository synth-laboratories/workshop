from __future__ import annotations

"""Restoring a sampled tool call to the item kind the caller asked for.

Both local backends reach this with a `(binding, arguments)` pair and nothing
else: the MLX backend parses Laguna's `<tool_call>` envelope dialect, the
llama.cpp backend receives OpenAI `tool_calls` deltas from the engine. What a
call *becomes* on the way out is a property of the immutable `ToolBinding`
table built at compile time, never of the name the model emitted — which is
why a shell tool cannot come back as a plain function call, and an unbound
name fails closed instead of being dispatched.
"""

import json
from typing import Any

from ..errors import ResponsesError
from ..ids import new_id
from .protocol import ModelEvent, ToolBinding


#: Bound tool kind → the Responses item a completed call is emitted as. Kinds
#: absent here (`function`, and anything the compiler lowered to one) are
#: function calls.
_EVENT_KIND_FOR_BINDING = {
    "tool_search": "tool_search_call",
    "shell": "shell_call",
    "local_shell": "shell_call",
    "apply_patch": "apply_patch_call",
    "mcp": "mcp_call",
}


def tool_call_event(
    binding: ToolBinding,
    arguments: dict[str, Any],
    *,
    raw_input: str | None = None,
) -> ModelEvent:
    """Build the typed call event for one completed tool call.

    `raw_input` is the custom-tool escape hatch: a custom tool's payload is a
    verbatim string, so a backend that holds the undecoded text passes it here
    rather than letting a JSON round-trip reshape it.
    """
    call_id = new_id("call")
    if binding.kind == "custom":
        parsed = arguments.get("input")
        raw = parsed if isinstance(parsed, str) else raw_input
        if not isinstance(raw, str):
            raise ResponsesError(
                "invalid_custom_tool_input",
                f"Custom tool {binding.original_name!r} did not return a raw input string.",
                422,
                error_type="model_error",
            )
        return ModelEvent(
            kind="custom_tool_call",
            name=binding.original_name,
            namespace=binding.namespace,
            call_id=call_id,
            input=raw,
        )
    return ModelEvent(
        kind=_EVENT_KIND_FOR_BINDING.get(binding.kind, "function_call"),
        name=binding.original_name,
        namespace=binding.namespace,
        call_id=call_id,
        arguments=json.dumps(arguments, ensure_ascii=False, separators=(",", ":")),
    )


def resolve_binding(name: str, bindings: dict[str, ToolBinding]) -> ToolBinding:
    """Look up the binding for a model-visible tool name, or fail closed."""
    binding = bindings.get(name)
    if binding is None:
        raise ResponsesError(
            "unknown_tool_call",
            f"The model selected unknown tool {name!r}.",
            422,
            error_type="model_error",
        )
    return binding
