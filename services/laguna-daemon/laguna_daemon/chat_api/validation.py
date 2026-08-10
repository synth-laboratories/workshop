from __future__ import annotations

from typing import Any

from ..responses_api.errors import ResponsesError, invalid


CHAT_ROLES = {"system", "developer", "user", "assistant", "tool"}

#: Text part kinds this server can faithfully render into a prompt.
TEXT_PART_TYPES = {"text", "input_text", "output_text"}

#: Fields OpenAI defines on a Chat request that this backend cannot honor.
#: Silently accepting any of them would make the response a quiet lie about
#: what was actually asked for, so each returns a stable error instead.
#: Each entry pairs a description with a predicate deciding whether the value
#: actually asks for the capability. A blanket "reject if present" would reject
#: SDK defaults like `logit_bias: {}`, while a blanket "reject if truthy" would
#: accept `web_search_options: {}`, which means *enable search with defaults*.
UNSUPPORTED_FIELDS: dict[str, tuple[str, Any]] = {
    "logprobs": ("token log probabilities", bool),
    "top_logprobs": ("token log probabilities", lambda v: int(v or 0) > 0),
    "logit_bias": ("logit bias", bool),
    "seed": ("deterministic seeding", lambda v: v is not None),
    "audio": ("audio output", lambda v: v is not None),
    "modalities": (
        "non-text modalities",
        lambda v: any(entry != "text" for entry in (v or [])),
    ),
    "prediction": ("predicted outputs", lambda v: v is not None),
    # An empty options object still requests the tool.
    "web_search_options": ("hosted web search", lambda v: v is not None),
    "functions": ("the deprecated functions parameter (use tools)", bool),
    "function_call": (
        "the deprecated function_call parameter (use tool_choice)",
        lambda v: v is not None,
    ),
}


def _unsupported(capability: str, param: str) -> ResponsesError:
    return ResponsesError(
        "unsupported_chat_field",
        f"This local model does not support {capability}.",
        400,
        param,
    )


def validate_chat_request(body: Any) -> dict[str, Any]:
    """Validate the supported Chat subset, rejecting the rest explicitly."""
    if not isinstance(body, dict):
        raise invalid("The request body must be a JSON object.")

    messages = body.get("messages")
    if not isinstance(messages, list) or not messages:
        raise invalid("messages must be a non-empty array.", param="messages")

    for field, (capability, requests_it) in UNSUPPORTED_FIELDS.items():
        if field in body and requests_it(body[field]):
            raise _unsupported(capability, field)

    n = body.get("n")
    if n is not None and int(n) != 1:
        raise _unsupported("more than one choice per request (n > 1)", "n")

    stop = body.get("stop")
    if stop:
        raise _unsupported("custom stop sequences", "stop")

    for field in ("presence_penalty", "frequency_penalty"):
        value = body.get(field)
        if value is not None and float(value) != 0.0:
            raise _unsupported(
                "presence or frequency penalties; the local sampler exposes only "
                "temperature and top_p",
                field,
            )

    tier = body.get("service_tier")
    if tier is not None and tier not in {"auto", "default"}:
        raise _unsupported(f"the {tier!r} service tier", "service_tier")

    effort = body.get("reasoning_effort")
    if effort == "max":
        # The desktop's historical binary "On" label; a documented alias.
        body["reasoning_effort"] = "high"
    elif effort not in (None, "none", "high"):
        # One thinking mode exists. Running `high` while reporting `medium`
        # would misdescribe what was measured; degrading to `none` would lie
        # in the other direction.
        raise ResponsesError(
            "unsupported_reasoning_effort",
            f"Reasoning effort {effort!r} is not supported by this model; use "
            "'none' or 'high' ('max' is accepted as a legacy alias for 'high').",
            400,
            "reasoning_effort",
        )

    top_k = body.get("top_k")
    if top_k is not None and (
        not isinstance(top_k, int) or isinstance(top_k, bool) or not 0 <= top_k <= 8192
    ):
        raise invalid("top_k must be an integer between 0 and 8192.", param="top_k")

    if body.get("store"):
        raise _unsupported(
            "server-side conversation storage on this surface; the Chat surface "
            "is stateless, and the native Responses surface owns persistence",
            "store",
        )

    response_format = body.get("response_format")
    if response_format is not None:
        if not isinstance(response_format, dict):
            raise invalid("response_format must be an object.", param="response_format")
        kind = response_format.get("type")
        if kind not in {"text", "json_object", "json_schema"}:
            raise _unsupported(
                f"the {kind!r} response format", "response_format.type"
            )

    for index, message in enumerate(messages):
        _validate_message(message, index)

    _validate_tools(body.get("tools"))
    _validate_tool_choice(body.get("tool_choice"), body.get("tools"))
    return body


