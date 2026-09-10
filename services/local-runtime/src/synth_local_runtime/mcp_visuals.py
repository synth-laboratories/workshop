"""Minimal stdio MCP-ish JSON-RPC shim for visual tools (local agents)."""

from __future__ import annotations

import json
import sys
from pathlib import Path

from .config import RuntimeConfig
from .service import RuntimeService


def _ok(id_value, result):
    return {"jsonrpc": "2.0", "id": id_value, "result": result}


def _err(id_value, message: str):
    return {"jsonrpc": "2.0", "id": id_value, "error": {"code": -32000, "message": message}}


def main() -> int:
    workshop = Path(__file__).resolve().parents[4]
    config = RuntimeConfig.from_env(
        host="127.0.0.1",
        port=0,
        data_dir=Path.home() / ".synth-desktop" / "runtime" / "mcp-data",
    )
    # Force workshop visuals root for MCP process.
    object.__setattr__(config, "workshop_root", workshop)
    object.__setattr__(config, "visuals_root", workshop / "visuals")
    service = RuntimeService(config)

    tools_path = workshop / "visuals" / "mcp" / "tools.json"
    tools = json.loads(tools_path.read_text(encoding="utf-8"))

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            req = json.loads(line)
        except json.JSONDecodeError:
            continue
        req_id = req.get("id")
        method = req.get("method")
        params = req.get("params") or {}

        try:
            if method == "tools/list":
                print(json.dumps(_ok(req_id, tools)), flush=True)
                continue
            if method == "tools/call":
                name = params.get("name")
                args = params.get("arguments") or {}
                if name == "visual_list_templates":
                    result = {"templates": service.list_visual_templates()}
                elif name == "visual_create_from_template":
                    result = service.create_visual(
                        {
                            "templateId": args["template_id"],
                            "title": args.get("title"),
                            "bindings": args.get("props") or {},
                            "id": args.get("instance_id"),
                        }
                    )
                elif name == "visual_bind_data_source":
                    visual = service.get_visual(args["instance_id"])
                    bindings = dict(visual.get("bindings") or {})
                    input_name = args.get("input") or args.get("slot") or "primary"
                    bindings[input_name] = args.get("binding") or args
                    result = service.update_visual(args["instance_id"], {"bindings": bindings})
                elif name == "visual_save_tsx":
                    result = service.save_visual_tsx(
                        args["instance_id"], tsx=args.get("tsx")
                    )
                elif name == "visual_open_in_pane":
                    result = {
                        "opened": True,
                        "visual": service.get_visual(args["instance_id"]),
                    }
                elif name == "visual_stream_live_eval":
                    result = service.simulate_live_eval(kind=str(args.get("kind") or "eval"))
                else:
                    print(json.dumps(_err(req_id, f"unknown tool {name}")), flush=True)
                    continue
                print(json.dumps(_ok(req_id, result)), flush=True)
                continue
            print(json.dumps(_err(req_id, f"unknown method {method}")), flush=True)
        except Exception as exc:  # noqa: BLE001
            print(json.dumps(_err(req_id, str(exc))), flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
