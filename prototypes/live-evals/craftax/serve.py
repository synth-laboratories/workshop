#!/usr/bin/env python3
"""Serve the Craftax reference UI and tail one real eval JSONL as SSE."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import tempfile
import time
from datetime import UTC, datetime
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.error import HTTPError, URLError
from urllib.parse import urlparse
from urllib.request import urlopen


def event_files(events_path: Path) -> list[Path]:
    if events_path.is_file():
        return [events_path]
    event_root = events_path / "event_logs" if (events_path / "event_logs").is_dir() else events_path
    return sorted(event_root.glob("*.jsonl")) if event_root.is_dir() else []


def compat_seal_events(events_path: Path) -> list[dict]:
    if events_path.is_file():
        return []
    storage_root = events_path.parent if events_path.name == "event_logs" else events_path
    events: list[dict] = []
    for seal_path in sorted((storage_root / "seals").glob("*.trace-v5.json")):
        try:
            seal = json.loads(seal_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            continue
        digest = seal.get("content_digest")
        rollout_id = str(seal.get("rollout_id") or seal.get("trace_id") or "")
        if not digest or not rollout_id:
            continue
        events.append(
            {
                "kind": "trace.reconciled",
                "source": "synth.trace.v5",
                "occurred_at": datetime.fromtimestamp(seal_path.stat().st_mtime, UTC).isoformat(),
                "run_id": rollout_id,
                "rollout_id": rollout_id,
                "lane": rollout_id,
                "sequence": f"trace:sealed:{digest}",
                "payload": {
                    "trace_id": seal.get("trace_id"),
                    "trace_digest": digest,
                    "high_water": seal.get("high_water"),
                    "closed": seal.get("closed"),
                    "capture_closed": seal.get("capture.closed"),
                },
            }
        )
    return events


def enrich(event: dict, container_base: str, frame_cache: Path) -> dict:
    payload = event.get("payload")
    if not isinstance(payload, dict):
        return event
    kind = str(event.get("kind") or "")
    if kind not in {"snapshot", "frame"}:
        return event
    rollout_id = str(event.get("run_id") or payload.get("rollout_id") or "")
    step = payload.get("step_index", payload.get("step"))
    progress = payload.get("progress")
    if step is None and isinstance(progress, dict):
        step = progress.get("done", progress.get("env_steps"))
    if rollout_id and isinstance(step, int):
        rollout_key = hashlib.sha256(rollout_id.encode()).hexdigest()[:24]
        cached = frame_cache / rollout_key / f"{step}.png"
        if not cached.is_file():
            remote = str(payload.get("frame_url") or payload.get("url") or "")
            parsed_remote = urlparse(remote)
            if not (
                parsed_remote.scheme == "http"
                and parsed_remote.hostname in {"127.0.0.1", "localhost", "::1"}
            ):
                remote = f"{container_base.rstrip('/')}/rollouts/{rollout_id}/frames/{step}.png"
            try:
                with urlopen(remote, timeout=0.75) as response:
                    body = response.read()
                if body.startswith(b"\x89PNG\r\n\x1a\n"):
                    cached.parent.mkdir(parents=True, exist_ok=True)
                    with tempfile.NamedTemporaryFile(dir=cached.parent, delete=False) as staged:
                        staged.write(body)
                        staged.flush()
                        os.fsync(staged.fileno())
                        staged_path = Path(staged.name)
                    os.replace(staged_path, cached)
            except (HTTPError, URLError, TimeoutError, OSError):
                pass
        if not cached.is_file():
            return event
        payload = dict(payload)
        payload["frame_url"] = f"/api/frames/{rollout_key}/{step}.png"
        event = dict(event)
        event["payload"] = payload
    return event


def sealed_trace_events(bundle_root: Path | None) -> list[dict]:
    if bundle_root is None or not bundle_root.is_dir():
        return []
    events: list[dict] = []
    for index_path in sorted(bundle_root.glob("**/trace-index.json")):
        try:
            index = json.loads(index_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            continue
        for entry in index.get("entries") or []:
            if not isinstance(entry, dict) or not entry.get("trace_digest"):
                continue
            rollout_id = ""
            lane = str(entry.get("lane") or "craftax")
            projection = entry.get("visual_projection")
            if isinstance(projection, dict):
                for item in projection.get("items") or []:
                    detail = item.get("detail") if isinstance(item, dict) else None
                    if isinstance(detail, dict) and detail.get("rollout_id"):
                        rollout_id = str(detail["rollout_id"])
                        lane = str(detail.get("lane") or lane)
                        break
            trace_digest = str(entry["trace_digest"])
            events.append(
                {
                    "kind": "trace.reconciled",
                    "source": "trace_v5",
                    "occurred_at": datetime.fromtimestamp(index_path.stat().st_mtime, UTC).isoformat(),
                    "run_id": rollout_id or str(entry.get("trace_id") or "craftax"),
                    "lane": lane,
                    "sequence": f"trace:sealed:{trace_digest}",
                    "payload": {
                        "capture_id": entry.get("capture_id"),
                        "trace_id": entry.get("trace_id"),
                        "trace_digest": trace_digest,
                        "bundle_root": str(index_path.parent),
                        "high_water_ordinal": (projection or {}).get("high_water_ordinal") if isinstance(projection, dict) else None,
                        "live_reconciliation": entry.get("live_reconciliation"),
                    },
                }
            )
    return events


def handler(directory: Path, events_path: Path, container_base: str, bundle_root: Path | None, frame_cache: Path):
    class Handler(SimpleHTTPRequestHandler):
        def __init__(self, *args, **kwargs):
            super().__init__(*args, directory=str(directory), **kwargs)

        def end_headers(self) -> None:
            self.send_header("Access-Control-Allow-Origin", "*")
            super().end_headers()

        def do_GET(self) -> None:
            parsed = urlparse(self.path)
            if parsed.path == "/api/events":
                self._events()
                return
            if parsed.path == "/api/health":
                body = json.dumps(
                    {
                        "ok": True,
                        "events_path": str(events_path),
                        "event_file_count": len(event_files(events_path)),
                        "container_base": container_base,
                        "bundle_root": None if bundle_root is None else str(bundle_root),
                    }
                ).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)
                return
            frame_match = re.fullmatch(r"/api/frames/([0-9a-f]{24})/(\d+)\.png", parsed.path)
            if frame_match:
                frame_path = frame_cache / frame_match.group(1) / f"{frame_match.group(2)}.png"
                if frame_path.is_file():
                    body = frame_path.read_bytes()
                    self.send_response(200)
                    self.send_header("Content-Type", "image/png")
                    self.send_header("Content-Length", str(len(body)))
                    self.end_headers()
                    self.wfile.write(body)
                else:
                    self.send_error(404)
                return
            super().do_GET()

        def _events(self) -> None:
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Cache-Control", "no-cache")
            self.send_header("Connection", "keep-alive")
            self.end_headers()
            positions: dict[Path, int] = {}
            rollout_ids: dict[Path, str] = {}
            heartbeat_at = 0.0
            emitted_traces: set[str] = set()
            while True:
                try:
                    for event_path in event_files(events_path):
                        position = positions.get(event_path, 0)
                        size = event_path.stat().st_size
                        if size < position:
                            position = 0
                        with event_path.open("r", encoding="utf-8") as handle:
                            handle.seek(position)
                            while True:
                                line = handle.readline()
                                if not line:
                                    positions[event_path] = handle.tell()
                                    break
                                positions[event_path] = handle.tell()
                                try:
                                    row = json.loads(line)
                                except (json.JSONDecodeError, TypeError):
                                    continue
                                if row.get("record") != "envelope" or not isinstance(row.get("envelope"), dict):
                                    continue
                                event = dict(row["envelope"])
                                payload = event.get("payload") if isinstance(event.get("payload"), dict) else {}
                                rollout_id = str(payload.get("rollout_id") or rollout_ids.get(event_path) or "")
                                if rollout_id:
                                    rollout_ids[event_path] = rollout_id
                                    event["rollout_id"] = rollout_id
                                    event["run_id"] = rollout_id
                                    event["lane"] = rollout_id
                                event = enrich(event, container_base, frame_cache)
                                sequence = str(event.get("sequence") or event.get("event_id") or "")
                                if sequence:
                                    self.wfile.write(f"id: {rollout_id}:{sequence}\n".encode())
                                self.wfile.write(
                                    ("data: " + json.dumps(event, separators=(",", ":")) + "\n\n").encode()
                                )
                                self.wfile.flush()
                    for event in [*sealed_trace_events(bundle_root), *compat_seal_events(events_path)]:
                        sequence = str(event["sequence"])
                        if sequence in emitted_traces:
                            continue
                        emitted_traces.add(sequence)
                        self.wfile.write(f"id: {sequence}\n".encode())
                        self.wfile.write(
                            ("data: " + json.dumps(event, separators=(",", ":")) + "\n\n").encode()
                        )
                        self.wfile.flush()
                    now = time.monotonic()
                    if now >= heartbeat_at:
                        self.wfile.write(b": live-eval-heartbeat\n\n")
                        self.wfile.flush()
                        heartbeat_at = now + 10.0
                    time.sleep(0.1)
                except (BrokenPipeError, ConnectionResetError):
                    return

        def log_message(self, format: str, *args: object) -> None:
            if "/api/events" not in str(args[0] if args else ""):
                super().log_message(format, *args)

    return Handler


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=4188)
    parser.add_argument(
        "--events",
        type=Path,
        required=True,
        help="One eval JSONL or a Containers storage root/event_logs directory",
    )
    parser.add_argument("--container-base", default="http://127.0.0.1:8099")
    parser.add_argument("--bundle-root", type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parent
    event_source = args.events.resolve()
    frame_cache = (event_source if event_source.is_dir() else event_source.parent) / "frame-cache"
    server = ThreadingHTTPServer(
        ("127.0.0.1", args.port),
        handler(root, args.events.resolve(), args.container_base, None if args.bundle_root is None else args.bundle_root.resolve(), frame_cache),
    )
    print(f"Craftax reference: http://127.0.0.1:{args.port}")
    print(f"SSE: http://127.0.0.1:{args.port}/api/events")
    print(f"Events: {args.events.resolve()}")
    server.serve_forever()


if __name__ == "__main__":
    main()
