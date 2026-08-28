#!/usr/bin/env python3
"""Serve eval JSONL as SSE and start the eval only after POST /start."""
from __future__ import annotations

import argparse, json, os, queue, subprocess, threading, time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=18121)
    parser.add_argument("--jsonl", required=True)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command:
        parser.error("an eval command is required after --")
    path = Path(args.jsonl).resolve()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("", encoding="utf-8")
    subscribers: set[queue.Queue[str]] = set()
    history: list[str] = []
    lock = threading.Lock()
    state: dict[str, object] = {"status": "ready", "exit_code": None, "events": 0}

    def publish(value: dict) -> None:
        line = json.dumps(value, separators=(",", ":"))
        with lock:
            history.append(line)
            state["events"] = int(state["events"]) + 1
            for subscriber in tuple(subscribers):
                subscriber.put(line)

    def tail() -> None:
        with path.open("r", encoding="utf-8") as handle:
            while state["status"] in {"ready", "running"}:
                line = handle.readline()
                if not line:
                    time.sleep(0.05)
                    continue
                try:
                    publish(json.loads(line))
                except json.JSONDecodeError:
                    pass
            for line in handle:
                try:
                    publish(json.loads(line))
                except json.JSONDecodeError:
                    pass

    def run_eval() -> None:
        if state["status"] != "ready":
            return
        state["status"] = "running"
        proc = subprocess.run(command, env=os.environ.copy(), check=False)
        state["exit_code"] = proc.returncode
        state["status"] = "completed" if proc.returncode == 0 else "failed"
        time.sleep(0.2)
        publish({
            "occurred_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "run_id": path.stem, "kind": "eval.stream.terminal", "source": "local",
            "payload": {"status": state["status"], "exit_code": proc.returncode},
            "schema_version": "evals.event-stream.v1",
        })

    threading.Thread(target=tail, name="eval-jsonl-tail", daemon=True).start()

    class Handler(BaseHTTPRequestHandler):
        def ok(self, content_type: str) -> None:
            self.send_response(200)
            self.send_header("Content-Type", content_type)
            self.send_header("Access-Control-Allow-Origin", "*")
            self.send_header("Cache-Control", "no-cache")
            self.end_headers()

        def do_GET(self) -> None:  # noqa: N802
            if self.path == "/health":
                self.ok("application/json")
                self.wfile.write(json.dumps(state).encode())
                return
            if self.path != "/events":
                self.send_error(404); return
            self.ok("text/event-stream")
            inbox: queue.Queue[str] = queue.Queue()
            with lock:
                for message in history:
                    inbox.put(message)
                subscribers.add(inbox)
            try:
                self.wfile.write(b": connected\n\n"); self.wfile.flush()
                while True:
                    try:
                        self.wfile.write(f"data: {inbox.get(timeout=10)}\n\n".encode())
                    except queue.Empty:
                        self.wfile.write(b": keepalive\n\n")
                    self.wfile.flush()
            except (BrokenPipeError, ConnectionResetError):
                pass
            finally:
                with lock: subscribers.discard(inbox)

        def do_POST(self) -> None:  # noqa: N802
            if self.path != "/start":
                self.send_error(404); return
            if state["status"] == "ready":
                threading.Thread(target=run_eval, name="eval-command", daemon=True).start()
            self.ok("application/json")
            self.wfile.write(json.dumps(state).encode())

        def log_message(self, fmt: str, *values: object) -> None:
            print(f"[live-eval] {fmt % values}", flush=True)

    print(f"SSE http://127.0.0.1:{args.port}/events", flush=True)
    print(f"Start: curl -X POST http://127.0.0.1:{args.port}/start", flush=True)
    ThreadingHTTPServer(("127.0.0.1", args.port), Handler).serve_forever()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