def _validate_message(message: Any, index: int) -> None:
    param = f"messages[{index}]"
    if not isinstance(message, dict):
        raise invalid("Every message must be an object.", param=param)
    role = message.get("role")
    if role not in CHAT_ROLES:
        raise invalid(
            f"Unsupported message role {role!r}.", param=f"{param}.role"
        )
    if role == "tool" and not message.get("tool_call_id"):
        raise invalid(
            "A tool message requires tool_call_id.", param=f"{param}.tool_call_id"
        )

    content = message.get("content")
    if content is None:
        # Only an assistant turn that is purely tool calls may omit content.
        if role == "assistant" and message.get("tool_calls"):
            return
        raise invalid("Message content is required.", param=f"{param}.content")
    if isinstance(content, str):
        return
    if not isinstance(content, list):
        raise invalid(
            "Message content must be a string or an array of parts.",
            param=f"{param}.content",
        )
    for part_index, part in enumerate(content):
        if not isinstance(part, dict):
            raise invalid(
                "Every content part must be an object.",
                param=f"{param}.content[{part_index}]",
            )
        kind = part.get("type")
        if kind not in TEXT_PART_TYPES:
            raise _unsupported(
                f"{kind!r} content parts; this model accepts text only",
                f"{param}.content[{part_index}].type",
            )


def _validate_tools(tools: Any) -> None:
    if tools is None:
        return
    if not isinstance(tools, list):
        raise invalid("tools must be an array.", param="tools")
    for index, tool in enumerate(tools):
        param = f"tools[{index}]"
        if not isinstance(tool, dict):
            raise invalid("Every tool must be an object.", param=param)
        kind = tool.get("type")
        if kind != "function":
            # Responses-only tool kinds cannot round-trip through Chat: the
            # wire format has no way to carry back a custom tool's raw input or
            # a namespaced call's identity, so accepting them here would mean
            # returning something the caller did not ask for.
            raise _unsupported(
                f"{kind!r} tools on the Chat surface; the native Responses "
                "surface carries custom, namespace, MCP, shell, and apply_patch "
                "tools without loss",
                f"{param}.type",
            )
        function = tool.get("function")
        if not isinstance(function, dict) or not function.get("name"):
            raise invalid(
                "A function tool requires function.name.",
                param=f"{param}.function.name",
            )
        parameters = function.get("parameters")
        if parameters is not None and not isinstance(parameters, dict):
            raise invalid(
                "function.parameters must be a JSON Schema object.",
                param=f"{param}.function.parameters",
            )


def _validate_tool_choice(tool_choice: Any, tools: Any) -> None:
    if tool_choice is None:
        return
    if isinstance(tool_choice, str):
        if tool_choice not in {"auto", "none", "required"}:
            raise invalid(
                "tool_choice must be auto, none, required, or a named function.",
                param="tool_choice",
            )
        if tool_choice == "required" and not tools:
            raise invalid(
                "tool_choice required needs at least one tool.", param="tool_choice"
            )
        return
    if isinstance(tool_choice, dict):
        if tool_choice.get("type") != "function":
            raise _unsupported(
                f"{tool_choice.get('type')!r} tool choice", "tool_choice.type"
            )
        function = tool_choice.get("function")
        if not isinstance(function, dict) or not function.get("name"):
            raise invalid(
                "A named tool_choice requires function.name.",
                param="tool_choice.function.name",
            )
        return
    raise invalid("tool_choice must be a string or an object.", param="tool_choice")
