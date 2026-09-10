#!/usr/bin/env python3
"""A scriptable Codex app-server stand-in that speaks -- and enforces -- the wire protocol.

A real child process over real pipes, not a mock: the failures worth testing here
are process-shaped (a server that dies mid-turn, one that never answers, one that
redelivers an event) and an in-process double cannot produce any of them.

It **validates what the client sends**, which is the point. A permissive fixture
accepts whatever the code under test happens to emit, so both drift together and
the suite stays green while the real runtime would reject every call. The rules
below are transcribed from the desktop client in
`apps/synth_desktop/src-tauri/src/session/codex/manager.rs`:

  - `initialize` first, with `clientInfo.name` and `clientInfo.version`.
  - an `initialized` notification before any thread is opened; the real server
    treats the connection as unready until it arrives.
  - `thread/start` carries model, cwd, approvalPolicy and sandbox.
  - `turn/start` carries threadId and `input` as a list of typed content items,
    never a bare string.

Violations come back as JSON-RPC errors, so a client that regresses fails loudly.

Scenario is one JSON argument:

  initialize   result for `initialize` (or {"__error__": {...}} to fail the handshake)
  thread       result for `thread/start` / `thread/resume`
  turn         result for `turn/start`
  transcript   path to append every received message to, for contract assertions
  lenient      true to skip validation, for tests about malformed servers
  events       what to emit after the turn is acknowledged, in order

An event is a notification `{"method":..., "params":...}` or a directive:

  {"__sleep__": seconds}      stall, to drive deadline handling
  {"__crash__": true}         exit abruptly, mid-turn
  {"__repeat__": n, ...}      emit the same notification n times, same item id
  {"__approval__": {...}}     send a server->client approval request and wait
"""
import json
import os
import sys
import time

SCENARIO = json.loads(sys.argv[1]) if len(sys.argv) > 1 else {}
STRICT = not SCENARIO.get("lenient")
STATE = {"initialized": False, "handshaken": False, "thread": None}


class Violation(Exception):
    def __init__(self, message):
        super().__init__(message)
        self.message = message


def write(message):
    sys.stdout.write(json.dumps(message) + "\n")
    sys.stdout.flush()


def notify(method, params):
    write({"jsonrpc": "2.0", "method": method, "params": params})


def read():
    line = sys.stdin.readline()
    if not line:
        raise SystemExit(0)
    message = json.loads(line)
    path = SCENARIO.get("transcript")
    if path:
        with open(path, "a", encoding="utf-8") as handle:
            handle.write(json.dumps(message) + "\n")
    return message


def check(method, params):
    """Raise Violation when the client breaks the handshake contract."""
    if not STRICT:
        return
    if method == "initialize":
        info = (params or {}).get("clientInfo") or {}
        for field in ("name", "version"):
            if not info.get(field):
                raise Violation(f"initialize requires clientInfo.{field}")
        return
    if method == "initialized":
        return
    if not STATE["handshaken"]:
        raise Violation(f"{method} before initialize")
    if not STATE["initialized"]:
        raise Violation(f"{method} before the initialized notification")
    if method in {"thread/start", "thread/resume"}:
        for field in ("model", "cwd", "approvalPolicy", "sandbox"):
            if params.get(field) in (None, ""):
                raise Violation(f"{method} requires {field}")
        if method == "thread/resume" and not params.get("threadId"):
            raise Violation("thread/resume requires threadId")
        return
    if method == "turn/start":
        if not params.get("threadId"):
            raise Violation("turn/start requires threadId")
        supplied = params.get("input")
        if not isinstance(supplied, list) or not supplied:
            raise Violation("turn/start input must be a non-empty list of content items")
        for item in supplied:
            if not isinstance(item, dict) or item.get("type") != "text" or not isinstance(item.get("text"), str):
                raise Violation("turn/start input items must be {type: text, text: ...}")
        if not params.get("approvalPolicy"):
            raise Violation("turn/start requires approvalPolicy")


def emit(events):
    rpc_id = 9000
    for event in events:
        if "__sleep__" in event:
            time.sleep(event["__sleep__"])
            continue
        if event.get("__crash__"):
            os._exit(9)
        if "__request__" in event:
            rpc_id += 1
            request = event["__request__"]
            write({"jsonrpc": "2.0", "id": rpc_id, "method": request["method"],
                   "params": request.get("params", {})})
            reply = read()
            notify("request/observed", {"result": reply.get("result"), "error": reply.get("error")})
            continue
        if "__approval__" in event:
            rpc_id += 1
            request = event["__approval__"]
            write({"jsonrpc": "2.0", "id": rpc_id, "method": request.get("method", "permissions/request"),
                   "params": request.get("params", {})})
            reply = read()
            notify("approval/observed", {"decision": (reply.get("result") or {}).get("decision"),
                                         "error": reply.get("error")})
            continue
        for _ in range(event.get("__repeat__", 1)):
            notify(event["method"], event.get("params", {}))


def main():
    while True:
        message = read()
        method, rpc_id, params = message.get("method"), message.get("id"), message.get("params") or {}
        try:
            check(method, params)
        except Violation as violation:
            if rpc_id is None:
                notify("session/unhealthy", {"reason": violation.message})
                continue
            write({"jsonrpc": "2.0", "id": rpc_id,
                   "error": {"code": -32602, "message": f"protocol violation: {violation.message}"}})
            continue
        if method == "initialize":
            configured = SCENARIO.get("initialize", {"serverInfo": {"name": "fake-codex", "version": "0.0.1"}})
            if "__error__" in configured:
                write({"jsonrpc": "2.0", "id": rpc_id, "error": configured["__error__"]})
                continue
            STATE["handshaken"] = True
            write({"jsonrpc": "2.0", "id": rpc_id, "result": configured})
        elif method == "initialized":
            STATE["initialized"] = True
        elif method in {"thread/start", "thread/resume"}:
            result = SCENARIO.get("thread", {"threadId": "thread-1"})
            STATE["thread"] = result.get("threadId")
            write({"jsonrpc": "2.0", "id": rpc_id, "result": result})
        elif method == "turn/start":
            write({"jsonrpc": "2.0", "id": rpc_id, "result": SCENARIO.get("turn", {"turnId": "turn-1"})})
            emit(SCENARIO.get("events", []))
        elif method == "turn/interrupt":
            if STRICT and not (params.get("threadId") and params.get("turnId")):
                write({"jsonrpc": "2.0", "id": rpc_id,
                       "error": {"code": -32602, "message": "turn/interrupt requires threadId and turnId"}})
                continue
            if rpc_id is not None:
                write({"jsonrpc": "2.0", "id": rpc_id, "result": {}})
            notify("turn/interrupted", {"turn": {"id": "turn-1", "status": "interrupted"}})
        elif rpc_id is not None:
            write({"jsonrpc": "2.0", "id": rpc_id, "error": {"code": -32601, "message": f"No method {method}"}})


if __name__ == "__main__":
    main()
