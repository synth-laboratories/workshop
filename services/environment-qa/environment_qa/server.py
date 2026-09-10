"""Loopback API and identical HTML client for browser and Workshop embed."""
import argparse
import fcntl
import json
import secrets
import threading
import time
import sys
from concurrent.futures import ThreadPoolExecutor
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlparse, parse_qs

from .bundles import export_bundle, verified_path
from .core import CHARTERS, Conflict, Store, digest
from .worker import recover, step


def serve(root, task_roots, port=7338, provider_budget=None, allowance_id=None, operator_token=None,
          on_start=None):
    from . import __version__
    import hashlib
    engine_digest=digest({p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(Path(__file__).parent.glob('*.py'))})
    store = Store(Path(root))
    if provider_budget is not None:
        if not allowance_id: raise ValueError("An explicit authorization ID is required")
        store.authorize_service(allowance_id, provider_budget)
    lock = (store.root / "worker.lock").open("a")
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        raise SystemExit("This QA store already has an active worker")
    recover(store)
    token = secrets.token_urlsafe(32)
    web = Path(__file__).parent.parent / "web"
    if not web.is_dir(): web = Path(sys.prefix)/"share"/"workshop-environment-qa"/"web"
    # The runtime is advertised only when one is actually configured. A client
    # cannot be trusted to hide a disabled option, so the reason travels with the
    # config and admission re-checks it on every submission.
    from .dispatch import NoAppServer, configured_launcher
    try:
        configured_launcher()
        ai_runtime, runtime_disabled_reason = "codex-app-server", None
    except NoAppServer as unavailable:
        ai_runtime, runtime_disabled_reason = None, str(unavailable)
    origins = {f"http://127.0.0.1:{port}", f"http://localhost:{port}"}
    hosts = {f"127.0.0.1:{port}", f"localhost:{port}"}

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def send(self, value, status=200, content_type="application/json"):
            body = json.dumps(value).encode() if content_type == "application/json" else value
            self.send_response(status)
            self.send_header("Content-Type", content_type)
            self.send_header("Cache-Control", "no-store")
            self.send_header("X-Content-Type-Options", "nosniff")
            self.send_header("Content-Security-Policy", "default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'none'")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def stream(self, run_id, after):
            """Server-sent events for one run, framed like every other synth stream.

            Deliberately bypasses `send`: an event stream has no Content-Length and
            must not be buffered. ThreadingHTTPServer gives each follower its own
            thread, so a client parked on a long gate does not block the UI.
            """
            from .follow import SSE_HEADERS, follow, format_sse
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("X-Content-Type-Options", "nosniff")
            for header, value in SSE_HEADERS.items():
                self.send_header(header, value)
            self.end_headers()
            try:
                for event in follow(store, run_id, after):
                    self.wfile.write((format_sse(event) if event else ": heartbeat\n\n").encode())
                    self.wfile.flush()
            except (BrokenPipeError, ConnectionResetError):
                pass  # the reader went away; its cursor lets it resume

        def valid_host(self):
            if self.headers.get("Host") not in hosts:
                self.send({"error": "Invalid host"}, 403)
                return False
            return True

        def authorized(self):
            if not self.valid_host():
                return False
            if self.headers.get("Origin") and self.headers["Origin"] not in origins:
                self.send({"error": "Cross-origin request denied"}, 403)
                return False
            if not secrets.compare_digest(self.headers.get("X-QA-Token", ""), token):
                self.send({"error": "Open the local review page to connect"}, 401)
                return False
            return True

        def attested_surface(self):
            """Which document the browser says made this request.

            Page script cannot set Referer; it can only suppress it. So an absent
            value means "no attestation available", never "standalone". The
            embedded client is the one Workshop loads with `embed=workshop`.
            """
            referer = self.headers.get("Referer")
            if not referer:
                return None
            try:
                parsed = urlparse(referer)
            except ValueError:
                return None
            if f"{parsed.hostname}:{parsed.port}" not in hosts:
                return None
            return "workshop-embed" if "embed=workshop" in (parsed.query or "") else "standalone-web"

        def operator_verified(self):
            """True only when the caller presented the secret the service was started with.

            The service is loopback-only and unauthenticated, so this does not
            prove a person acted. It proves the caller holds something the CUA
            harness was not given, which is the whole difference between a
            recorded human decision and a recorded string.
            """
            if operator_token is None:
                return False
            return secrets.compare_digest(self.headers.get("X-QA-Operator", ""), operator_token)

        def do_GET(self):
            path = urlparse(self.path).path
            if not self.valid_host():
                return
            if path in {"/", "/app.js", "/style.css"}:
                name = "index.html" if path == "/" else path[1:]
                data = (web / name).read_bytes()
                if path == "/":
                    data = data.replace(b"QA_SESSION_TOKEN", token.encode())
                self.send(data, content_type={"index.html": "text/html; charset=utf-8", "app.js": "text/javascript", "style.css": "text/css"}[name])
                return
            if path == "/health":
                self.send({"service": "environment-qa", "version": __version__,"startup_engine_digest":engine_digest})
                return
            if not self.authorized():
                return
            try:
                parts = path.strip("/").split("/")
                if path == "/api/config":
                    from .policy import full_policy
                    from .profiles import advertise
                    self.send({"charters": CHARTERS, "task_roots": [str(p) for p in task_roots], "provider_calls_enabled": provider_budget is not None,
                               "allowance":store.service_allowance(allowance_id) if allowance_id else None,"full_policy":full_policy(),
                               "ai_runtime": ai_runtime, "ai_runtime_disabled_reason": runtime_disabled_reason,
                               "profiles": advertise() if ai_runtime else [],
                               "operator_token_required": operator_token is not None})
                elif path == "/api/runs":
                    self.send(store.list())
                elif len(parts) == 3 and parts[:2] == ["api", "runs"]:
                    self.send(store.get(parts[2]))
                elif len(parts) == 4 and parts[:2] == ["api", "runs"] and parts[3] == "events":
                    from .follow import poll_payload
                    query = parse_qs(urlparse(self.path).query)
                    self.send(poll_payload(store, parts[2], int(query.get("after", [0])[0]),
                                           int(query.get("limit", [1000])[0])))
                elif len(parts) == 4 and parts[:2] == ["api", "runs"] and parts[3] == "stream":
                    query = parse_qs(urlparse(self.path).query)
                    self.stream(parts[2], int(query.get("after", [0])[0]))
                elif len(parts) == 4 and parts[:2] == ["api", "runs"] and parts[3] == "evaluation":
                    run = store.get(parts[2])
                    if not run["seal"]:
                        raise ValueError("Evaluation is unavailable until predictions are sealed")
                    report = store.root / "evaluations" / (run["id"] + ".json")
                    data = json.loads(report.read_text()) if report.exists() else None
                    if data and data["prediction_seal"] != run["seal"]["sha256"]:
                        raise ValueError("Evaluation receipt does not match this seal")
                    self.send(data)
                elif len(parts) == 4 and parts[:2] == ["api", "runs"] and parts[3] == "activity":
                    from .activity import activity
                    self.send(activity(store, parts[2]))
                elif len(parts) == 4 and parts[:2] == ["api", "runs"] and parts[3] == "adjudication":
                    from .adjudication import get
                    self.send(get(store, parts[2]))
                elif len(parts) == 4 and parts[:2] == ["api", "runs"] and parts[3] == "source":
                    run = store.get(parts[2])
                    query = parse_qs(urlparse(self.path).query)
                    file = query.get("path", [""])[0]
                    context_id = query.get("context",[""])[0]
                    evidence = next((e for e in run["evidence"] if e["id"] == context_id),None)
                    if evidence and evidence["result"].get("context_ref"):
                        ref = evidence["result"]["context_ref"]
                        target = (store.root/ref["path"]).resolve()
                        if not target.is_relative_to(store.root/"contexts"/run["id"]): raise ValueError("Invalid context path")
                        files = json.loads(target.read_text())
                        if digest(files) != ref["sha256"]: raise ValueError("Context integrity failure")
                        if file not in files: raise ValueError("File is absent from this review context")
                        data = files[file].encode()
                    else:
                        if file not in run["bundle"]["files"]: raise ValueError("File is not in the bundle")
                        data = (verified_path(store, run["bundle"]) / file).read_bytes()
                    self.send({"path": file, "text": data[:200_000].decode(errors="replace"), "truncated": len(data) > 200_000})
                elif len(parts) == 4 and parts[:2] == ["api", "runs"] and parts[3] == "artifact":
                    run = store.get(parts[2])
                    file = parse_qs(urlparse(self.path).query).get("path", [""])[0]
                    declared = {t["log"] for ev in run["evidence"] for t in ev["result"].get("trials", []) if "log" in t}
                    manifests = {a["path"]:a for ev in run["evidence"] for a in ev["result"].get("artifacts",[])}
                    declared |= manifests.keys()
                    target = (store.root / file).resolve()
                    if file not in declared or not target.is_relative_to(store.root):
                        raise ValueError("Artifact is not declared by this run")
                    data = target.read_bytes()
                    if file in manifests:
                        import hashlib
                        if hashlib.sha256(data).hexdigest() != manifests[file]["sha256"]: raise ValueError("Artifact integrity failure")
                    self.send({"path": file, "text": data[:200_000].decode(errors="replace"), "truncated": len(data) > 200_000})
                else:
                    self.send({"error": "Not found"}, 404)
            except KeyError:
                self.send({"error": "Run not found"}, 404)
            except (ValueError, OSError) as exc:
                self.send({"error": str(exc)}, 400)

        def do_POST(self):
            if not self.authorized():
                return
            try:
                size = int(self.headers.get("Content-Length", "0"))
                if not 0 < size <= 32_000 or self.headers.get_content_type() != "application/json":
                    raise ValueError("Expected a bounded JSON request")
                body = json.loads(self.rfile.read(size))
                if not isinstance(body, dict):
                    raise ValueError("Expected an object")
                key = body.get("request_key")
                if not isinstance(key, str) or not 1 <= len(key) <= 200:
                    raise ValueError("An idempotency key is required")
                parts = urlparse(self.path).path.strip("/").split("/")
                if parts == ["api", "runs"]:
                    # Provider runs remain CLI-only in v1: the app cannot bypass
                    # Workshop paid-compute approval with a local HTTP checkbox.
                    # Admission happens here, before anything is created. A
                    # submission that names an unknown profile used to fall through
                    # to `pipeline=None` and quietly produce a legacy rules-only
                    # run, which reads downstream as a completed QA pass over a
                    # task nothing actually reviewed.
                    from .policy import full_policy, targeted_policy
                    from .profiles import UnknownProfile, resolve
                    profile_id = body.get("profile_id")
                    requested = body.get("pipeline")
                    mode = body.get("mode", "automated")
                    pipeline = None
                    if profile_id is not None:
                        if not isinstance(profile_id, str):
                            raise ValueError("profile_id must be a string")
                        if ai_runtime is None:
                            raise ValueError(f"No AI runtime is configured ({runtime_disabled_reason}); "
                                             "profile runs cannot be admitted")
                        mode, pipeline = resolve(profile_id, body.get("mode"))
                    elif requested is not None:
                        if requested not in {"full", "targeted"}:
                            raise ValueError(
                                f"Unknown pipeline {requested!r}. Submit profile_id instead; "
                                "an unrecognised pipeline is refused rather than run as a legacy review.")
                        pipeline = targeted_policy() if requested == "targeted" else full_policy()
                    full = pipeline is not None
                    if not full and (body.get("reviewer", "rules") != "rules" or body.get("budget_usd", 0) != 0):
                        raise ValueError("Paid AI runs must be submitted through the explicitly budgeted CLI")
                    if full and provider_budget is None:
                        raise ValueError("Full QA requires an operator-authorized provider allowance; no provider calls were made")
                    bundle = export_bundle(body["task_path"], store.root, task_roots)
                    run = store.create(bundle, mode=mode, charter=body.get("charter", "terminal-bench"),
                                       probes=body.get("probes", False), request_key=key,
                                       parent_id=body.get("parent_id"), overlay=body.get("task_goals"),
                                       reviewer="ai" if full else "rules",budget_usd=body.get("budget_usd",0) if full else 0,
                                       pipeline=pipeline,allowance_id=allowance_id if full else None,
                                       surface=body.get("surface"), surface_attested=self.attested_surface())
                elif len(parts) == 4 and parts[:2] == ["api", "runs"]:
                    revision = body["revision"]
                    if not isinstance(revision, int):
                        raise ValueError("Expected integer revision")
                    if parts[3] == "adjudication":
                        from .adjudication import decide
                        run = decide(store, parts[2], body)
                    elif parts[3] in {"permission", "clarification"}:
                        from .runtime_interactions import respond
                        run = respond(store, parts[2], body, parts[3])
                    elif parts[3] == "retry-unstarted":
                        from .retry_unstarted import retry
                        run = retry(store, parts[2], body)
                    elif parts[3] == "decision":
                        run = store.decide(parts[2], body["interaction_id"], body["decision"], body["reason"],
                                           body["context_digest"], revision, key, actor=body.get("actor"),
                                           assurance="operator-token" if self.operator_verified()
                                           else "unverified-client-claim")
                    else:
                        run = store.control(parts[2], parts[3], revision, key)
                else:
                    self.send({"error": "Not found"}, 404)
                    return
                self.send(run)
            except Conflict as exc:
                self.send({"error": str(exc)}, 409)
            except (ValueError, KeyError, TypeError, OSError) as exc:
                self.send({"error": str(exc)}, 400)

    stop = threading.Event()
    def work():
        pool = ThreadPoolExecutor(max_workers=8)
        active = []
        while not stop.is_set():
            for future in active:
                if future.done():
                    try: future.result()
                    except Exception as exc: print("QA worker completion: "+type(exc).__name__,flush=True)
            active = [f for f in active if not f.done()]
            for run in store.list():
                if stop.is_set():
                    break
                try:
                    if run["policy"].get("pipeline"):
                        from .dag import claim, execute, expire
                        expire(store, run["id"])
                        if len(active) < 8:
                            claimed = claim(store, run["id"])
                            if claimed and claimed[0] is not None:
                                active.append(pool.submit(execute, store, run["id"], *claimed))
                    elif len(active) < 8:
                        active.append(pool.submit(step, store, run["id"]))
                except Exception as exc:
                    print(f"QA worker {run['id']}: {type(exc).__name__}", flush=True)
            stop.wait(0.25)
        pool.shutdown(wait=True)
    worker = threading.Thread(target=work, daemon=True)
    server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    worker.start()
    print(f"Environment QA: http://127.0.0.1:{port} — store {store.root}", flush=True)
    # Handed to the caller so a test can shut the service down instead of leaving
    # a worker polling a store directory that has already been deleted.
    if on_start is not None:
        on_start(server)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        stop.set()
        for run in store.list():
            if any(g["status"] == "running" for g in run["gates"]):
                try:
                    store.control(run["id"], "cancel", run["revision"], secrets.token_hex(16))
                except Conflict:
                    pass
        worker.join(timeout=20)
        server.server_close()
        lock.close()
