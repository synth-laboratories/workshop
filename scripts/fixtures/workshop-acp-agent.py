#!/usr/bin/env python3
"""Deterministic ACP v1 acceptance peer. No model, network, or credentials."""
import json
import sys

pending = None
permission = None
remote = "fixture-session"


def emit(value):
    print(json.dumps({"jsonrpc": "2.0", **value}), flush=True)


def respond(message, result):
    emit({"id": message["id"], "result": result})


for line in sys.stdin:
    message = json.loads(line)
    method = message.get("method")
    if method == "initialize":
        assert message["params"]["protocolVersion"] == 1
        respond(message, {"protocolVersion": 1, "agentCapabilities": {"loadSession": True}})
    elif method in ("session/new", "session/load"):
        assert message["params"]["cwd"].startswith("/")
        assert message["params"]["mcpServers"][0]["name"] == "workshop"
        if method == "session/load":
            # Exercise replay larger than the host event queue during handshake.
            for index in range(150):
                emit({"method": "session/update", "params": {"sessionId": remote, "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "replay"}}}})
        respond(message, {"sessionId": remote})
    elif method == "session/prompt":
        text = message["params"]["prompt"][0]["text"]
        if text == "crash":
            sys.exit(7)
        elif text in ("wait", "ignore-cancel"):
            pending = message
            pending["ignore"] = text == "ignore-cancel"
        elif text in ("permission", "permission-complete"):
            permission = message
            emit({"id": "approval-1", "method": "session/request_permission", "params": {"sessionId": remote, "toolCall": {"toolCallId": "test-tool", "title": "Fixture permission request"}, "options": [{"kind": "allow_once", "optionId": "allow", "name": "Allow once"}, {"kind": "reject_once", "optionId": "reject", "name": "Reject"}]}})
            if text == "permission-complete":
                respond(message, {"stopReason": "end_turn"})
                permission = None
        else:
            emit({"method": "session/update", "params": {"sessionId": remote, "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}}}})
            respond(message, {"stopReason": "end_turn"})
    elif method == "session/cancel":
        if pending and not pending.get("ignore"):
            respond(pending, {"stopReason": "cancelled"})
            pending = None
    elif message.get("id") == "approval-1" and permission:
        assert message["result"]["outcome"]["outcome"] in ("selected", "cancelled")
        respond(permission, {"stopReason": "cancelled" if message["result"]["outcome"]["outcome"] == "cancelled" else "end_turn"})
        permission = None
    elif "id" in message:
        emit({"id": message["id"], "error": {"code": -32601, "message": "Unsupported fixture method"}})
