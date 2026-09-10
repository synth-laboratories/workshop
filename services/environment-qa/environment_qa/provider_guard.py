"""Gate-local Responses transport guard, not a second model/agent loop.

Every HTTP request (including Codex retries) reserves its full input/output bound
atomically before forwarding. Unknown charges keep the reservation. The real
provider credential never enters the app-server environment or task container.
"""
import json
import secrets
import threading
import time
import urllib.error
import urllib.request
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from .core import Conflict, digest

MODEL = "openai/gpt-5.6-luna"
MAX_BODY = 600_000
OUTPUT_TOKENS = 16384
# Above all catalog tiers, including cache-write pricing, checked 2026-09-06.
# Provider routing is constrained to these maxima on every request.
INPUT_RATE, OUTPUT_RATE = .5, 1.8


def project_request(body, granted_tools):
    if not isinstance(body, dict) or body.get("model") not in {MODEL, "gpt-5.6-luna"}:
        raise ValueError("Only the pinned Luna model is allowed")
    if body.get("previous_response_id") or body.get("conversation") or body.get("prompt"):
        raise ValueError("Hidden server-side context cannot be bounded")
    for item in body.get("input", []) if isinstance(body.get("input"), list) else []:
        for content in item.get("content", []) if isinstance(item, dict) and isinstance(item.get("content"), list) else []:
            if content.get("type") not in {"input_text", "output_text"}:
                raise ValueError("This bounded text-only runtime does not admit media")
    result = {k: body[k] for k in ("input", "instructions", "stream") if k in body}
    if (body.get("reasoning") or {}).get("effort"):
        result["reasoning"] = {"effort":body["reasoning"]["effort"]}
    if (body.get("text") or {}).get("format"):
        result["text"] = {"format":body["text"]["format"]}
    result.update(model=MODEL, max_output_tokens=OUTPUT_TOKENS, store=False,
                  provider={"max_price":{"prompt":INPUT_RATE,"completion":OUTPUT_RATE}, "require_parameters":True})
    result["tools"] = [tool for tool in body.get("tools", []) if
        tool.get("type") == "function" and tool.get("name") in granted_tools]
    result["tool_choice"] = "auto"
    wire = json.dumps(result, ensure_ascii=False).encode()
    if len(wire) > MAX_BODY:
        raise ValueError("Context exceeds bounded request size")
    # One token per serialized byte plus generous provider framing. Output limit
    # includes reasoning tokens. No media, hidden context or paid server tools.
    reserve = ((len(wire) + 8192) * INPUT_RATE + OUTPUT_TOKENS * OUTPUT_RATE) / 1_000_000
    return result, wire, reserve


def validate_event(event, tools, *, code_mode=False):
    items = [event["item"]] if isinstance(event.get("item"), dict) else []
    if isinstance(event.get("response"), dict):
        items.extend(event["response"].get("output") or [])
    for item in items:
        kind = item.get("type")
        # App-server's Code Mode host routes nested dynamic calls back through
        # item/tool/call, where the gate grant is enforced. This is not the host
        # shell tool. Only isolated dynamic-tool sessions opt into this wrapper.
        if code_mode and kind in {"custom_tool_call", "function_call"} and item.get("name") in {"exec", "wait"}:
            continue
        if kind in {"function_call", "custom_tool_call"}:
            if item.get("name") not in tools:
                raise ValueError("Provider returned a tool outside the gate grant: " + str(item.get("name"))[:120])
        elif kind not in {"message", "reasoning"}:
            raise ValueError("Provider returned an unsupported output item: " + str(kind)[:80])


def admit(store, spec, reserve, request_hash):
    request_id = uuid.uuid4().hex
    def apply(run):
        gate = next(g for g in run["gates"] if g["id"] == spec["gate_id"])
        # Pause stops new gate/turn admission in dispatch. A bounded turn already
        # admitted may finish its model round trips; cancel stops all new HTTP.
        if run["status"] in {"cancelling", "cancelled", "completed"} or gate["status"] != "running" or gate["attempt"] != spec["attempt"]:
            raise Conflict("Provider request is paused, cancelled, or stale")
        guard = run["budget"].setdefault("transport", {"reserved_usd":0, "calls":{}})
        if guard["reserved_usd"] + reserve > run["budget"]["limit_usd"]:
            raise ValueError("Transport-enforced aggregate allowance exhausted")
        guard["reserved_usd"] += reserve
        guard["calls"][request_id] = {"gate_id":spec["gate_id"], "attempt":spec["attempt"],
            "request_sha256":request_hash, "reserved_usd":reserve, "actual_usd":None,
            "output_token_limit":OUTPUT_TOKENS, "at":time.time()}
    store.mutate(spec["run_id"], "transport.admitted", apply)
    return request_id


