from __future__ import annotations

import json
import hashlib
import re
from typing import Any, Callable

from .backends.protocol import CompiledTurn, ToolBinding
from .errors import ResponsesError, invalid


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
                messages.append(
                    {
                        "role": "system" if role == "developer" else role,
                        "content": _content_text(item.get("content")),
                    }
                )
        elif kind == "reasoning":
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
            raw = item.get("arguments", item.get("input", item.get("action", {})))
            if isinstance(raw, str):
                try:
                    arguments = json.loads(raw)
                except ValueError:
                    arguments = {"input": raw}
            else:
                arguments = raw if isinstance(raw, dict) else {"input": raw}
            messages.append(
                {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [
                        {
                            "id": item.get("call_id"),
                            "type": "function",
                            "function": {"name": name, "arguments": arguments},
                        }
                    ],
                }
            )
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


def compile_turn(
    request: dict[str, Any],
    context_items: list[dict[str, Any]],
    generation_id: str,
    *,
    prompt_builder: Callable[..., str | list[int]] | None = None,
) -> CompiledTurn:
    messages = items_to_messages(request, context_items)
    tools, bindings = build_tool_bindings(list(request.get("tools") or []))
    tool_choice = request.get("tool_choice", "auto")
    if tool_choice == "none":
        tools = []
        bindings = {}
    elif tool_choice == "required":
        if not bindings:
            raise invalid("tool_choice required needs at least one tool.", param="tool_choice")
        if len(bindings) == 1:
            selected = next(iter(bindings))
            directive = (
                f"You must call the tool `{selected}` now. Do not answer with prose."
            )
        else:
            directive = "You must call one of the provided tools now. Do not answer with prose."
        messages.insert(0, {"role": "system", "content": directive})
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
        messages.insert(
            0,
            {
                "role": "system",
                "content": f"You must call the tool `{selected}` now. Do not answer with prose.",
            },
        )
    elif tool_choice != "auto":
        raise invalid("tool_choice must be auto, none, required, or a named tool.", param="tool_choice")
    if bindings:
        available = ", ".join(f"`{name}`" for name in bindings)
        messages.insert(
            0,
            {
                "role": "system",
                "content": (
                    "The only callable tool names are: "
                    f"{available}. Never invent, abbreviate, or call any other tool name."
                ),
            },
        )
    reasoning = request.get("reasoning") or {}
    enable_thinking = reasoning.get("effort") not in {None, "none", "minimal"}
    prompt = None
    if prompt_builder is not None:
        prompt = prompt_builder(
            messages,
            tools=tools or None,
            add_generation_prompt=True,
            enable_thinking=enable_thinking,
        )
    text = request.get("text") or {}
    structured_format = text.get("format") if isinstance(text, dict) else None
    return CompiledTurn(
        generation_id=generation_id,
        model=str(request["model"]),
        request=request,
        context_items=context_items,
        messages=messages,
        prompt=prompt,
        tools=tools,
        bindings=bindings,
        max_output_tokens=int(request.get("max_output_tokens") or 1024),
        temperature=float(request.get("temperature", 1.0)),
        top_p=float(request.get("top_p", 1.0)),
        structured_format=structured_format if isinstance(structured_format, dict) else None,
    )
