from __future__ import annotations

import importlib.util
import json
import tempfile
import threading
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "modern_stack_dogfood.py"
SPEC = importlib.util.spec_from_file_location("modern_stack_dogfood", SCRIPT)
assert SPEC and SPEC.loader
dogfood = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(dogfood)


class FakeWorkshop(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    calls: list[tuple[str, str, dict]] = []
    visual: dict = {}
    recipe_availability = "available"
    recipe_prerequisites: list[str] = []

    def log_message(self, *_args):
        return

    def payload(self) -> dict:
        length = int(self.headers.get("Content-Length", "0"))
        return json.loads(self.rfile.read(length) or b"{}")

    def send_json(self, value: dict, status: int = 200) -> None:
        raw = json.dumps(value).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def do_GET(self):
        body = self.payload()
        self.calls.append(("GET", self.path, body))
        if self.path == "/v1/optimizers/recipes":
            return self.send_json(
                {
                    "recipes": [
                        {
                            "id": "gepa.banking77.luna.v1",
                            "availability": type(self).recipe_availability,
                            "prerequisites": type(self).recipe_prerequisites,
                        }
                    ]
                }
            )
        if self.path.endswith("/events"):
            after = body.get("after_seq", 0)
            events = []
            if after == 0:
                kinds = (
                    "proposer.delta",
                    "candidate.registered",
                    "candidate.evaluated",
                    "frontier.updated",
                    "child.evaluation.completed",
                )
                events = [
                    {
                        "sequenceNumber": index,
                        "type": kind,
                        "delta": {"safe": True},
                    }
                    for index, kind in enumerate(kinds, start=1)
                ]
            return self.send_json({"events": events})
        if self.path == "/v1/optimizers/runs/run_gepa":
            return self.send_json(
                {
                    "run": {
                        "id": "run_gepa",
                        "status": "completed",
                        "visualRefs": [{"kind": "visual", "id": "vis_gepa"}],
                        "inputRefs": [],
                        "outputRefs": [],
                        "executionBindings": [],
                        "usage": {"costUsd": None},
                        "error": None,
                    }
                }
            )
        if self.path == "/v1/visuals/vis_harbor":
            return self.send_json({"visual": self.visual})
        if self.path.endswith("/rollouts/roll_harbor"):
            return self.send_json({"state": {"status": "completed", "reward": 1.0}})
        return self.send_json({"error": "not found"}, 404)

    def do_POST(self):
        body = self.payload()
        self.calls.append(("POST", self.path, body))
        if self.path == "/v1/optimizers/recipes/run":
            return self.send_json(
                {
                    "run": {
                        "id": "run_gepa",
                        "status": "queued",
                        "cursorSeq": 0,
                        "visualRefs": [{"kind": "visual", "id": "vis_gepa"}],
                    },
                    "event": {},
                }
            )
        if self.path == "/v1/containers":
            return self.send_json(
                {
                    "container": {
                        "id": "ctr_harbor",
                        "metadata": {
                            "liveEval": {
                                "family": "harbor",
                                "templateId": "live.harbor_eval.v1",
                                "slot": "stream",
                                "policyRefs": body["metadata"]["policyRefs"],
                            }
                        },
                    },
                    "liveEval": {
                        "family": "harbor",
                        "templateId": "live.harbor_eval.v1",
                        "slot": "stream",
                        "policyRefs": body["metadata"]["policyRefs"],
                    },
                }
            )
        if self.path.endswith("/rollouts/prepare"):
            stream = {
                "transports": {
                    "sse": {"url": "/rollouts/roll_harbor/events/stream"},
                    "poll": {"url": "/rollouts/roll_harbor/events"},
                },
                "cursor": {"kind": "sequence"},
            }
            binding = {
                "slot": "stream",
                "kind": "live_sse",
                "source": "http://127.0.0.1:4567/rollouts/roll_harbor/events/stream",
                "poll_url": "http://127.0.0.1:4567/rollouts/roll_harbor/events",
                "schema": "synth.trace-stream-event.v1",
            }
            return self.send_json(
                {
                    "prepared": {"rollout_id": "roll_harbor", "stream": stream},
                    "stream": stream,
                    "visual_binding": binding,
                    "resolved": {"sse_url": binding["source"], "poll_url": binding["poll_url"]},
                }
            )
        if self.path == "/v1/visuals":
            type(self).visual = {
                "id": "vis_harbor",
                "currentRevision": 1,
                "templateId": body["templateId"],
                "bindings": body["bindings"],
                "metadata": body["metadata"],
            }
            return self.send_json({"visual": type(self).visual, "event": {}})
        if self.path.endswith("/show"):
            return self.send_json({"shown": True})
        if self.path.endswith("/rollouts/start"):
            return self.send_json(
                {
                    "started": True,
                    "subscription": {"kind": "stream.subscribed", "ready": True},
                    "state": {"status": "running"},
                }
            )
        if self.path.endswith("/rollouts/poll"):
            return self.send_json(
                {
                    "page": {
                        "rollout_id": "roll_harbor",
                        "events": [
                            {"sequence": 1, "kind": "trial.started"},
                            {"sequence": 2, "kind": "verifier.completed"},
                            {"sequence": 3, "kind": "capture.closed"},
                        ],
                        "cursor": {"high_water": 3, "closed": True},
                    },
                    "next_cursor": 3,
                }
            )
        return self.send_json({"error": "not found"}, 404)


class ModernStackDogfoodTest(unittest.TestCase):
    def setUp(self):
        FakeWorkshop.calls = []
        FakeWorkshop.visual = {}
        FakeWorkshop.recipe_availability = "available"
        FakeWorkshop.recipe_prerequisites = []
        self.server = ThreadingHTTPServer(("127.0.0.1", 0), FakeWorkshop)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.connection = self.root / "connection.json"
        self.connection.write_text(
            json.dumps(
                {
                    "url": f"http://127.0.0.1:{self.server.server_port}",
                    "token": "super-secret-workshop-token",
                }
            )
        )

    def tearDown(self):
        self.server.shutdown()
        self.server.server_close()
        self.temp.cleanup()

    def test_optimizer_writes_complete_redacted_receipt(self):
        receipt = self.root / "optimizer"
        code = dogfood.main(
            [
                "--connection",
                str(self.connection),
                "--receipt-dir",
                str(receipt),
                "--timeout",
                "2",
                "--poll-interval",
                "0.01",
                "optimizer",
                "--recipe",
                "gepa.banking77.luna.v1",
                "--execute",
            ]
        )
        self.assertEqual(code, 0)
        result = json.loads((receipt / "receipt.json").read_text())
        self.assertEqual(result["status"], "PASS")
        self.assertTrue(result["checks"]["proposal_visible"])
        self.assertTrue(result["checks"]["frontier_visible"])
        self.assertNotIn("super-secret-workshop-token", "".join(p.read_text() for p in receipt.glob("*.*")))
        for expected in dogfood.ReceiptBundle.STANDARD:
            self.assertTrue((receipt / expected).is_file(), expected)

    def test_optimizer_execute_fails_closed_when_catalog_reports_unavailable(self):
        FakeWorkshop.recipe_availability = "unavailable"
        FakeWorkshop.recipe_prerequisites = ["TINKER_API_KEY", "hosted sampler"]
        receipt = self.root / "optimizer-unavailable"

        code = dogfood.main(
            [
                "--connection",
                str(self.connection),
                "--receipt-dir",
                str(receipt),
                "optimizer",
                "--recipe",
                "gepa.banking77.luna.v1",
                "--execute",
            ]
        )

        self.assertEqual(code, 2)
        result = json.loads((receipt / "receipt.json").read_text())
        self.assertEqual(result["status"], "BLOCKED")
        self.assertTrue(result["executionAuthorized"])
        self.assertEqual(result["blockers"][0]["code"], "recipe_unavailable")
        self.assertFalse(
            any(path == "/v1/optimizers/recipes/run" for _, path, _ in FakeWorkshop.calls)
        )

    def test_container_prepare_stops_for_review_then_resumes_exact_stream(self):
        receipt = self.root / "harbor"
        prepare = dogfood.main(
            [
                "--connection",
                str(self.connection),
                "--receipt-dir",
                str(receipt),
                "--timeout",
                "2",
                "container-prepare",
                "--base-url",
                "http://127.0.0.1:4567",
                "--family",
                "harbor",
                "--name",
                "Harbor fixture",
                "--rollout-id",
                "roll_harbor",
                "--task-instance-id",
                "gamebench-task-1",
                "--execute",
            ]
        )
        self.assertEqual(prepare, 2)
        prepared_receipt = json.loads((receipt / "receipt.json").read_text())
        self.assertEqual(prepared_receipt["status"], "BLOCKED")
        post_paths = [path for method, path, _ in FakeWorkshop.calls if method == "POST"]
        self.assertLess(post_paths.index("/v1/visuals"), post_paths.index("/v1/visuals/vis_harbor/show"))
        self.assertFalse(any(path.endswith("/rollouts/start") for path in post_paths))

        FakeWorkshop.visual["metadata"]["qualityGate"] = {"ready": True, "revision": 1}
        start = dogfood.main(
            [
                "--connection",
                str(self.connection),
                "--receipt-dir",
                str(receipt),
                "--timeout",
                "2",
                "--poll-interval",
                "0.01",
                "container-start",
                "--start-retries",
                "2",
                "--execute",
            ]
        )
        self.assertEqual(start, 0)
        final_receipt = json.loads((receipt / "receipt.json").read_text())
        self.assertEqual(final_receipt["status"], "PASS")
        self.assertTrue(final_receipt["checks"]["stream_subscribed_before_start"])
        self.assertTrue(final_receipt["checks"]["idempotent_start_replay"])
        starts = [path for method, path, _ in FakeWorkshop.calls if method == "POST" and path.endswith("/rollouts/start")]
        self.assertEqual(len(starts), 2)
        counts = json.loads((receipt / "event-kind-counts.json").read_text())
        self.assertEqual(counts["capture.closed"], 1)

    def test_redactor_handles_keys_bearers_and_query_values(self):
        value = dogfood.redact(
            {
                "api_key": "abc",
                "url": "https://example.test/x?token=abc&safe=yes",
                "header": "Bearer xyz.123",
            }
        )
        self.assertEqual(value["api_key"], "[REDACTED]")
        self.assertIn("token=[REDACTED]", value["url"])
        self.assertEqual(value["header"], "Bearer [REDACTED]")


if __name__ == "__main__":
    unittest.main()
