#!/usr/bin/env python3
"""Exercise the real Workshop MCP bridge and running desktop, without a model.

Creates one shared diagram in the selected instance, updates and presents it,
then verifies a native PNG capture. This is a real app integration check, not a
provider evaluation. Run only against an instance intended for acceptance work.
"""

import argparse
import json
from pathlib import Path
import selectors
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--data-root", type=Path, required=True)
    parser.add_argument("--receipt", type=Path, required=True)
    parser.add_argument("--fullscreen", action="store_true", help="Exercise native full-screen presentation, then exit full screen")
    args = parser.parse_args()
    root = args.data_root.resolve(strict=True)
    process = subprocess.Popen(
        [str(args.binary.resolve(strict=True)), "mcp", "--data-root", str(root)],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True,
    )
    selector = selectors.DefaultSelector()
    selector.register(process.stdout, selectors.EVENT_READ)
    sequence = 0
    presented_visual = None

    def request(method, params):
        nonlocal sequence
        sequence += 1
        process.stdin.write(json.dumps({"jsonrpc": "2.0", "id": sequence, "method": method, "params": params}) + "\n")
        process.stdin.flush()
        if not selector.select(timeout=65):
            raise RuntimeError(f"MCP timed out during {method}")
        line = process.stdout.readline()
        if not line:
            raise RuntimeError(f"MCP exited during {method}; check Workshop diagnostics")
        response = json.loads(line)
        assert response["id"] == sequence, response
        assert "error" not in response, response
        return response["result"]

    def tool(name, arguments, failure=False):
        result = request("tools/call", {"name": name, "arguments": arguments})
        assert bool(result.get("isError")) == failure, f"Unexpected tool outcome: {name}: {result}"
        return result if failure else result["structuredContent"]

    try:
        initialized = request("initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                                               "clientInfo": {"name": "workshop-acceptance", "version": "1"}})
        assert initialized["instructions"]
        process.stdin.write(json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n")
        process.stdin.flush()
        request("ping", {})
        names = [entry["name"] for entry in request("tools/list", {})["tools"]]
        assert len(names) == len(set(names))
        catalogue = tool("visual_list_templates", {"genre": "diagram.mermaid.v1"})
        assert any(template["id"] == "diagram.mermaid.v1" for template in catalogue["templates"])
        visual = tool("visual_create", {"template_id": "diagram.mermaid.v1", "title": "Workshop MCP acceptance",
            "content": "flowchart LR\nInput[Input] --> Analysis[Analysis] --> Result[Result]"})["visual"]
        assert visual["sessionId"] is None and visual["workspaceId"]
        visual_id, old_revision = visual["id"], visual["currentRevision"]
        updated = tool("visual_update", {"visual_id": visual_id, "expected_revision": old_revision,
            "title": "Workshop MCP connection verified"})["visual"]
        assert updated["currentRevision"] == old_revision + 1
        assert updated["workspaceId"] == visual["workspaceId"]
        tool("visual_update", {"visual_id": visual_id, "expected_revision": old_revision,
                               "title": "Stale write must fail"}, failure=True)
        presented_visual = visual_id
        tool("visual_present", {"visual_id": visual_id, "fullscreen": args.fullscreen})
        capture = request("tools/call", {"name": "visual_capture", "arguments": {"visual_id": visual_id}})
        assert not capture.get("isError"), capture
        assert any(item["type"] == "image" and item["mimeType"] == "image/png" for item in capture["content"])
        captured = capture["structuredContent"]
        image = Path(captured["receipt"]["path"]).resolve(strict=True)
        assert image.is_relative_to(root)
        assert image.read_bytes().startswith(b"\x89PNG\r\n\x1a\n")
        assert captured["revision"] == updated["currentRevision"]
        assert captured["receipt"]["windowFullscreen"] == args.fullscreen, captured["receipt"]
        observation = tool("visual_observe", {"visual_id": visual_id})
        evidence = {"server": initialized["serverInfo"], "tools": names, "visualId": visual_id,
                    "workspaceId": visual["workspaceId"], "revision": captured["revision"],
                    "capture": captured["receipt"], "observation": observation,
                    "staleWriteRejected": True, "providerCalls": 0}
        args.receipt.parent.mkdir(parents=True, exist_ok=True)
        args.receipt.write_text(json.dumps(evidence, indent=2) + "\n")
        print(json.dumps({"passed": True, "visualId": visual_id, "image": str(image), "receipt": str(args.receipt)}))
    finally:
        try:
            if args.fullscreen and presented_visual and process.poll() is None:
                tool("visual_present", {"visual_id": presented_visual, "fullscreen": False})
        finally:
            selector.close()
            process.stdin.close()
            try:
                process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                process.terminate()
                process.wait(timeout=3)


if __name__ == "__main__":
    main()
