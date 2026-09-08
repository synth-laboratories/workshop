#!/usr/bin/env python3
"""Native runtime acceptance using a deterministic ACP peer (no provider calls).

Requires a dedicated running instance. Temporarily installs a private fixture
backend, exercises real MCP, storage, approvals and process lifecycle, then
restores the backend file. Does not modify real client configuration.
"""
import argparse
import json
from pathlib import Path
import selectors
import subprocess
import sys
import time
import jsonschema


class MCP:
    def __init__(self, binary, root):
        self.process = subprocess.Popen([str(binary), "mcp", "--data-root", str(root)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        self.selector = selectors.DefaultSelector()
        self.selector.register(self.process.stdout, selectors.EVENT_READ)
        self.sequence = 0
        self.schemas = {}
        self.request("initialize", {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "runtime-acceptance", "version": "1"}})

    def request(self, method, params):
        self.sequence += 1
        self.process.stdin.write(json.dumps({"jsonrpc": "2.0", "id": self.sequence, "method": method, "params": params}) + "\n")
        self.process.stdin.flush()
        assert self.selector.select(timeout=60), f"timeout: {method}"
        line = self.process.stdout.readline()
        if not line:
            raise AssertionError(f"MCP exited during {method}: {self.process.stderr.read()}")
        response = json.loads(line)
        assert response.get("id") == self.sequence and "error" not in response, response
        return response["result"]

    def call(self, name, args=None, error=False):
        response = self.request("tools/call", {"name": name, "arguments": args or {}})
        assert bool(response.get("isError")) == error, (name, response)
        if error:
            return response
        value = response["structuredContent"]
        if name in self.schemas:
            jsonschema.Draft202012Validator(self.schemas[name]).validate(value)
        return value.get("result", value)

    def close(self):
        self.process.stdin.close()
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.terminate()
            self.process.wait(timeout=5)


def until(fn, timeout=15):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = fn()
        if result:
            return result
        time.sleep(.1)
    raise AssertionError("runtime state did not settle")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--data-root", type=Path, required=True)
    parser.add_argument("--receipt", type=Path, required=True)
    args = parser.parse_args()
    binary, root = args.binary.resolve(strict=True), args.data_root.resolve(strict=True)
    fixture = Path(__file__).resolve().parent / "fixtures/workshop-acp-agent.py"
    registry = root / "agent-backends.json"
    assert not registry.exists(), "use a dedicated instance without a backend registry"
    registry.write_text(json.dumps([{"id": "acceptance", "command": str(Path(sys.executable).resolve()), "args": [str(fixture)], "workspace": str(fixture.parent), "envFile": None, "maxSessions": 2, "maxTurnSeconds": 10}]))
    registry.chmod(0o600)
    client = None
    sessions = []
    try:
        client = MCP(binary, root)
        catalogue = client.request("tools/list", {})["tools"]
        client.schemas = {tool["name"]: tool["outputSchema"] for tool in catalogue if "outputSchema" in tool}
        for tool in catalogue:
            jsonschema.Draft202012Validator.check_schema(tool["inputSchema"])
            if "outputSchema" in tool:
                jsonschema.Draft202012Validator.check_schema(tool["outputSchema"])
        names = {tool["name"] for tool in catalogue}
        assert len(names) == len(catalogue) and len(names) >= 338
        assert "agent_session_start" in names and "visuals_observation_report" not in names
        initial = client.call("runtime_status")
        client.call("runtime_control", {"action": "detach"})
        until(lambda: not client.call("runtime_status")["desktopAttached"])
        assert client.call("runtime_status")["processId"] == initial["processId"]
        client.call("visual_list")
        client.call("core_diagnostics")
        assert client.call("agent_backends_list")[0]["id"] == "acceptance"
        start = client.call("agent_session_start", {"request": {"backendId": "acceptance", "title": "ACP native acceptance", "parentSessionId": None}})
        sid = start["sessionId"]
        sessions.append(sid)
        def row():
            return next(item for item in client.call("agent_sessions_list")["sessions"] if item["sessionId"] == sid)
        def history():
            return client.call("core_session_events_after", {"sessionId": sid, "afterSequence": 0, "limit": 500})
        client.call("agent_session_send", {"sessionId": sid, "text": "Native ACP echo"})
        until(lambda: row()["status"] == "ready")
        assert any(event["kind"] == "agent.update" and event["payload"].get("content", {}).get("text") == "Native ACP echo" for event in history())
        client.call("agent_session_send", {"sessionId": sid, "text": "permission"})
        approval = until(lambda: next((event for event in history() if event["kind"] == "approval.requested"), None))
        denied = client.call("codex_approval_resolve", {"request": {"sessionId": sid, "approvalId": approval["payload"]["approvalId"], "decision": "once"}}, error=True)
        assert "human_action_required" in json.dumps(denied)
        client.call("agent_session_cancel", {"sessionId": sid})
        until(lambda: row()["status"] == "ready")
        assert any(event["kind"] == "approval.expired" for event in history())
        client.call("agent_session_send", {"sessionId": sid, "text": "permission-complete"})
        until(lambda: row()["status"] == "ready")
        def no_pending_approvals():
            events = history()
            pending = {event["payload"]["approvalId"] for event in events if event["kind"] == "approval.requested"}
            settled = {event["payload"]["approvalId"] for event in events if event["kind"] in ("approval.expired", "approval.granted", "approval.rejected")}
            return not (pending - settled)
        until(no_pending_approvals)
        client.call("agent_session_close", {"sessionId": sid})
        assert not row()["attached"]
        client.call("agent_session_resume", {"sessionId": sid})
        assert row()["attached"]
        until(lambda: sum(event["kind"] == "agent.update" for event in history()) >= 151)
        client.call("agent_session_send", {"sessionId": sid, "text": "ignore-cancel"})
        client.call("agent_session_cancel", {"sessionId": sid})
        until(lambda: not row()["attached"], timeout=12)
        assert row()["status"] == "interrupted", row()
        client.call("agent_session_resume", {"sessionId": sid})
        client.call("agent_session_send", {"sessionId": sid, "text": "crash"})
        until(lambda: not row()["attached"])
        assert row()["status"] == "failed", row()
        client.call("runtime_control", {"action": "attach"})
        until(lambda: client.call("runtime_status")["desktopAttached"])
        assert client.call("runtime_status")["processId"] == initial["processId"]
        receipt = {"providerCalls": 0, "toolCount": len(catalogue), "runtime": initial, "sessionId": sid, "checks": ["generated discovery", "headless same-process survival", "headless visual reads", "ACP initialize/new/prompt", "journaled streaming", "agent cannot self-approve", "permission cancellation", "completed-turn permission expiry", "explicit load with replay", "noncooperative cancellation", "EOF terminalizes run", "same-process desktop attach"]}
        args.receipt.write_text(json.dumps(receipt, indent=2) + "\n")
        print(json.dumps(receipt, indent=2))
    finally:
        if client:
            for sid in sessions:
                try:
                    client.call("agent_session_close", {"sessionId": sid})
                except Exception:
                    pass
            client.close()
        registry.unlink()


if __name__ == "__main__":
    main()
