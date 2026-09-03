from __future__ import annotations

import json
import hashlib
import re
from typing import Any, Callable

from .backends.protocol import CompiledTurn, ToolBinding
from .errors import ResponsesError, invalid
from .capabilities import DEFAULT_MAX_OUTPUT_TOKENS, SamplingDefaults


_SAFE_NAME = re.compile(r"[^A-Za-z0-9_-]")


def _model_visible_name(value: str) -> str:
    sanitized = _SAFE_NAME.sub("_", value)
    if len(sanitized) <= 64:
        return sanitized
    digest = hashlib.sha256(value.encode()).hexdigest()[:12]
    return f"{sanitized[:49]}__{digest}"


def _content_text(content: Any) -> str:
    if isinstance(content, str):
        return content
    if not isinstance(content, list):
        return ""
    pieces: list[str] = []
    for part in content:
        if not isinstance(part, dict):
            continue
        kind = part.get("type")
        if kind in {"input_text", "output_text", "text", "summary_text"}:
            pieces.append(str(part.get("text") or ""))
        elif kind == "refusal":
            pieces.append(str(part.get("refusal") or ""))
    return "\n".join(piece for piece in pieces if piece)


def _tool_message(item: dict[str, Any]) -> dict[str, Any] | None:
    kind = item.get("type")
    if kind in {
        "function_call_output",
        "custom_tool_call_output",
        "shell_call_output",
        "local_shell_call_output",
        "apply_patch_call_output",
        "mcp_call_output",
        "tool_search_output",
    }:
        output = item.get("output")
        if not isinstance(output, str):
            output = json.dumps(output, ensure_ascii=False, separators=(",", ":"))
        return {
            "role": "tool",
            "content": output,
            "tool_call_id": item.get("call_id"),
        }
    return None


def items_to_messages(
    request: dict[str, Any], context_items: list[dict[str, Any]]
) -> list[dict[str, Any]]:
    messages: list[dict[str, Any]] = []
    pending_reasoning: list[str] = []
    instructions = request.get("instructions")
    if isinstance(instructions, str) and instructions:
        messages.append({"role": "system", "content": instructions})
    for item in context_items:
        if not isinstance(item, dict):
            continue
        kind = item.get("type")
        if kind == "message":
            role = item.get("role")
            if role in {"system", "developer", "user", "assistant"}:
                message = {
                    "role": "system" if role == "developer" else role,
                    "content": _content_text(item.get("content")),
                }
                if role == "assistant" and pending_reasoning:
                    message["reasoning_content"] = "\n".join(pending_reasoning)
                    pending_reasoning.clear()
                messages.append(message)
        elif kind == "reasoning":
            # Laguna's template accepts prior thinking in `reasoning_content`.
            # Responses represents that text as a separate output item before
            # the assistant message or tool call, so retain it until that
            # assistant item is lowered. Dropping it makes multi-step agentic
            # turns silently stop reasoning after the first tool boundary.
            reasoning = _content_text(item.get("summary"))
            if reasoning:
                pending_reasoning.append(reasoning)
            continue
        elif kind in {
            "function_call",
            "custom_tool_call",
            "shell_call",
            "local_shell_call",
            "apply_patch_call",
            "mcp_call",
            "tool_search_call",
        }:
            name = str(item.get("name") or item.get("server_label") or kind)
            namespace = str(item.get("namespace") or "")
            # Responses exposes namespaced calls to Codex as separate
            # `namespace` + original `name` fields. Laguna's prompt template,
            # however, advertised the collision-safe joined name. Replaying
            # only the short original name taught the model to emit a name
            # that was absent from the current binding table on continuation
            # turns (for example `container_list` instead of
            # `mcp__synth_containers__container_list`). Restore the exact
            # model-visible spelling when lowering history back into a prompt.
            history_name = (
                _model_visible_name(f"{namespace}__{name}")
                if namespace and not name.startswith(f"{namespace}__")
                else name
            )
            raw = item.get("arguments", item.get("input", item.get("action", {})))
            if isinstance(raw, str):
                try:
                    arguments = json.loads(raw)
                except ValueError:
                    arguments = {"input": raw}
            else:
                arguments = raw if isinstance(raw, dict) else {"input": raw}
            message = {
                "role": "assistant",
                "content": "",
                "tool_calls": [
                    {
                        "id": item.get("call_id"),
                        "type": "function",
                        "function": {"name": history_name, "arguments": arguments},
                    }
                ],
            }
            if pending_reasoning:
                message["reasoning_content"] = "\n".join(pending_reasoning)
                pending_reasoning.clear()
            messages.append(message)
        else:
            tool_message = _tool_message(item)
            if tool_message:
                messages.append(tool_message)
    if not messages:
        messages.append({"role": "user", "content": ""})
    return messages


