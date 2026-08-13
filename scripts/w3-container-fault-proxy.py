#!/usr/bin/env python3
"""Loopback-only, file-controlled W3 fault proxy for a Containers service.

The proxy never modifies the upstream service.  Faults are scoped by exact
rollout id and are disabled by deleting/resetting the state file.
"""

from __future__ import annotations

import argparse
import json
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlsplit


def load_state(path: Path) -> dict:
    try:
        value = json.loads(path.read_text())
        return value if isinstance(value, dict) else {}
    except (OSError, json.JSONDecodeError):
        return {}


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_GET(self) -> None:  # noqa: N802
        state = load_state(self.server.state_file)
        route = urlsplit(self.path).path
        rollout = str(state.get("rollout_id") or "")
        prefix = f"/rollouts/{rollout}" if rollout else ""
        if prefix and route == f"{prefix}/events" and state.get("poll_503") is True:
            self.reply(503, {"error": "w3_injected_poll_unavailable", "retryable": True})
            return
        if prefix and route.startswith(f"{prefix}/frames/") and state.get("frame_404") is True:
            self.reply(404, {"detail": "w3_injected_frame_not_found"})
            return
        self.forward()

    def do_POST(self) -> None:  # noqa: N802
        state = load_state(self.server.state_file)
        route = urlsplit(self.path).path
        if route in {"/rollouts", "/rollout"} and state.get("policy_pin_refusal") is True:
            length = int(self.headers.get("content-length", "0"))
            body = self.rfile.read(length) if length else b"{}"
            try:
                requested = json.loads(body)
            except json.JSONDecodeError:
                requested = {}
            rollout = str(state.get("rollout_id") or "")
            if rollout and requested.get("rollout_id") == rollout:
                self.reply(
                    403,
                    {
                        "error": "bind_refused",
                        "affordance": "bind_policy_config",
                        "reason": "w3_injected_policy_pin_refusal",
                    },
                )
                return
            self.forward(body)
            return
        self.forward()

    def do_PUT(self) -> None:  # noqa: N802
        self.forward()

    def do_DELETE(self) -> None:  # noqa: N802
        self.forward()

    def forward(self, body: bytes | None = None) -> None:
        if body is None:
            length = int(self.headers.get("content-length", "0"))
            body = self.rfile.read(length) if length else None
        headers = {
            name: value
            for name, value in self.headers.items()
            if name.lower() not in {"host", "content-length", "connection"}
        }
        request = urllib.request.Request(
            f"{self.server.upstream}{self.path}",
            data=body,
            headers=headers,
            method=self.command,
        )
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                payload = response.read()
                self.send_response(response.status)
                for name, value in response.headers.items():
                    if name.lower() not in {"connection", "content-length", "transfer-encoding"}:
                        self.send_header(name, value)
        except urllib.error.HTTPError as error:
            payload = error.read()
            self.send_response(error.code)
            for name, value in error.headers.items():
                if name.lower() not in {"connection", "content-length", "transfer-encoding"}:
                    self.send_header(name, value)
        self.send_header("content-length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def reply(self, status: int, payload: dict) -> None:
        body = json.dumps(payload, separators=(",", ":")).encode()
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, pattern: str, *args: object) -> None:
        print(f"w3-proxy {self.address_string()} {pattern % args}", flush=True)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--listen", default="127.0.0.1:18097")
    parser.add_argument("--upstream", default="http://127.0.0.1:8097")
    parser.add_argument("--state-file", required=True, type=Path)
    args = parser.parse_args()
    host, raw_port = args.listen.rsplit(":", 1)
    if host not in {"127.0.0.1", "localhost", "::1"}:
        raise SystemExit("W3 proxy refuses non-loopback --listen addresses")
    server = ThreadingHTTPServer((host, int(raw_port)), Handler)
    server.upstream = args.upstream.rstrip("/")
    server.state_file = args.state_file.resolve()
    print(
        json.dumps(
            {
                "listening": f"http://{args.listen}",
                "upstream": server.upstream,
                "state_file": str(server.state_file),
                "faults_default": "off",
            }
        ),
        flush=True,
    )
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        server.server_close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