def reconcile_completed_gates(store, run_id):
    """Release only proven unused token capacity, never assume a failed call free.

    A successful gate's cumulative usage covers its tool round trips. Failed
    attempts retain full bounds. This is an upper-bound settlement, not a claimed
    provider invoice. Original request reservations remain in the audit record.
    """
    run = store.get(run_id)
    if run.get("seal") or not run["budget"].get("transport"): return
    from .activity import gate_events
    grouped = gate_events(store, run_id)
    completed, cursor = set(), 0
    while True:
        page = store.events(run_id, cursor)
        if not page: break
        completed.update(e["payload"].get("call_id") for e in page if e["kind"] == "transport.completed")
        cursor = page[-1]["seq"]
    settlements = {}
    for gate in run["gates"]:
        if gate["status"] != "succeeded": continue
        entries = grouped.get((gate["id"],gate.get("attempt")), [])
        usage = next(((e["payload"].get("tokenUsage") or {}).get("total") for e in reversed(entries)
                      if e["kind"] == "thread/tokenUsage/updated"), None)
        if not usage or any(type(usage.get(k)) is not int or usage[k] < 0 for k in ("inputTokens","outputTokens")): continue
        calls = {id:c for id,c in run["budget"]["transport"]["calls"].items()
                 if c["gate_id"] == gate["id"] and c["attempt"] == gate.get("attempt") and id in completed}
        if not calls: continue
        upper = (usage["inputTokens"] * INPUT_RATE + usage["outputTokens"] * OUTPUT_RATE)/1_000_000
        if upper > sum(c["reserved_usd"] for c in calls.values()): continue
        key = gate["id"] + ":" + gate["attempt"]
        settlements[key] = {"calls":sorted(calls),"charged_upper_bound_usd":upper,"usage":usage}
    if not settlements: return
    def apply(current):
        guard = current["budget"]["transport"]
        guard.setdefault("settlements",{}).update(settlements)
        covered = {id for group in guard["settlements"].values() for id in group["calls"]}
        guard["reserved_usd"] = (sum(c["reserved_usd"] for id,c in guard["calls"].items() if id not in covered)
                                 + sum(group["charged_upper_bound_usd"] for group in guard["settlements"].values()))
    store.mutate(run_id,"transport.bound.reconciled",apply)


class ProviderGuard:
    def __init__(self, store, spec, credential, upstream="https://openrouter.ai/api/v1/responses"):
        if upstream != "https://openrouter.ai/api/v1/responses":
            raise ValueError("Only the authorized OpenRouter endpoint is allowed")
        self.store, self.spec, self.credential, self.upstream = store, spec, credential, upstream
        self.token = secrets.token_urlsafe(32)
        guard = self
        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_): pass
            def do_POST(self):
                if self.path != "/responses" or self.headers.get("Authorization") != "Bearer " + guard.token:
                    self.send_error(403); return
                try:
                    size = int(self.headers.get("Content-Length", "0"))
                    if size <= 0 or size > MAX_BODY:
                        raise ValueError("Invalid request size")
                    raw = self.rfile.read(size)
                    body = json.loads(raw)
                    guard.store.append_event(guard.spec["run_id"], "transport.tool_catalog", {
                        "gate_id":guard.spec["gate_id"], "tools":[{"type":t.get("type"),"name":t.get("name"),
                        "nested":[{"type":n.get("type"),"name":n.get("name")} for n in t.get("tools",[])]}
                        for t in body.get("tools",[])]})
                    projected, wire, reserve = project_request(body, guard.spec["tools"])
                    call_id = admit(guard.store, guard.spec, reserve, digest(projected))
                    request = urllib.request.Request(guard.upstream, data=wire, headers={
                        "Authorization":"Bearer " + guard.credential, "Content-Type":"application/json"})
                    with urllib.request.urlopen(request, timeout=120) as response:
                        self.send_response(response.status)
                        self.send_header("Content-Type", response.headers.get("Content-Type", "text/event-stream"))
                        self.send_header("Connection", "close")
                        self.end_headers()
                        self.close_connection = True
                        # Streaming line-by-line avoids buffering the entire answer.
                        for line in response:
                            if line.startswith(b"data: ") and line.strip() != b"data: [DONE]":
                                validate_event(json.loads(line[6:]), guard.spec["tools"],
                                               code_mode=bool(guard.spec.get("dynamic_tools")))
                            self.wfile.write(line)
                            self.wfile.flush()
                    guard.store.append_event(guard.spec["run_id"], "transport.completed", {"call_id":call_id,"gate_id":guard.spec["gate_id"]})
                except (ValueError, Conflict, urllib.error.URLError) as error:
                    # Never include upstream bodies, URLs or authorization headers.
                    detail = str(error)[:1000] if isinstance(error, (ValueError, Conflict)) else type(error).__name__
                    if isinstance(error, urllib.error.HTTPError):
                        raw_error = error.read(4096).decode(errors="replace").replace(guard.credential, "[REDACTED]")
                        try:
                            detail = str(json.loads(raw_error).get("error", {}))[:1000]
                        except ValueError:
                            detail = "Non-JSON provider error"
                    guard.store.append_event(guard.spec["run_id"], "transport.failed", {
                        "gate_id":guard.spec["gate_id"], "http_status":getattr(error,"code",None), "detail":detail})
                    self.send_error(429 if isinstance(error, (ValueError, Conflict)) else 502, type(error).__name__)
                except (BrokenPipeError, ConnectionResetError):
                    pass  # Uncertain upstream charge remains reserved.
        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.server.daemon_threads = True
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)

    def start(self):
        self.thread.start()
        return f"http://127.0.0.1:{self.server.server_port}"

    def close(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=2)
