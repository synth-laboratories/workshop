#!/usr/bin/python3 -I
"""Scripted stdio app-server for the confined mailbox executor tests.

It never calls a model. On turn/start it probes the confinement it is running
under (file reads, directory listing, writes and network) and asks for a
command approval, then reports what happened as its final agent message.
The probe targets arrive in the (untrusted) turn text as KEY=value lines.
"""
import json
import os
import socket
import sys


def send(message):
    sys.stdout.write(json.dumps(message, separators=(",", ":")) + "\n")
    sys.stdout.flush()


def read_one():
    line = sys.stdin.readline()
    return json.loads(line) if line else None


def probe_read(path):
    try:
        with open(path, "r", encoding="utf-8") as handle:
            return "read:" + handle.read().strip()
    except PermissionError:
        return "denied"
    except OSError as error:
        return "error:" + type(error).__name__


def probe_list(path):
    try:
        os.listdir(path)
        return "listed"
    except PermissionError:
        return "denied"
    except OSError as error:
        return "error:" + type(error).__name__


def probe_write(path):
    try:
        with open(path, "w", encoding="utf-8") as handle:
            handle.write("escape")
        return "written"
    except PermissionError:
        return "denied"
    except OSError as error:
        return "error:" + type(error).__name__


def probe_connect(host, port):
    sock = socket.socket()
    sock.settimeout(2)
    try:
        sock.connect((host, int(port)))
        return "connected"
    except PermissionError:
        return "denied"
    except OSError as error:
        return "error:" + type(error).__name__
    finally:
        sock.close()


def targets(params):
    text = " ".join(item.get("text", "") for item in params.get("input", []))
    found = {}
    for token in text.split():
        if "=" in token and token.split("=", 1)[0].startswith("PROBE_"):
            key, value = token.split("=", 1)
            found[key] = value
    return found


while True:
    message = read_one()
    if message is None:
        break
    method = message.get("method")
    request_id = message.get("id")
    params = message.get("params") or {}
    if method == "initialize":
        send({"jsonrpc": "2.0", "id": request_id, "result": {"userAgent": "fake-restricted-codex/1"}})
    elif method == "thread/start":
        send({"jsonrpc": "2.0", "id": request_id, "result": {"thread": {"id": "thread-restricted"},
              "echo": {"approvalPolicy": params.get("approvalPolicy"), "sandbox": params.get("sandbox"),
                       "ephemeral": params.get("ephemeral")}}})
    elif method == "turn/start":
        send({"jsonrpc": "2.0", "id": request_id, "result": {"turn": {"id": "turn-restricted"}}})
        probe = targets(params)
        allowed_dir = os.path.join(os.getcwd(), "allowed")
        allowed_files = sorted(os.listdir(allowed_dir)) if os.path.isdir(allowed_dir) else []
        home = os.environ.get("CODEX_HOME", "")
        with open(os.path.join(home, "config.toml"), "r", encoding="utf-8") as handle:
            config = handle.read()
        report = {
            "secret": probe_read(probe.get("PROBE_SECRET", "/nonexistent")),
            "listing": probe_list(probe.get("PROBE_LIST", "/nonexistent")),
            "write_outside": probe_write(probe.get("PROBE_WRITE", "/nonexistent/x")),
            "allowed": probe_read(os.path.join(allowed_dir, allowed_files[0])) if allowed_files else "missing",
            "provider": probe_connect("127.0.0.1", probe.get("PROBE_PROVIDER_PORT", "1")),
            "other_loopback": probe_connect("127.0.0.1", probe.get("PROBE_OTHER_PORT", "1")),
            "external": probe_connect("1.1.1.1", 443),
            "shell_disabled": "shell_tool = false" in config and "unified_exec = false" in config,
            "no_mcp": "[mcp_servers" not in config,
            "env_keys": sorted(os.environ.keys()),
            "turn": {"approvalPolicy": params.get("approvalPolicy"), "sandboxPolicy": params.get("sandboxPolicy")},
        }
        # A tool attempt: the executor must decline it.
        send({"jsonrpc": "2.0", "id": 9001, "method": "item/commandExecution/requestApproval",
              "params": {"command": "cat ~/.ssh/id_rsa", "cwd": os.getcwd(),
                         "availableDecisions": ["accept", "acceptForSession", "decline", "cancel"]}})
        decision = None
        while True:
            reply = read_one()
            if reply is None:
                break
            if reply.get("id") == 9001:
                decision = (reply.get("result") or {}).get("decision") or ("error" if "error" in reply else None)
                break
        report["approval_decision"] = decision
        send({"jsonrpc": "2.0", "method": "item/completed", "params": {"item": {
            "id": "msg-1", "type": "agentMessage", "text": json.dumps(report, sort_keys=True)}}})
        send({"jsonrpc": "2.0", "method": "turn/completed", "params": {"turn": {"id": "turn-restricted", "status": "completed"}}})
    elif request_id is not None and method is not None:
        send({"jsonrpc": "2.0", "id": request_id, "error": {"code": -32601, "message": "unsupported"}})
