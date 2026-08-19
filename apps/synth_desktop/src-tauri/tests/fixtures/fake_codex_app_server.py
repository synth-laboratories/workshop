#!/usr/bin/env python3
"""Small stdio JSON-RPC app-server used by Rust process-boundary tests."""

import json
import os
import pathlib
import subprocess
import sys

if sys.argv[1:] == ["debug", "models", "--bundled"]:
    print(json.dumps({
        "models": [{
            "slug": "fixture-fallback",
            "base_instructions": "Fixture bundled model instructions.",
            "context_window": 272000,
            "max_context_window": 1000000,
            "input_modalities": ["text", "image"],
            "supports_image_detail_original": True,
        }]
    }))
    raise SystemExit(0)


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
reject_thread_resume = home / "reject-thread-resume"
request_approval_on_turn_start = home / "request-approval-on-turn-start"
complete_before_turn_start_response = home / "complete-before-turn-start-response"
final_answer_then_exit = home / "final-answer-then-exit"
ignore_interrupt_and_spawn_sleeper = home / "ignore-interrupt-and-spawn-sleeper"
sleeping_child_pid = home / "sleeping-child.pid"
approval_response_path = home / "approval-response.json"


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
    if method is None and isinstance(request_id, int) and request_id >= 9000:
        approval_response_path.write_text(json.dumps(message, separators=(",", ":")), encoding="utf-8")
        send({
            "jsonrpc": "2.0",
            "method": "turn/completed",
            "params": {"turn": {"id": "turn-fixture-approval", "status": "completed", "items": []}},
        })
        continue
    if request_id is None:
        continue
    # Desktop's answer to a server-originated approval request. It is already
    # captured in the JSONL log above; there is no request method to dispatch.
    if method is None:
        continue

    if method == "initialize":
        result = {"userAgent": "synth-fake-app-server/1"}
    elif method == "thread/start":
        result = {"thread": {"id": "thread-fixture"}}
    elif method == "thread/resume":
        if reject_thread_resume.exists():
            send({
                "jsonrpc": "2.0",
                "id": request_id,
                "error": {"code": -32600, "message": f"no rollout found for thread id {params.get('threadId')}"},
            })
            continue
        result = {"thread": {"id": params.get("threadId", "thread-fixture")}}
    elif method == "thread/name/set":
        result = {}
    elif method == "thread/loaded/list":
        result = {"data": [params.get("threadId", "thread-fixture")]}
    elif method == "thread/compact/start":
        result = {}
    elif method == "turn/start":
        if exit_on_turn_start.exists():
            if exit_on_turn_start.read_text(encoding="utf-8").strip() == "once":
                exit_on_turn_start.unlink()
            sys.exit(0)
        turn_number += 1
        turn_id = f"turn-fixture-{os.getpid()}-{turn_number}"
        if ignore_interrupt_and_spawn_sleeper.exists():
            child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(30)"])
            sleeping_child_pid.write_text(str(child.pid), encoding="utf-8")
        if complete_before_turn_start_response.exists():
            complete_before_turn_start_response.unlink()
            send({
                "jsonrpc": "2.0",
                "method": "turn/completed",
                "params": {
                    "threadId": params.get("threadId", "thread-fixture"),
                    "turn": {"id": turn_id, "status": "completed", "items": []},
                },
            })
        result = {"turn": {"id": turn_id}}
    elif method == "turn/interrupt":
        result = {}
    elif method == "turn/steer":
        result = {"turnId": params.get("expectedTurnId")}
    elif method == "thread/compact/start":
        result = {}
    else:
        send({
            "jsonrpc": "2.0",
            "id": request_id,
            "error": {"code": -32601, "message": f"unsupported fixture method: {method}"},
        })
        continue

    send({"jsonrpc": "2.0", "id": request_id, "result": result})
    if method == "turn/start" and request_approval_on_turn_start.exists():
        send({
            "jsonrpc": "2.0",
            "id": 9000 + turn_number,
            "method": "item/commandExecution/requestApproval",
            "params": {
                "command": "printf fixture",
                "cwd": str(home.parent),
                "availableDecisions": ["decline", "accept", "acceptForSession"],
            },
        })
    if method == "turn/start" and final_answer_then_exit.exists():
        send({
            "jsonrpc": "2.0",
            "method": "item/agentMessage",
            "params": {
                "item": {
                    "id": "message-final-fixture",
                    "type": "agentMessage",
                    "text": "FINAL_ANSWER_OK",
                    "phase": "final_answer",
                },
            },
        })
        sys.exit(0)
    if method == "thread/compact/start":
        compact_turn_id = "compact-fixture-1"
        send({
            "jsonrpc": "2.0",
            "method": "thread/compacted",
            "params": {
                "threadId": params.get("threadId", "thread-fixture"),
                "turnId": compact_turn_id,
            },
        })
        send({
            "jsonrpc": "2.0",
            "method": "turn/completed",
            "params": {
                "threadId": params.get("threadId", "thread-fixture"),
                "turn": {
                    "id": compact_turn_id,
                    "status": "completed",
                    "items": [],
                },
            },
        })
    if method == "turn/interrupt" and not ignore_interrupt_and_spawn_sleeper.exists():
        send({
            "jsonrpc": "2.0",
            "method": "turn/interrupted",
            "params": {
                "threadId": params.get("threadId"),
                "turnId": params.get("turnId"),
                "status": "interrupted",
            },
        })