def build_tool_bindings(
    tools: list[dict[str, Any]],
) -> tuple[list[dict[str, Any]], dict[str, ToolBinding]]:
    compiled: list[dict[str, Any]] = []
    bindings: dict[str, ToolBinding] = {}
    used_names: set[str] = set()
    for index, tool in enumerate(tools):
        if not isinstance(tool, dict):
            raise invalid("Every tool must be an object.", param=f"tools[{index}]")
        kind = str(tool.get("type") or "")
        if kind == "web_search":
            # Provider-hosted web search is a phase-5 capability. Codex sends
            # an optional declaration in every turn; it is intentionally not
            # model-visible until a hosted provider is configured.
            continue
        if kind == "namespace":
            namespace = str(tool.get("name") or "")
            nested = tool.get("tools")
            if not namespace or not isinstance(nested, list) or not nested:
                raise invalid(
                    "Namespace tools require a name and a non-empty tools array.",
                    param=f"tools[{index}]",
                )
            for nested_index, nested_tool in enumerate(nested):
                if not isinstance(nested_tool, dict):
                    raise invalid(
                        "Namespace entries must be tool objects.",
                        param=f"tools[{index}].tools[{nested_index}]",
                    )
                nested_kind = str(nested_tool.get("type") or "")
                if nested_kind not in {"function", "custom"}:
                    raise invalid(
                        f"Unsupported namespaced tool type {nested_kind!r}.",
                        param=f"tools[{index}].tools[{nested_index}].type",
                    )
                nested_name = str(nested_tool.get("name") or "")
                if not nested_name:
                    raise invalid(
                        "Namespaced tool name is required.",
                        param=f"tools[{index}].tools[{nested_index}].name",
                    )
                model_name = _model_visible_name(f"{namespace}__{nested_name}")
                if model_name in used_names:
                    raise ResponsesError(
                        "ambiguous_tool_binding",
                        f"Multiple tools lower to the model-visible name {model_name!r}.",
                        400,
                        f"tools[{index}].tools[{nested_index}].name",
                    )
                used_names.add(model_name)
                schema = (
                    nested_tool.get("parameters")
                    if isinstance(nested_tool.get("parameters"), dict)
                    else {"type": "object", "properties": {}}
                )
                format_spec = (
                    nested_tool.get("format")
                    if isinstance(nested_tool.get("format"), dict)
                    else None
                )
                if nested_kind == "custom":
                    schema = {
                        "type": "object",
                        "properties": {"input": {"type": "string"}},
                        "required": ["input"],
                        "additionalProperties": False,
                    }
                binding = ToolBinding(
                    model_name=model_name,
                    original_name=nested_name,
                    kind=nested_kind,
                    schema=schema,
                    format=format_spec,
                    namespace=namespace,
                )
                bindings[model_name] = binding
                compiled.append(
                    {
                        "type": "function",
                        "function": {
                            "name": model_name,
                            "description": str(nested_tool.get("description") or ""),
                            "parameters": schema,
                        },
                    }
                )
            continue
        original_name = str(
            tool.get("name")
            or tool.get("server_label")
            or tool.get("namespace")
            or kind
        )
        if not original_name:
            raise invalid("Tool name is required.", param=f"tools[{index}].name")
        model_name = _model_visible_name(original_name)
        if model_name in used_names:
            raise ResponsesError(
                "ambiguous_tool_binding",
                f"Multiple tools lower to the model-visible name {model_name!r}.",
                400,
                f"tools[{index}].name",
            )
        used_names.add(model_name)
        schema = tool.get("parameters") if isinstance(tool.get("parameters"), dict) else None
        format_spec = tool.get("format") if isinstance(tool.get("format"), dict) else None
        description = str(tool.get("description") or "")
        if kind == "custom":
            schema = {
                "type": "object",
                "properties": {"input": {"type": "string"}},
                "required": ["input"],
                "additionalProperties": False,
            }
            description = (
                f"{description}\nReturn the custom tool input verbatim in the `input` argument."
            ).strip()
            if format_spec and format_spec.get("type") == "grammar":
                syntax = str(format_spec.get("syntax") or "grammar")
                definition = str(format_spec.get("definition") or "")
                if definition:
                    description = (
                        f"{description}\nThe raw input MUST match this {syntax} grammar exactly:\n"
                        f"{definition}"
                    )
        elif kind == "tool_search":
            schema = schema or {
                "type": "object",
                "properties": {
                    "query": {"type": "string"},
                    "namespace": {"type": "string"},
                },
                "required": ["query"],
                "additionalProperties": False,
            }
        elif kind in {"shell", "local_shell"}:
            schema = schema or {
                "type": "object",
                "properties": {"command": {"type": "string"}},
                "required": ["command"],
            }
        elif kind == "apply_patch":
            schema = schema or {
                "type": "object",
                "properties": {"patch": {"type": "string"}},
                "required": ["patch"],
            }
        elif kind == "mcp":
            schema = schema or {
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "arguments": {"type": "object"},
                },
                "required": ["name", "arguments"],
            }
        elif kind != "function":
            raise invalid(f"Unsupported tool type {kind!r}.", param=f"tools[{index}].type")
        schema = schema or {"type": "object", "properties": {}}
        binding = ToolBinding(
            model_name=model_name,
            original_name=original_name,
            kind=kind,
            schema=schema,
            format=format_spec,
            namespace=str(tool.get("namespace") or "") or None,
            caller="server" if tool.get("server_url") else "client",
            authorization="hosted_allowlist" if tool.get("server_url") else "client_delegated",
        )
        bindings[model_name] = binding
        compiled.append(
            {
                "type": "function",
                "function": {
                    "name": model_name,
                    "description": description,
                    "parameters": schema,
                },
            }
        )
    return compiled, bindings


