"""Deterministic stdio MCP server for real Codex/Laguna integration gates."""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path
from typing import Any


TOOLS = [
    {
        "name": "fixture_echo",
        "description": "Echo one value. Use this when explicitly asked to verify MCP echo dispatch.",
        "inputSchema": {
            "type": "object",
            "properties": {"value": {"type": "string"}},
            "required": ["value"],
            "additionalProperties": False,
        },
    },
    {
        "name": "fixture_sum",
        "description": "Add two integers. Use this when explicitly asked to verify MCP arithmetic dispatch.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "a": {"type": "integer"},
                "b": {"type": "integer"},
            },
            "required": ["a", "b"],
            "additionalProperties": False,
        },
    },
]


def reply(request_id: Any, result: dict[str, Any]) -> None:
    print(json.dumps({"jsonrpc": "2.0", "id": request_id, "result": result}), flush=True)


def fail(request_id: Any, message: str) -> None:
    print(
        json.dumps(
            {
                "jsonrpc": "2.0",
                "id": request_id,
                "error": {"code": -32602, "message": message},
            }
        ),
        flush=True,
    )


def record(entry: dict[str, Any]) -> None:
    configured = os.getenv("FIXTURE_MCP_LOG")
    if not configured:
        return
    path = Path(configured)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(entry, sort_keys=True) + "\n")


def main() -> int:
    label = os.getenv("FIXTURE_MCP_LABEL", "fixture")
    for raw_line in sys.stdin:
        try:
            request = json.loads(raw_line)
        except json.JSONDecodeError:
            continue
        method = request.get("method")
        request_id = request.get("id")
        params = request.get("params") or {}
        if method == "initialize":
            reply(
                request_id,
                {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": f"laguna-{label}-fixture", "version": "1.0.0"},
                },
            )
        elif method in {"notifications/initialized", "notifications/cancelled"}:
            continue
        elif method == "ping":
            reply(request_id, {})
        elif method == "tools/list":
            reply(request_id, {"tools": TOOLS})
        elif method == "tools/call":
            name = params.get("name")
            arguments = params.get("arguments") or {}
            record({"label": label, "name": name, "arguments": arguments})
            if name == "fixture_echo" and isinstance(arguments.get("value"), str):
                text = f"MCP_ECHO:{arguments['value']}"
            elif name == "fixture_sum" and all(
                isinstance(arguments.get(key), int) for key in ("a", "b")
            ):
                text = f"MCP_SUM:{arguments['a'] + arguments['b']}"
            else:
                fail(request_id, f"invalid call for {name!r}")
                continue
            reply(
                request_id,
                {
                    "content": [{"type": "text", "text": text}],
                    "structuredContent": {"result": text},
                    "isError": False,
                },
            )
        else:
            fail(request_id, f"unsupported method {method!r}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
