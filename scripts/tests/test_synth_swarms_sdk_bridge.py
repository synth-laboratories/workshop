from __future__ import annotations

import json
import os
import pathlib
import subprocess
import sys
import threading
import unittest
import importlib.util
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

ROOT = pathlib.Path(__file__).resolve().parents[2]
BRIDGE = pathlib.Path(os.environ.get("SYNTH_SWARMS_BRIDGE", ROOT / "scripts/synth-swarms-sdk-bridge.py"))
SDK_ROOT = pathlib.Path(
    os.environ.get(
        "SYNTH_AI_PROJECT_ROOT",
        "/Users/joshuapurtell/GitHub/worktrees/syn-3263-sdk-release-version",
    )
)

ATTESTATION = {
    "schema_version": "smr.execution-target.v1",
    "actor_host": "docker",
    "capacity_authority": "bound_runtime",
    "audience": "synth-smr-run-start",
    "provider": "local_slot_manager",
    "attestation_id": "att-local-1",
    "provider_resource_id": "slot-local-1",
    "control_slot_instance_id": "slot-instance-1",
    "runtime_id": "runtime-local-1",
    "dispatch_pool": "local-docker",
    "claim_id": "claim-local-1",
    "runtime_generation": "1",
    "signer_key_id": "local-test-key",
    "signature": "test-signature",
}


class SlotHandler(BaseHTTPRequestHandler):
    requests: list[dict] = []

    def log_message(self, *_args):
        return

    def _json(self, status: int, body: dict):
        encoded = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(encoded)))
        self.end_headers()
        self.wfile.write(encoded)

    def do_POST(self):
        length = int(self.headers.get("Content-Length", "0"))
        body = json.loads(self.rfile.read(length) or b"{}")
        self.requests.append({"method": "POST", "path": self.path, "body": body})
        if self.path == "/smr/projects/project-local-1/launch-preflight":
            self._json(200, {"project_id": "project-local-1", "clear_to_trigger": True, "blockers": []})
            return
        if self.path == "/smr/projects/project-local-1/trigger":
            self._json(200, {
                "run_id": "swarm-local-1", "project_id": "project-local-1", "org_id": "org-local-1",
                "public_state": "running", "runbook": "lite", "trigger": "workshop",
                "created_at": "2026-09-16T18:00:00Z", "updated_at": "2026-09-16T18:00:01Z",
                "work_completed": False,
            })
            return
        self._json(404, {"detail": "not found"})

    def do_GET(self):
        self.requests.append({"method": "GET", "path": self.path, "last_event_id": self.headers.get("Last-Event-ID")})
        if self.path.startswith("/smr/runs/swarm-local-1/runtime/stream"):
            envelope = {
                "schema_version": "research.swarm_event.v1",
                "sequence": "evt-2",
                "run_id": "swarm-local-1",
                "ts": "2026-09-16T18:00:02Z",
                "kind": "heartbeat",
                "payload": {"version": "1", "source": "smr_control_plane.redis.runtime_stream.v1"},
                "state_version": "2",
                "transcript_cursor": "cursor-2",
            }
            encoded = f"id: evt-2\nevent: heartbeat\ndata: {json.dumps(envelope)}\n\n".encode()
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Content-Length", str(len(encoded)))
            self.end_headers()
            self.wfile.write(encoded)
            return
        self._json(404, {"detail": "not found"})


class BridgeLocalSlotTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.server = ThreadingHTTPServer(("127.0.0.1", 0), SlotHandler)
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()
        cls.base_url = f"http://127.0.0.1:{cls.server.server_port}/api/v1"

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()

    def call(self, operation: str, payload: dict) -> list[dict]:
        env = {**os.environ, "PYTHONPATH": str(SDK_ROOT), "PYTHONDONTWRITEBYTECODE": "1"}
        command = [str(BRIDGE), operation] if os.access(BRIDGE, os.X_OK) else [sys.executable, str(BRIDGE), operation]
        completed = subprocess.run(
            command,
            input=json.dumps({"baseUrl": self.base_url, "apiKey": "local-slot-test", "payload": payload}),
            text=True,
            capture_output=True,
            env=env,
            timeout=15,
            check=False,
        )
        self.assertEqual(completed.returncode, 0, completed.stderr + completed.stdout)
        return [json.loads(line) for line in completed.stdout.splitlines() if line.strip()]

    def test_preflight_create_and_resume_sse_on_a_local_slot(self):
        spec = {
            "objective": "Exercise the Workshop local slot SDK path",
            "runbook": "lite",
            "timeboxSeconds": 120,
            "agentModel": "gpt-5.6-luna",
            "provider": "auto",
            "idempotencyKey": "workshop-local-slot-test",
            "limit": {"maxSpendUsd": 0, "maxConcurrentActors": 2, "maxWallclockSeconds": 120},
            "executionTarget": {"kind": "bound_runtime", "attestation": ATTESTATION},
        }
        preflight = self.call("preflight", {"projectId": "project-local-1", "spec": spec})[0]
        self.assertTrue(preflight["result"]["clear_to_trigger"])
        created = self.call("create", {"projectId": "project-local-1", "spec": spec})[0]
        self.assertEqual(created["result"]["swarm_id"], "swarm-local-1")
        event = self.call("watch", {"swarmId": "swarm-local-1", "lastEventId": "evt-1", "timeoutSeconds": 5})[0]
        self.assertEqual(event["event"]["sequence"], "evt-2")
        trigger = next(item for item in SlotHandler.requests if item["path"] == "/smr/projects/project-local-1/trigger")
        self.assertEqual(trigger["body"]["execution_target"]["attestation"]["provider"], "local_slot_manager")
        self.assertEqual(trigger["body"]["limit"]["max_spend_usd"], 0)
        stream = next(item for item in SlotHandler.requests if item["method"] == "GET")
        self.assertEqual(stream["last_event_id"], "evt-1")

    def test_hosted_spec_defers_model_harness_and_provider_to_backend_preset(self):
        sys.path.insert(0, str(SDK_ROOT))
        spec = importlib.util.spec_from_file_location("workshop_swarms_bridge", BRIDGE)
        self.assertIsNotNone(spec)
        self.assertIsNotNone(spec.loader)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        value = module._spec({
            "objective": "Exercise the hosted Workshop SDK path",
            "runbook": "lite",
            "agentModel": "gpt-5.6-luna",
            "agentHarness": "codex",
            "provider": "auto",
            "executionTarget": {"kind": "platform_resolved"},
        }, hosted=True)
        wire = value.to_wire()
        self.assertNotIn("agent_model", wire)
        self.assertNotIn("agent_harness", wire)
        self.assertNotIn("provider", wire)
        self.assertEqual(wire["runbook"], "lite")


if __name__ == "__main__":
    unittest.main()