def compile_messages(
    messages: list[dict[str, Any]],
    *,
    model: str,
    generation_id: str,
    tools: list[dict[str, Any]] | None = None,
    tool_choice: Any = "auto",
    max_output_tokens: int = DEFAULT_MAX_OUTPUT_TOKENS,
    temperature: float = 1.0,
    top_p: float = 1.0,
    top_k: int = 0,
    enable_thinking: bool = True,
    structured_format: dict[str, Any] | None = None,
    prompt_cache_key: str | None = None,
    prompt_builder: Callable[..., str | list[int]] | None = None,
    request: dict[str, Any] | None = None,
    context_items: list[dict[str, Any]] | None = None,
) -> CompiledTurn:
    """Compile an already-neutral turn into a `CompiledTurn`.

    This is the protocol-neutral core shared by every wire surface. It knows
    about chat-shaped messages, the flat tool declarations `build_tool_bindings`
    consumes, and sampling — and nothing about Responses items or Chat request
    objects. The native Responses surface reaches it through `compile_turn`
    below; the Chat Completions surface reaches it through `chat_api.front`.
    Neither surface is expressed in terms of the other.

    `messages` is mutated in place with tool directives, so callers pass a list
    they own.
    """
    tools, bindings = build_tool_bindings(list(tools or []))
    if structured_format:
        # A JSON grammar owns every sampled token of a structured turn, so an
        # open thinking span in the prompt could never be closed; the model
        # would be forced to emit its JSON "inside" <think>. Render the prompt
        # with thinking off so the template pre-closes the span instead.
        enable_thinking = False
    directives: list[str] = []
    if tool_choice == "none":
        tools = []
        bindings = {}
    elif tool_choice == "required":
        if not bindings:
            raise invalid("tool_choice required needs at least one tool.", param="tool_choice")
        if len(bindings) == 1:
            selected = next(iter(bindings))
            directives.append(
                f"You must call the tool `{selected}` now. Do not answer with prose."
            )
        else:
            directives.append(
                "You must call one of the provided tools now. Do not answer with prose."
            )
    elif isinstance(tool_choice, dict):
        selected_name = tool_choice.get("name")
        function = tool_choice.get("function")
        if not selected_name and isinstance(function, dict):
            selected_name = function.get("name")
        selected = next(
            (
                binding.model_name
                for binding in bindings.values()
                if selected_name in {binding.original_name, binding.model_name}
            ),
            None,
        )
        if selected is None:
            raise invalid("tool_choice names an unavailable tool.", param="tool_choice")
        directives.append(
            f"You must call the tool `{selected}` now. Do not answer with prose."
        )
    elif tool_choice != "auto":
        raise invalid("tool_choice must be auto, none, required, or a named tool.", param="tool_choice")
    if bindings:
        available = ", ".join(f"`{name}`" for name in bindings)
        directives.insert(
            0,
            "The only callable tool names are: "
            f"{available}. Never invent, abbreviate, or call any other tool name.",
        )
    if directives:
        # Laguna's template promotes the leading system message into the
        # prompt's header <system> block — the block that also carries the
        # <available_tools> listing. Prepending each directive as its own
        # system message therefore displaced the caller's own system prompt
        # from the header on every tool-bearing turn, a structural difference
        # from training-shaped prompts. Append to the caller's system message
        # instead; only invent one when the caller sent none.
        joined = "\n\n".join(directives)
        if messages and messages[0].get("role") == "system":
            existing = str(messages[0].get("content") or "")
            messages[0] = {
                **messages[0],
                "content": f"{existing}\n\n{joined}".strip(),
            }
        else:
            messages.insert(0, {"role": "system", "content": joined})
    prompt = None
    if prompt_builder is not None:
        prompt = prompt_builder(
            messages,
            tools=tools or None,
            add_generation_prompt=True,
            enable_thinking=enable_thinking,
        )
    return CompiledTurn(
        generation_id=generation_id,
        model=model,
        request=request if request is not None else {},
        context_items=context_items if context_items is not None else [],
        messages=messages,
        prompt=prompt,
        tools=tools,
        bindings=bindings,
        max_output_tokens=max_output_tokens,
        temperature=temperature,
        top_p=top_p,
        top_k=top_k,
        enable_thinking=enable_thinking,
        structured_format=structured_format if isinstance(structured_format, dict) else None,
        prompt_cache_key=prompt_cache_key,
    )


