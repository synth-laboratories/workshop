#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
import urllib.request
from pathlib import Path
from typing import Any


OPENAI_COMMIT = "c309ca176bc22c6075a0c2c2543f2ac4f307c447"
OPENAI_SHA256 = "cdabcdfc529b1ec0582009bb2ef7d06b64a66d4f6644e66142305a48f0b7658d"
OPENAI_URL = f"https://raw.githubusercontent.com/openai/openai-openapi/{OPENAI_COMMIT}/openapi.json"
OUTPUT = Path(__file__).parents[1] / "schemas" / "openai" / OPENAI_COMMIT / "responses-extension-overlay.json"

SEEDS = {
    "ApplyPatchToolCall",
    "ApplyPatchToolCallItemParam",
    "ApplyPatchToolCallOutput",
    "ApplyPatchToolCallOutputItemParam",
    "ApplyPatchToolParam",
    "CustomGrammarFormatParam",
    "CustomTextFormatParam",
    "CustomToolCall",
    "CustomToolCallOutputResource",
    "CustomToolCallResource",
    "CustomToolParam",
    "FunctionShellCall",
    "FunctionShellCallItemParam",
    "FunctionShellCallOutput",
    "FunctionShellCallOutputItemParam",
    "FunctionShellToolParam",
    "LocalShellToolCall",
    "LocalShellToolCallOutput",
    "LocalShellToolParam",
    "MCPApprovalRequest",
    "MCPApprovalResponseResource",
    "MCPListTools",
    "MCPTool",
    "MCPToolCall",
    "NamespaceToolParam",
    "ResponseCustomToolCallInputDeltaEvent",
    "ResponseCustomToolCallInputDoneEvent",
    "ResponseMCPCallArgumentsDeltaEvent",
    "ResponseMCPCallArgumentsDoneEvent",
    "ResponseMCPCallCompletedEvent",
    "ResponseMCPCallFailedEvent",
    "ResponseMCPCallInProgressEvent",
    "ResponseMCPListToolsCompletedEvent",
    "ResponseMCPListToolsFailedEvent",
    "ResponseMCPListToolsInProgressEvent",
    "ResponsesClientEventResponseCreate",
    "ToolChoiceCustom",
    "ToolChoiceMCP",
}

REF_PATTERN = re.compile(r"^#/components/schemas/(.+)$")


def referenced_names(value: Any) -> set[str]:
    found: set[str] = set()
    if isinstance(value, dict):
        reference = value.get("$ref")
        if isinstance(reference, str):
            match = REF_PATTERN.match(reference)
            if match:
                found.add(match.group(1))
        for child in value.values():
            found.update(referenced_names(child))
    elif isinstance(value, list):
        for child in value:
            found.update(referenced_names(child))
    return found


def build(source: dict[str, Any]) -> dict[str, Any]:
    schemas = source["components"]["schemas"]
    selected: dict[str, Any] = {}
    pending = list(sorted(SEEDS))
    while pending:
        name = pending.pop()
        if name in selected:
            continue
        if name not in schemas:
            raise RuntimeError(f"OpenAI schema is missing required definition {name}")
        selected[name] = schemas[name]
        pending.extend(sorted(referenced_names(schemas[name]) - selected.keys()))
    return {
        "openapi": "3.1.0",
        "info": {
            "title": "Synth Laguna OpenAI/Codex Responses extension overlay",
            "version": OPENAI_COMMIT,
        },
        "x-source": {
            "url": OPENAI_URL,
            "sha256": OPENAI_SHA256,
            "license": "MIT",
        },
        "x-synth-paths": [
            "GET /responses/{response_id}",
            "DELETE /responses/{response_id}",
            "POST /responses/{response_id}/cancel",
            "GET /responses/{response_id}/input_items",
            "POST /responses/input_tokens",
        ],
        "components": {"schemas": {key: selected[key] for key in sorted(selected)}},
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--source", type=Path)
    args = parser.parse_args()
    raw = args.source.read_bytes() if args.source else urllib.request.urlopen(OPENAI_URL, timeout=60).read()
    digest = hashlib.sha256(raw).hexdigest()
    if digest != OPENAI_SHA256:
        raise RuntimeError(f"OpenAI schema hash mismatch: expected {OPENAI_SHA256}, got {digest}")
    rendered = json.dumps(build(json.loads(raw)), indent=2, sort_keys=True) + "\n"
    if args.check:
        if not OUTPUT.exists() or OUTPUT.read_text(encoding="utf-8") != rendered:
            print(f"generated overlay differs: {OUTPUT}", file=sys.stderr)
            return 1
        print(f"schema overlay is reproducible: {OUTPUT}")
        return 0
    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_text(rendered, encoding="utf-8")
    print(OUTPUT)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
