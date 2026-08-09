from __future__ import annotations

import json
import os
import threading
import time
import urllib.parse
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any

from .service import RuntimeService


class RuntimeHTTPServer(ThreadingHTTPServer):
    daemon_threads = True
    allow_reuse_address = True

    def __init__(
        self,
        server_address: tuple[str, int],
        service: RuntimeService,
        *,
        token: str | None,
    ) -> None:
        super().__init__(server_address, RuntimeRequestHandler)
        self.service = service
        self.token = token


class RuntimeRequestHandler(BaseHTTPRequestHandler):
    server: RuntimeHTTPServer
    protocol_version = "HTTP/1.1"

    def log_message(self, format: str, *args: Any) -> None:
        print(f"[runtime-http] {self.address_string()} {format % args}", flush=True)

    def do_OPTIONS(self) -> None:  # noqa: N802
        self.send_response(HTTPStatus.NO_CONTENT)
        self._cors_headers()
        self.send_header("Content-Length", "0")
        self.end_headers()

    def do_GET(self) -> None:  # noqa: N802
        self._dispatch("GET")

    def do_POST(self) -> None:  # noqa: N802
        self._dispatch("POST")

    def do_DELETE(self) -> None:  # noqa: N802
        self._dispatch("DELETE")

    def _dispatch(self, method: str) -> None:
        try:
            if not self._authorized():
                self._json_error(HTTPStatus.UNAUTHORIZED, "invalid runtime token")
                return
            parsed = urllib.parse.urlparse(self.path)
            path = parsed.path.rstrip("/") or "/"
            query = urllib.parse.parse_qs(parsed.query)

            if method == "GET" and path == "/v1/health":
                self._json(HTTPStatus.OK, self.server.service.health())
                return
            if method == "GET" and path == "/v1/sessions":
                self._json(
                    HTTPStatus.OK,
                    {"sessions": self.server.service.list_sessions()},
                )
                return
            if method == "GET" and path == "/v1/projects":
                self._json(HTTPStatus.OK, {"projects": self.server.service.list_projects()})
                return
            if method == "POST" and path == "/v1/projects":
                body = self._read_json()
                self._json(HTTPStatus.CREATED, self.server.service.create_project(body))
                return
            if method == "POST" and path == "/v1/sessions":
                body = self._read_json()
                session = self.server.service.create_session(
                    body.get("target"),
                    title=body.get("title"),
                    project_id=body.get("projectId"),
                    metadata=body.get("metadata") if isinstance(body.get("metadata"), dict) else None,
                )
                self._json(HTTPStatus.CREATED, session)
                return
            if method == "POST" and path == "/v1/shutdown":
                self._json(HTTPStatus.ACCEPTED, {"stopping": True})
                threading.Thread(target=self.server.shutdown, daemon=True).start()
                return

            # Inventory + visuals
            if method == "GET" and path == "/v1/containers":
                self._json(
                    HTTPStatus.OK,
                    {"containers": self.server.service.list_containers()},
                )
                return
            if method == "POST" and path == "/v1/containers":
                body = self._read_json()
                self._json(HTTPStatus.CREATED, self.server.service.upsert_container(body))
                return
            if method == "GET" and path == "/v1/traces":
                self._json(HTTPStatus.OK, {"traces": self.server.service.list_traces()})
                return
            if method == "POST" and path == "/v1/traces":
                body = self._read_json()
                self._json(HTTPStatus.CREATED, self.server.service.ingest_trace(body))
                return
            if method == "GET" and path == "/v1/visuals/templates":
                self._json(
                    HTTPStatus.OK,
                    {"templates": self.server.service.list_visual_templates()},
                )
                return
            if method == "GET" and path == "/v1/visuals":
                self._json(HTTPStatus.OK, {"visuals": self.server.service.list_visuals()})
                return
            if method == "POST" and path == "/v1/visuals":
                body = self._read_json()
                self._json(HTTPStatus.CREATED, self.server.service.create_visual(body))
                return
            if method == "POST" and path == "/v1/visuals/simulate-live":
                body = self._read_json()
                self._json(
                    HTTPStatus.CREATED,
                    self.server.service.simulate_live_eval(kind=str(body.get("kind") or "eval")),
                )
                return
            if method == "GET" and path == "/v1/usage":
                limit = self._query_int(query, "limit", 100, minimum=1, maximum=1000)
                self._json(
                    HTTPStatus.OK,
                    {"entries": self.server.service.list_usage(limit=limit)},
                )
                return

            parts = [segment for segment in path.split("/") if segment]
            if len(parts) >= 3 and parts[:2] == ["v1", "projects"]:
                project_id = urllib.parse.unquote(parts[2])
                if len(parts) == 3 and method == "GET":
                    self._json(HTTPStatus.OK, self.server.service.get_project(project_id))
                    return
                if len(parts) == 3 and method == "DELETE":
                    self._json(
                        HTTPStatus.OK,
                        {"deleted": self.server.service.delete_project(project_id)},
                    )
                    return
            if len(parts) >= 3 and parts[:2] == ["v1", "containers"]:
                container_id = urllib.parse.unquote(parts[2])
                if len(parts) == 3 and method == "GET":
                    self._json(HTTPStatus.OK, self.server.service.get_container(container_id))
                    return
                if len(parts) == 3 and method == "DELETE":
                    self._json(
                        HTTPStatus.OK,
                        {"deleted": self.server.service.delete_container(container_id)},
                    )
                    return
                if len(parts) == 4 and parts[3] == "probe" and method == "POST":
                    self._json(HTTPStatus.OK, self.server.service.probe_container(container_id))
                    return
            if len(parts) >= 3 and parts[:2] == ["v1", "traces"]:
                trace_id = urllib.parse.unquote(parts[2])
                if len(parts) == 3 and method == "GET":
                    self._json(HTTPStatus.OK, self.server.service.get_trace(trace_id))
                    return
            if len(parts) >= 3 and parts[:2] == ["v1", "visuals"]:
                if parts[2] == "templates" and len(parts) == 4 and method == "GET":
                    template_id = urllib.parse.unquote(parts[3])
                    self._json(
                        HTTPStatus.OK,
                        self.server.service.resolve_visual_template(template_id),
                    )
                    return
                visual_id = urllib.parse.unquote(parts[2])
                if len(parts) == 3 and method == "GET":
                    self._json(HTTPStatus.OK, self.server.service.get_visual(visual_id))
                    return
                if len(parts) == 3 and method == "POST":
                    body = self._read_json()
                    self._json(
                        HTTPStatus.OK,
                        self.server.service.update_visual(visual_id, body),
                    )
                    return
                if len(parts) == 4 and parts[3] == "save-tsx" and method == "POST":
                    body = self._read_json()
                    self._json(
                        HTTPStatus.OK,
                        self.server.service.save_visual_tsx(
                            visual_id, tsx=body.get("tsx")
                        ),
                    )
                    return

            if len(parts) >= 3 and parts[:2] == ["v1", "sessions"]:
                session_id = urllib.parse.unquote(parts[2])
                if len(parts) == 3:
                    if method == "GET":
                        self._json(
                            HTTPStatus.OK,
                            self.server.service.get_session(session_id),
                        )
                        return
                    if method == "DELETE":
                        deleted = self.server.service.delete_session(session_id)
                        self._json(HTTPStatus.OK, {"deleted": deleted})
                        return
                if len(parts) == 4 and parts[3] == "runs" and method == "GET":
                    self._json(
                        HTTPStatus.OK,
                        {"runs": self.server.service.list_runs(session_id)},
                    )
                    return
                if len(parts) == 4 and parts[3] == "messages" and method == "POST":
                    body = self._read_json()
                    response = self.server.service.send_message(
                        session_id, body.get("body")
                    )
                    self._json(HTTPStatus.ACCEPTED, response)
                    return
                if len(parts) == 4 and parts[3] == "commands" and method == "POST":
                    body = self._read_json()
                    response = self.server.service.control(
                        session_id,
                        body.get("kind"),
                        body.get("payload", {}),
                    )
                    self._json(HTTPStatus.ACCEPTED, response)
                    return
                if len(parts) == 4 and parts[3] == "events" and method == "GET":
                    after_sequence = self._query_int(query, "after_sequence", 0, minimum=0)
                    limit = self._query_int(query, "limit", 500, minimum=1, maximum=500)
                    self._json(
                        HTTPStatus.OK,
                        self.server.service.events(
                            session_id,
                            after_sequence=after_sequence,
                            limit=limit,
                        ),
                    )
                    return
                if (
                    len(parts) == 5
                    and parts[3:] == ["events", "stream"]
                    and method == "GET"
                ):
                    after_sequence = self._query_int(query, "after_sequence", 0, minimum=0)
                    self._stream_events(session_id, after_sequence)
                    return

            self._json_error(HTTPStatus.NOT_FOUND, "route not found")
        except KeyError as exc:
            self._json_error(HTTPStatus.NOT_FOUND, f"resource not found: {exc.args[0]}")
        except ValueError as exc:
            self._json_error(HTTPStatus.BAD_REQUEST, str(exc))
        except RuntimeError as exc:
            self._json_error(HTTPStatus.CONFLICT, str(exc))
        except BrokenPipeError:
            return
        except ConnectionResetError:
            return
        except Exception as exc:
            self._json_error(
                HTTPStatus.INTERNAL_SERVER_ERROR,
                f"{exc.__class__.__name__}: {exc}",
            )

    def _stream_events(self, session_id: str, after_sequence: int) -> None:
        self.server.service.get_session(session_id)
        self.send_response(HTTPStatus.OK)
        self._cors_headers()
        self.send_header("Content-Type", "text/event-stream; charset=utf-8")
        self.send_header("Cache-Control", "no-cache, no-transform")
        self.send_header("Connection", "keep-alive")
        self.send_header("X-Accel-Buffering", "no")
        self.end_headers()
        cursor = after_sequence
        self.wfile.write(b": connected\n\n")
        self.wfile.flush()
        while True:
            page = self.server.service.events(
                session_id, after_sequence=cursor, limit=500
            )
            events = page["events"]
            if events:
                for event in events:
                    encoded = json.dumps(
                        event, separators=(",", ":"), ensure_ascii=False
                    )
                    frame = (
                        f"id: {event['sequence']}\n"
                        f"event: {event['eventKind']}\n"
                        f"data: {encoded}\n\n"
                    ).encode("utf-8")
                    self.wfile.write(frame)
                    cursor = int(event["sequence"])
                self.wfile.flush()
                continue

            changed = self.server.service.broker.wait_for(
                session_id,
                lambda: self.server.service.get_session(session_id)["latestCursor"] > cursor,
                timeout=15.0,
            )
            if not changed:
                self.wfile.write(b": heartbeat\n\n")
                self.wfile.flush()

    def _authorized(self) -> bool:
        expected = self.server.token
        if not expected:
            return True
        authorization = self.headers.get("Authorization", "")
        return authorization == f"Bearer {expected}"

    def _read_json(self) -> dict[str, Any]:
        raw_length = self.headers.get("Content-Length", "0")
        try:
            length = int(raw_length)
        except ValueError as exc:
            raise ValueError("invalid Content-Length") from exc
        if length < 0 or length > 1_000_000:
            raise ValueError("request body is too large")
        raw = self.rfile.read(length) if length else b"{}"
        try:
            value = json.loads(raw.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError) as exc:
            raise ValueError("request body must be valid JSON") from exc
        if not isinstance(value, dict):
            raise ValueError("request body must be a JSON object")
        return value

    @staticmethod
    def _query_int(
        query: dict[str, list[str]],
        key: str,
        default: int,
        *,
        minimum: int,
        maximum: int | None = None,
    ) -> int:
        raw = query.get(key, [str(default)])[0]
        try:
            value = int(raw)
        except ValueError as exc:
            raise ValueError(f"{key} must be an integer") from exc
        if value < minimum or (maximum is not None and value > maximum):
            raise ValueError(f"{key} is out of range")
        return value

    def _json(self, status: HTTPStatus, value: Any) -> None:
        raw = json.dumps(value, separators=(",", ":"), ensure_ascii=False).encode(
            "utf-8"
        )
        self.send_response(status)
        self._cors_headers()
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def _json_error(self, status: HTTPStatus, message: str) -> None:
        self._json(status, {"error": {"status": int(status), "message": message}})

    def _cors_headers(self) -> None:
        self.send_header("Access-Control-Allow-Origin", "http://127.0.0.1:5173")
        self.send_header("Access-Control-Allow-Headers", "Authorization, Content-Type")
        self.send_header("Access-Control-Allow-Methods", "GET, POST, DELETE, OPTIONS")


def write_connection_file(
    path: Path,
    *,
    url: str,
    token: str | None,
    service: RuntimeService,
) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    payload = {
        "url": url,
        "token": token,
        "pid": os.getpid(),
        "runtimeId": service.runtime_id,
        "protocolVersion": service.protocol_version,
        "writtenAt": time.time(),
    }
    temporary.write_text(json.dumps(payload, indent=2), encoding="utf-8")
    try:
        temporary.chmod(0o600)
    except OSError:
        pass
    temporary.replace(path)
