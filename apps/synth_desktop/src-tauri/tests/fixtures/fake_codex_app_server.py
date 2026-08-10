#!/usr/bin/env python3
"""Small stdio JSON-RPC app-server used by Rust process-boundary tests."""

import json
import os
import pathlib
import sys


home = pathlib.Path(os.environ["CODEX_HOME"])
home.mkdir(parents=True, exist_ok=True)
requests_path = home / "fake-app-server-requests.jsonl"
turn_number = 0
# Lifecycle race control. When this marker exists the server dies the moment a
# turn/start arrives, without answering it, which is exactly what a real
# app-server crash between attach and turn start looks like to the manager.
# A marker whose contents are "once" removes itself first, so the very next
# attempt succeeds.
exit_on_turn_start = home / "exit-on-turn-start"


def send(message: dict) -> None:
    sys.stdout.write(json.dumps(message, separators=(",", ":")) + "\n")
    sys.stdout.flush()


for raw in sys.stdin:
    try:
        message = json.loads(raw)
    except json.JSONDecodeError:
        continue

    with requests_path.open("a", encoding="utf-8") as log:
        log.write(json.dumps(message, separators=(",", ":")) + "\n")

    method = message.get("method")
    request_id = message.get("id")
    params = message.get("params") or {}
    if request_id is None:
        continue

    if method == "initialize":
        result = {"userAgent": "synth-fake-app-server/1"}
    elif method == "thread/start":
        result = {"thread": {"id": "thread-fixture"}}
    elif method == "thread/resume":
        result = {"thread": {"id": params.get("threadId", "thread-fixture")}}
    elif method == "thread/name/set":
        result = {}
    elif method == "thread/loaded/list":
        result = {"data": [params.get("threadId", "thread-fixture")]}
    elif method == "turn/start":
        if exit_on_turn_start.exists():
            if exit_on_turn_start.read_text(encoding="utf-8").strip() == "once":
                exit_on_turn_start.unlink()
            sys.exit(0)
        turn_number += 1
        result = {"turn": {"id": f"turn-fixture-{os.getpid()}-{turn_number}"}}
    elif method == "turn/interrupt":
        result = {}
    elif method == "turn/steer":
        result = {"turnId": params.get("expectedTurnId")}
    else:
        send({
            "jsonrpc": "2.0",
            "id": request_id,
            "error": {"code": -32601, "message": f"unsupported fixture method: {method}"},
        })
        continue

    send({"jsonrpc": "2.0", "id": request_id, "result": result})
    if method == "turn/interrupt":
        send({
            "jsonrpc": "2.0",
            "method": "turn/interrupted",
            "params": {
                "threadId": params.get("threadId"),
                "turnId": params.get("turnId"),
                "status": "interrupted",
            },
        })
