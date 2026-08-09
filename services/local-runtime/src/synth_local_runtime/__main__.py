from __future__ import annotations

import argparse
import json
import os
import signal
import sys
import threading
import urllib.error
import urllib.request
from pathlib import Path

from .api import RuntimeHTTPServer, write_connection_file
from .config import RuntimeConfig
from .service import RuntimeService


def _request_shutdown(server: RuntimeHTTPServer) -> None:
    """Request socketserver shutdown off the serving/main thread.

    ``BaseServer.shutdown`` waits for ``serve_forever`` to return, so calling it
    directly from a POSIX signal handler on that same thread deadlocks.
    """
    threading.Thread(target=server.shutdown, daemon=True).start()


def _default_connection_file() -> Path:
    return Path.home() / ".synth-desktop" / "runtime" / "connection.json"


def _stop(connection_file: Path) -> int:
    try:
        connection = json.loads(connection_file.read_text(encoding="utf-8"))
        url = str(connection["url"]).rstrip("/")
        token = connection.get("token")
    except (OSError, KeyError, json.JSONDecodeError) as exc:
        print(f"No usable runtime connection file at {connection_file}: {exc}", file=sys.stderr)
        return 1
    request = urllib.request.Request(
        f"{url}/v1/shutdown",
        data=b"{}",
        method="POST",
        headers={
            "Content-Type": "application/json",
            **({"Authorization": f"Bearer {token}"} if token else {}),
        },
    )
    try:
        with urllib.request.urlopen(request, timeout=5) as response:
            print(response.read().decode("utf-8"))
    except urllib.error.URLError as exc:
        print(f"Could not stop runtime: {exc}", file=sys.stderr)
        return 1
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Synth Desktop local runtime")
    parser.add_argument("--host")
    parser.add_argument("--port", type=int)
    parser.add_argument("--data-dir")
    parser.add_argument("--connection-file")
    parser.add_argument("--stop", action="store_true")
    return parser


def main() -> int:
    args = build_parser().parse_args()
    connection_file = Path(args.connection_file).expanduser() if args.connection_file else None
    if args.stop:
        return _stop(connection_file or _default_connection_file())

    config = RuntimeConfig.from_env(
        host=args.host,
        port=args.port,
        data_dir=args.data_dir,
        connection_file=connection_file,
    )
    service = RuntimeService(config)
    server = RuntimeHTTPServer(
        (config.host, config.port),
        service,
        token=config.runtime_token,
    )
    host, actual_port = server.server_address[:2]
    url = f"http://{host}:{actual_port}"
    if config.connection_file:
        write_connection_file(
            config.connection_file,
            url=url,
            token=config.runtime_token,
            service=service,
        )
    print(
        json.dumps(
            {
                "event": "runtime.started",
                "url": url,
                "pid": os.getpid(),
                "protocolVersion": service.protocol_version,
                "internMode": config.intern_mode,
                "lagunaMode": "codex" if config.laguna_base_url else "unconfigured",
                "modelTransport": "responses" if config.laguna_base_url else None,
            }
        ),
        flush=True,
    )

    def request_shutdown(_signum: int, _frame: object) -> None:
        _request_shutdown(server)

    signal.signal(signal.SIGTERM, request_shutdown)
    signal.signal(signal.SIGINT, request_shutdown)
    try:
        server.serve_forever(poll_interval=0.25)
    finally:
        server.server_close()
        if config.connection_file:
            try:
                current = json.loads(config.connection_file.read_text(encoding="utf-8"))
                if current.get("pid") == os.getpid():
                    config.connection_file.unlink(missing_ok=True)
            except (OSError, json.JSONDecodeError):
                pass
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
