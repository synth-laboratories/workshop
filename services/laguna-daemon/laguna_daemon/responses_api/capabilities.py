from __future__ import annotations

from dataclasses import asdict, dataclass
from typing import Any

from .errors import ResponsesError, unsupported


@dataclass(frozen=True, slots=True)
class ModelCapabilities:
    text: bool = True
    reasoning: bool = True
    function_tools: bool = True
    custom_tools: bool = True
    namespace_tools: bool = True
    mcp_items: bool = True
    shell_items: bool = True
    apply_patch_items: bool = True
    structured_output: bool = True
    images: bool = False
    files: bool = False
    video: bool = False
    audio: bool = False
    hosted_mcp: bool = False
    hosted_web_search: bool = False
    context_length: int = 262_144
    max_output_tokens: int = 32_768

    def json(self) -> dict[str, Any]:
        return asdict(self)


def validate_capabilities(
    request: dict[str, Any], capabilities: ModelCapabilities
) -> None:
    input_items = request.get("input")
    if isinstance(input_items, str) or input_items is None:
        input_items = []
    for item_index, item in enumerate(input_items):
        if not isinstance(item, dict):
            continue
        content = item.get("content")
        if not isinstance(content, list):
            continue
        for content_index, part in enumerate(content):
            if not isinstance(part, dict):
                continue
            kind = part.get("type")
            param = f"input[{item_index}].content[{content_index}]"
            if kind == "input_image" and not capabilities.images:
                raise unsupported("image input", param=param)
            if kind == "input_file" and not capabilities.files:
                raise unsupported("file input", param=param)
            if kind == "input_video" and not capabilities.video:
                raise unsupported("video input", param=param)
            if kind in {"input_audio", "audio"} and not capabilities.audio:
                raise unsupported("audio input", param=param)

    for tool_index, tool in enumerate(request.get("tools") or []):
        if not isinstance(tool, dict):
            continue
        kind = tool.get("type")
        param = f"tools[{tool_index}]"
        allowed = {
            "function": capabilities.function_tools,
            "custom": capabilities.custom_tools,
            "namespace": capabilities.namespace_tools,
            "tool_search": capabilities.namespace_tools,
            "mcp": capabilities.mcp_items,
            "shell": capabilities.shell_items,
            "local_shell": capabilities.shell_items,
            "computer": False,
            "apply_patch": capabilities.apply_patch_items,
        }
        if kind in allowed and not allowed[kind]:
            raise unsupported(f"{kind} tools", param=param)
        if kind == "mcp" and tool.get("server_url") and not capabilities.hosted_mcp:
            raise ResponsesError(
                "hosted_mcp_disabled",
                "Hosted MCP execution is disabled for this model. Use a client-delegated MCP bridge.",
                400,
                param,
            )
        # Codex declares web_search under tool_choice=auto even when it does
        # not require search. Accept that declaration but never offer it to
        # the local model. A forced choice fails before sampling.
        choice = request.get("tool_choice")
        forced_web = isinstance(choice, dict) and choice.get("type") in {
            "web_search",
            "web_search_preview",
        }
        only_web_required = choice == "required" and all(
            candidate.get("type") == "web_search"
            for candidate in request.get("tools") or []
            if isinstance(candidate, dict)
        )
        if kind == "web_search" and (forced_web or only_web_required):
            raise ResponsesError(
                "hosted_web_search_disabled",
                "Hosted web search is not configured for this local model.",
                400,
                param,
            )
