from __future__ import annotations

import secrets


_PREFIXES = {
    "response": "resp",
    "message": "msg",
    "function_call": "fc",
    "function_call_output": "fco",
    "custom_tool_call": "ctc",
    "custom_tool_call_output": "ctco",
    "reasoning": "rs",
    "compaction": "cmp",
    "shell_call": "shc",
    "shell_call_output": "shco",
    "apply_patch_call": "apc",
    "apply_patch_call_output": "apco",
    "mcp_call": "mcp",
    "mcp_list_tools": "mcpl",
    "tool_search_call": "tsc",
    "item": "item",
    "call": "call",
}


def new_id(kind: str) -> str:
    prefix = _PREFIXES.get(kind, kind)
    return f"{prefix}_{secrets.token_hex(12)}"


def ensure_id(value: object, kind: str) -> str:
    if isinstance(value, str) and value:
        return value
    return new_id(kind)