def compile_turn(
    request: dict[str, Any],
    context_items: list[dict[str, Any]],
    generation_id: str,
    *,
    prompt_builder: Callable[..., str | list[int]] | None = None,
    defaults: SamplingDefaults | None = None,
) -> CompiledTurn:
    """Responses front-end onto `compile_messages`.

    Lowering Responses items to chat-shaped prompt messages happens here, inside
    prompt compilation, which the contract explicitly permits. The immutable
    `ToolBinding` table restores the original call kind on the way out.
    """
    reasoning = request.get("reasoning") or {}
    text = request.get("text") or {}
    structured_format = text.get("format") if isinstance(text, dict) else None
    cache_key = request.get("prompt_cache_key")
    # Absent request fields fall back to the caller-supplied defaults (the
    # settings store's live object); the built-ins reproduce historical
    # behavior when no defaults were handed in.
    sampling = defaults or SamplingDefaults()
    return compile_messages(
        items_to_messages(request, context_items),
        model=str(request["model"]),
        generation_id=generation_id,
        tools=list(request.get("tools") or []),
        tool_choice=request.get("tool_choice", "auto"),
        max_output_tokens=int(
            request.get("max_output_tokens") or sampling.max_output_tokens
        ),
        temperature=float(request.get("temperature", sampling.temperature)),
        top_p=float(request.get("top_p", sampling.top_p)),
        top_k=int(request.get("top_k") or sampling.top_k),
        # Absent effort means the configured default (thinking on unless the
        # settings store says otherwise) — the same rule the Chat surface
        # applies. Only `none` turns thinking off; other spellings were
        # rejected upstream.
        enable_thinking=(reasoning.get("effort") or sampling.reasoning_effort)
        != "none",
        structured_format=structured_format,
        prompt_cache_key=cache_key if isinstance(cache_key, str) and cache_key else None,
        prompt_builder=prompt_builder,
        request=request,
        context_items=context_items,
    )
