"""Headless Codex app-server executor: one dedicated process and session per AI gate.

Every AI gate gets its own app-server process and its own thread. Turns inside one
gate reuse that gate's process; nothing -- process, thread, or conversation history
-- is ever shared between gates, so one gate cannot read another's context or be
steered by it. Deterministic gates never reach this module and never open a session.

The transport is JSON-RPC 2.0 as newline-delimited JSON over the child's stdio,
matching the desktop client's handling in `session/codex`: `initialize` once per
connection, then `thread/start` or `thread/resume`, then `turn/start`, journaling
notifications until a terminal turn status arrives.

Two properties are load-bearing and easy to get wrong:

  - A `turn/start` acknowledgement is not completion, and neither is every
    `turn/completed`: one carrying `status: failed` or a non-null `error` is a
    failure wearing a success method name. `terminal_method` normalises that.
  - Successful completion is necessary but not sufficient. Output is validated
    against the gate's schema before anything is committed, and a validation
    failure fails the gate closed with the raw response retained for audit.

Nothing here is headless-optional: the executor owns a plain child process and
never touches a window, renderer, or desktop session, so the standalone web
service and the Workshop pane drive identical code.
"""
from __future__ import annotations

import json
import os
import queue
import shutil
import signal
import subprocess
import sys
import threading
import time
from contextlib import contextmanager

from .core import digest

from . import __version__

# Matches the desktop client's handshake exactly (session/codex/manager.rs). The
# fake fixture is not the contract: it accepted whatever this module sent, so
# every field here is taken from the real client rather than invented.
CLIENT_INFO = {"name": "workshop-environment-qa", "title": "Environment QA", "version": __version__}
CAPABILITIES = {"experimentalApi": True}

# A review gate reads; it does not write to the workspace. Command gates raise
# this through their profile rather than the default being permissive.
DEFAULT_SANDBOX = "read-only"
DEFAULT_APPROVAL_POLICY = "untrusted"

# Server->client requests that ask permission. Anything matching this is a
# decision point, never something to answer implicitly.
APPROVAL_METHODS = {"permissions/request", "execCommandApproval", "applyPatchApproval"}
APPROVAL_SUFFIXES = ("/requestApproval", "/request_approval")

# The server advertises which decision words it accepts; these are the aliases
# each canonical choice may appear as.
DECISION_ALIASES = {
    "once": ("once", "accept", "approve", "allow", "yes"),
    "always": ("always", "acceptForSession", "allowForSession"),
    "reject": ("reject", "decline", "deny", "cancel", "no"),
}

TERMINAL_TURN_METHODS = {"turn/completed", "turn/failed", "turn/interrupted"}

# A server->client request for a published tool. It carries an id and expects a
# DynamicToolCallResponse; ignoring it leaves the model waiting for a result that
# never comes, which looks exactly like a slow gate rather than a broken one.
TOOL_CALL_METHOD = "item/tool/call"

# Only a person is a human decision. `agent-cua` is a real, permitted decision
# actor, and it is precisely the one that must never be recorded as human approval:
# the certificate counts human decisions, and a gate receipt is where that count
# would silently acquire one it did not earn.
HUMAN_ACTORS = {"local-human"}

# queued -> starting -> running -> validating -> completed, plus the branches a
# gate can leave the happy path on.
LIFECYCLE = ("queued", "starting", "running", "validating", "completed")
BRANCHES = ("waiting_for_review", "waiting_for_permission", "blocked", "failed", "cancelling", "cancelled")


class ProtocolError(RuntimeError):
    """The server said something the contract does not allow."""


class MessageTimeout(ProtocolError):
    """No event yet; short polling timeouts are not terminal failures."""


class GateBlocked(RuntimeError):
    """The gate needs an authority it does not have, so it stops rather than guessing."""


class GateCancelled(RuntimeError):
    pass


def is_approval_method(method):
    return method in APPROVAL_METHODS or any(method.endswith(s) for s in APPROVAL_SUFFIXES)


def terminal_method(method, params):
    """`turn/completed` that carries a failure is a failure.

    Trusting the method name alone books a failed turn as a completed gate, which
    is the difference between a QA result and an absence of one.
    """
    if method != "turn/completed":
        return method
    turn = params.get("turn") or params
    status = (turn.get("status") or "").lower()
    if status in {"failed", "error"} or turn.get("error") is not None:
        return "turn/failed"
    return method


def decision_word(choice, available):
    """Pick the server's word for `choice`, or None if it offers no synonym.

    Real servers mix plain words with structured options -- a command approval
    offers `"accept"`, `"cancel"` and an `acceptWithExecpolicyAmendment` object --
    so only string entries are candidates.
    """
    words = [entry for entry in available if isinstance(entry, str)]
    return next((word for word in DECISION_ALIASES[choice] if word in words), None)


def redactor(*secrets):
    """Scrub credentials out of anything on its way to a journal, log, or receipt.

    Applied at the boundary rather than at each call site: a journal entry is
    written for every event the server sends, and one un-scrubbed error string
    containing a key is a leaked key.
    """
    live = [s for s in secrets if s and isinstance(s, str) and len(s) >= 8]

    def scrub(value):
        if isinstance(value, str):
            for secret in live:
                value = value.replace(secret, "[REDACTED]")
            return value
        if isinstance(value, dict):
            return {scrub(k): scrub(v) for k, v in value.items()}
        if isinstance(value, list):
            return [scrub(v) for v in value]
        return value

    return scrub


class ServerPool:
    """Global bound on simultaneous app-server processes.

    An aggregate reservation, not a per-process cap: every gate that wants a server
    takes from one pool, so N parallel gates cannot each honour a local limit and
    still exhaust the host between them.
    """

    def __init__(self, limit):
        if not isinstance(limit, int) or isinstance(limit, bool) or limit < 1:
            raise ValueError("Server pool limit must be a positive integer")
        self.limit = limit
        self._held = {}
        self._condition = threading.Condition()

    @property
    def in_use(self):
        with self._condition:
            return len(self._held)

    @contextmanager
    def lease(self, key, timeout=30.0):
        deadline = time.monotonic() + timeout
        with self._condition:
            while len(self._held) >= self.limit:
                if not self._condition.wait(max(0.0, deadline - time.monotonic())):
                    raise GateBlocked(f"No app-server capacity within {timeout}s ({self.limit} in use)")
            if key in self._held:
                raise GateBlocked(f"Gate {key} already holds a server lease")
            self._held[key] = time.time()
        try:
            yield
        finally:
            with self._condition:
                self._held.pop(key, None)
                self._condition.notify_all()


class ApprovalLedger:
    """Persisted approvals bound to the work they authorised.

    A decision is only usable for the exact run/gate/attempt/input it was made
    against. Re-running a gate mints a new attempt, so yesterday's approval cannot
    silently authorise today's different command -- and a decision already spent
    cannot be replayed to unblock a second request.
    """

    def __init__(self):
        self._decisions = {}
        self._spent = set()

    # Fields describing how a decision may be phrased, not what is being decided.
    # Binding to them would mean a decision a person actually made never matches
    # the live request. Everything else stays in: excluding too much is the
    # dangerous direction, because it lets one approval authorise a different command.
    PRESENTATION_FIELDS = ("availableDecisions", "available_decisions")

    @classmethod
    def subject(cls, request):
        params = {k: v for k, v in (request.get("params") or {}).items()
                  if k not in cls.PRESENTATION_FIELDS}
        return {"method": request["method"], "params": params}

    @classmethod
    def fingerprint(cls, spec, request):
        """Identify the request being approved, deliberately without the attempt.

        Folding the attempt into the key would make a stale decision merely absent,
        so a replayed gate would report "nobody decided this" when someone did. The
        attempt is checked against the stored entry instead, which refuses the same
        work and can say why.
        """
        return digest({"run_id": spec["run_id"], "gate_id": spec["gate_id"],
                       "input_sha256": spec["input_sha256"], **cls.subject(request)})

    def record(self, spec, request, decision, actor, revision):
        if decision not in DECISION_ALIASES:
            raise ValueError(f"Unknown approval decision {decision!r}")
        key = self.fingerprint(spec, request)
        self._decisions[key] = {"decision": decision, "actor": actor, "revision": revision,
                                "run_id": spec["run_id"], "gate_id": spec["gate_id"],
                                "attempt": spec["attempt"], "recorded_at": time.time()}
        self._spent.discard(key)  # a fresh decision is not the previous one, already spent
        return key

    def lookup(self, spec, request, revision):
        """The decision for this exact request, if one was made and is still current."""
        key = self.fingerprint(spec, request)
        entry = self._decisions.get(key)
        if entry is None:
            return None, "no_decision"
        if entry["attempt"] != spec["attempt"]:
            return None, "stale_attempt"
        if revision is not None and entry["revision"] != revision:
            return None, "stale_revision"
        if key in self._spent:
            return None, "already_spent"
        self._spent.add(key)
        return entry, "fresh"


class Connection:
    """JSON-RPC 2.0 over newline-delimited JSON on a child process's stdio.

    A reader thread drains stdout into a queue so a wedged server becomes a
    timeout rather than a hang, and so notifications that arrive while we are
    waiting on a request result are not dropped.
    """

    def __init__(self, process, scrub=None):
        self.process = process
        self.scrub = scrub or (lambda value: value)
        self.inbox = queue.Queue()
        self.read_error = None
        self._next_id = 0
        self._reader = threading.Thread(target=self._drain, daemon=True)
        self._reader.start()

    def _drain(self):
        try:
            for line in self.process.stdout:
                line = line.strip()
                if not line:
                    continue
                try:
                    self.inbox.put(json.loads(line))
                except json.JSONDecodeError:
                    self.inbox.put({"__malformed__": self.scrub(line.decode() if isinstance(line, bytes) else line)})
        except Exception as error:  # pragma: no cover - pipe teardown races
            self.read_error = error
        finally:
            self.inbox.put(None)  # EOF sentinel: the server is gone

    def send(self, message):
        if self.process.poll() is not None:
            raise ProtocolError("app_server_exited")
        self.process.stdin.write(json.dumps(message) + "\n")
        self.process.stdin.flush()

    def notify(self, method, params=None):
        self.send({"jsonrpc": "2.0", "method": method, **({"params": params} if params is not None else {})})

    def respond(self, rpc_id, result=None, error=None):
        payload = {"jsonrpc": "2.0", "id": rpc_id}
        payload["error" if error is not None else "result"] = error if error is not None else result
        self.send(payload)

    def request(self, method, params=None):
        self._next_id += 1
        self.send({"jsonrpc": "2.0", "id": self._next_id, "method": method, "params": params or {}})
        return self._next_id

    def next_message(self, timeout):
        try:
            message = self.inbox.get(timeout=timeout)
        except queue.Empty:
            raise MessageTimeout(f"No message from app server within {timeout}s")
        if message is None:
            raise ProtocolError("app_server_exited")
        if "__malformed__" in message:
            raise ProtocolError(f"Malformed server message: {message['__malformed__'][:200]}")
        return self.scrub(message)


class CodexGateExecutor:
    """Runs one AI gate against a dedicated app-server process.

    `spec` carries the gate's identity and limits:
      run_id, gate_id, attempt, profile, profile_version, input_sha256,
      model, effort, tools, schema, limits {turn_seconds, shutdown_seconds},
      approval_policy {mode: hitl|non_hitl, preauthorized: [...]}
    """

    def __init__(self, spec, launch, *, pool=None, approvals=None, secrets=(), clock=time.monotonic, on_event=None, on_permission=None, should_cancel=None, on_clarification=None):
        self.spec = spec
        self.launch = launch
        self.pool = pool
        self.approvals = approvals if approvals is not None else ApprovalLedger()
        self.scrub = redactor(*secrets)
        self.clock = clock
        self.on_event = on_event
        self.on_permission = on_permission
        self.on_clarification = on_clarification
        self.should_cancel = should_cancel
        self.review_wait_seconds = 0.0
        self.state = "queued"
        self.process = None
        self.connection = None
        self.thread_id = None
        self.turn_ids = []
        self.journal = []
        self.tool_results = []
        self.server_info = {}
        self.blocked_reason = None
        self.usage = {}
        self.turn_usage = {}
        self._turn_usage_start = {}
        self.rate_limits = None
        self.context_window = None
        self._seen = set()
        self._lease = None

    # --- journal -------------------------------------------------------

    def record(self, kind, payload, key=None):
        """Append one journal entry, ignoring a duplicate delivery of the same event.

        Servers may redeliver on reconnect. Journaling the same event twice would
        double-count tool calls and usage, so identity wins over arrival order.
        """
        if key is not None:
            if key in self._seen:
                return None
            self._seen.add(key)
        entry = {"sequence": len(self.journal) + 1, "at": time.time(), "kind": kind,
                 "payload": self.scrub(payload)}
        self.journal.append(entry)
        if self.on_event is not None:
            self.on_event(entry)
            self.published_upto = len(self.journal)
        return entry

    def transition(self, state):
        if state not in LIFECYCLE and state not in BRANCHES:
            raise ValueError(f"Unknown gate state {state!r}")
        self.record("gate.state", {"from": self.state, "to": state})
        self.state = state

    # --- lifecycle -----------------------------------------------------

    def __enter__(self):
        self.start()
        return self

    def __exit__(self, *_):
        self.close()
        return False

    def start(self):
        """Spawn, handshake, and open this gate's thread."""
        self.transition("starting")
        if self.pool is not None:
            self._lease = self.pool.lease(f"{self.spec['run_id']}:{self.spec['gate_id']}:{self.spec['attempt']}")
            self._lease.__enter__()
        try:
            self.process = self.launch(self.spec)
            self.record("process.started", {"pid": self.process.pid})
            self.connection = Connection(self.process, self.scrub)
            self.server_info = self._initialize()
            self.thread_id = self._open_thread()
        except BaseException:
            self.close()
            raise
        self.transition("running")
        return self

    def _initialize(self):
        """Handshake, then announce it. The `initialized` notification is not optional.

        The server treats the connection as unready until it arrives, so skipping
        it does not fail loudly -- it stalls on the first real request, which is a
        far worse failure to diagnose.
        """
        rpc_id = self.connection.request("initialize", {"clientInfo": CLIENT_INFO,
                                                        "capabilities": CAPABILITIES})
        result = self._await_result(rpc_id, "initialize")
        self.connection.notify("initialized")
        self.record("server.initialized", result)
        return result

    def server_approval_policy(self):
        """What the server enforces, distinct from what this executor decides.

        Server-side policy gates whether a request reaches us at all; the local
        approval ledger decides how we answer it. Conflating them would let a
        permissive server default silently skip the decision point entirely.
        """
        return self.spec.get("approval_policy", {}).get("server_policy", DEFAULT_APPROVAL_POLICY)

    def _open_thread(self):
        resume = self.spec.get("resume_thread_id")
        method = "thread/resume" if resume else "thread/start"
        # cwd, approvalPolicy and sandbox are the server-side confinement controls.
        # Omitting them does not disable them; it accepts the server's defaults,
        # which are more permissive than any gate should run under.
        params = {"model": self.spec["model"], "cwd": self.spec.get("cwd", "."),
                  "approvalPolicy": self.server_approval_policy(),
                  "sandbox": self.spec.get("sandbox", DEFAULT_SANDBOX)}
        if resume:
            params["threadId"] = resume
        if self.spec.get("dynamic_tools"):
            params["dynamicTools"] = self.spec["dynamic_tools"]
            # The host bridge owns all environment access. Do not select the
            # runtime's default host environment for this QA thread.
            params["environments"] = []
        result = self._await_result(self.connection.request(method, params), method)
        thread_id = result.get("threadId") or result.get("thread", {}).get("id")
        if not thread_id:
            raise ProtocolError(f"{method} returned no thread id")
        self.record("thread.opened", {"method": method, "threadId": thread_id})
        return thread_id

    def _await_result(self, rpc_id, method):
        """Wait for one request's result, servicing anything that arrives first."""
        deadline = self.clock() + self.spec.get("limits", {}).get("turn_seconds", 120)
        while True:
            message = self.connection.next_message(timeout=max(0.1, deadline - self.clock()))
            if message.get("id") == rpc_id and ("result" in message or "error" in message):
                if "error" in message:
                    raise ProtocolError(f"{method} failed: {json.dumps(message['error'])[:300]}")
                return message["result"]
            self._service(message)

    def _service(self, message):
        """Handle a notification or an inbound server request that is not our result."""
        method = message.get("method")
        if method is None:
            return
        if "id" in message and is_approval_method(method):
            self._answer_approval(message)
            return
        if "id" in message and method == TOOL_CALL_METHOD:
            self._answer_tool_call(message, None)
            return
        params = message.get("params", {})
        self.record(method, params, key=self._event_key(method, params))

    @staticmethod
    def _event_key(method, params):
        item = params.get("item") or {}
        identity = item.get("id") or params.get("eventId") or params.get("id")
        return f"{method}:{identity}" if identity else None

    # --- approvals -----------------------------------------------------

    def _answer_approval(self, message):
        """Approvals are decisions, never defaults.

        Non-HITL may only proceed on something preauthorized for this gate. Anything
        else is refused and the gate blocks explicitly, because inventing consent is
        exactly the failure the approval record exists to prevent.
        """
        params = message.get("params", {})
        # Journal what was asked, not only what was answered: a receipt showing a
        # refusal without the request cannot be reviewed.
        self.record("approval.requested", {"method": message["method"], "params": params})
        available = params.get("availableDecisions") or params.get("available_decisions") or list(DECISION_ALIASES)
        policy = self.spec.get("approval_policy", {})
        request = {"method": message["method"], "params": params}
        entry, why = self.approvals.lookup(self.spec, request, policy.get("revision"))
        if entry is None and why == "no_decision" and policy.get("mode") == "hitl" and self.on_permission is not None:
            entry = self.on_permission(self, message)
            why = entry.get("reason", "reviewed")
        if entry is not None:
            choice = entry["decision"]
        elif why == "no_decision" and self._preauthorized(params, policy):
            choice = "once"
            why = "preauthorized"
        else:
            choice = "reject"
        word = decision_word(choice, available)
        actor = entry["actor"] if entry is not None else ("policy" if why == "preauthorized" else "executor")
        self.record("approval.decided", {"method": message["method"], "decision": choice,
                                         "reason": why, "available": available, "sent": word,
                                         "actor": actor, "human": actor in HUMAN_ACTORS})
        if word is None:
            self.connection.respond(message["id"], error={"code": -32602,
                                                          "message": "No supported decision offered"})
        else:
            self.connection.respond(message["id"], result={"decision": word})
        if choice == "reject":
            self.blocked_reason = f"approval_{why}"
            self.transition("waiting_for_permission" if policy.get("mode") == "hitl" and self.on_permission is None else "blocked")

    def _answer_tool_call(self, message, bridge):
        """Run a published tool and return its result to the server.

        A refusal is answered too, with `success: false`. Staying silent would
        strand the turn; answering tells the model the tool is out of scope, which
        is information it can act on.
        """
        from .harbor_bridge import ToolRefused
        params = message.get("params", {})
        name = params.get("tool")
        try:
            if bridge is None or name not in self.spec.get("tools", []):
                raise ToolRefused("This gate publishes no tools")
            outcome = bridge.call(name, params.get("arguments"))
            if name == 'submit_result' and getattr(self, '_response_schema', None) is not None:
                from .inference import validate_shape
                validate_shape(outcome['arguments'], self._response_schema)
            body, success = json.dumps(outcome), True
        except ToolRefused as refusal:
            body, success, outcome = str(refusal), False, None
        except Exception as error:  # a tool fault is the gate's problem, not the model's to guess at
            body, success, outcome = f"{type(error).__name__}: {error}", False, None
        self.record("tool.result", {"tool": name, "call_id": params.get("callId"), "ok": success,
                                    **({'error': body} if not success else {})})
        self.connection.respond(message["id"],
                                result={"success": success,
                                        "contentItems": [{"type": "inputText", "text": body}]})
        return None if outcome is None else {"tool": name, "call_id": params.get("callId"), **outcome}

    def _preauthorized(self, params, policy):
        """Is this exact request on the gate's preauthorized list?

        A command approval names a command, not a tool, so matching only tool names
        silently made every command unapprovable in non-HITL. Commands are compared
        as their exact argv joined by spaces -- never a prefix or a substring, which
        would let `git status` authorise `git push`.
        """
        allowed = set(policy.get("preauthorized", []))
        if not allowed:
            return False
        item = params.get("item") or {}
        for candidate in (params.get("tool"), params.get("name"), item.get("name")):
            if candidate and candidate in allowed:
                return True
        command = params.get("command") or item.get("command")
        if isinstance(command, list) and all(isinstance(a, str) for a in command):
            return " ".join(command) in allowed
        return isinstance(command, str) and command in allowed

    # --- turns ---------------------------------------------------------

    def run_turn(self, prompt, schema=None, bridge=None, final=True):
        """Run one turn to a terminal status, then validate before returning.

        Returns the validated submission. Raises rather than returning a partial
        result: a gate with no defensible output must not look like a gate that
        found nothing.

        `final=False` leaves the gate running so the next turn reuses this same
        process and thread. That is what an agent loop is -- several turns inside
        one gate -- as opposed to several gates, which never share a session.
        """
        self.turn_usage = {}
        self._response_schema = schema
        self._turn_usage_start = dict(self.usage)
        self.record("turn.input", {"sha256":digest({"prompt":prompt,"schema":schema,"tools":self.spec.get("tools",[])}),
                                  "input_bytes":len(prompt.encode())})
        if self.state not in {"running", "waiting_for_review"}:
            raise GateBlocked(f"Cannot start a turn from state {self.state!r}")
        # `input` is a list of typed content items, never a bare string.
        params = {"threadId": self.thread_id, "model": self.spec["model"],
                  "input": [{"type": "text", "text": prompt}],
                  "approvalPolicy": self.server_approval_policy()}
        if self.spec.get("effort"):
            params["effort"] = self.spec["effort"]
        # The protocol constrains the final assistant message itself. Asking for
        # structure here is stronger than parsing for it afterwards.
        # The typed submission tool is authoritative when published. Requiring
        # another full structured final message duplicates large reports and can
        # exhaust the output stream after the actual result was already sent.
        if schema is not None and not self.spec.get('dynamic_tools'):
            params["outputSchema"] = schema
        rpc_id = self.connection.request("turn/start", params)
        acknowledged = self._await_result(rpc_id, "turn/start")
        turn_id = acknowledged.get("turnId") or acknowledged.get("turn", {}).get("id")
        self.turn_ids.append(turn_id)
        self.record("turn.acknowledged", {"turnId": turn_id})
        submission = self._pump(turn_id, bridge)
        self.transition("validating")
        validated = self._validate(submission, schema)
        self.transition("completed" if final else "running")
        return validated

    def _pump(self, turn_id, bridge):
        """Journal events until the turn reaches a terminal status."""
        deadline = self.clock() + self.spec.get("limits", {}).get("turn_seconds", 120)
        wait_at_start = self.review_wait_seconds
        submission = None
        while True:
            if self.should_cancel is not None and self.should_cancel():
                self.cancel()
                raise GateCancelled("Run cancelled by operator")
            effective_deadline = deadline + self.review_wait_seconds - wait_at_start
            if self.clock() > effective_deadline:
                self.transition("failed")
                raise ProtocolError(f"Turn {turn_id} exceeded its deadline")
            try:
                message = self.connection.next_message(timeout=min(0.5, max(0.1, effective_deadline - self.clock())))
            except MessageTimeout:
                continue
            except ProtocolError as error:
                self.transition("failed")
                self.record("turn.failed", {"turnId": turn_id, "reason": str(error)})
                raise
            method = message.get("method")
            if method == "item/tool/requestUserInput" and "id" in message:
                if self.spec.get("approval_policy", {}).get("mode") != "hitl" or self.on_clarification is None:
                    self.connection.respond(message["id"], error={"code": -32000, "message": "Clarification requires HITL authority"})
                    self.transition("blocked")
                    raise GateBlocked("Clarification needed; no answer invented")
                answer = self.on_clarification(self, message)
                self.record("clarification.decided", answer)
                if answer["decision"] != "answer":
                    self.connection.respond(message["id"], error={"code": -32000, "message": "Clarification declined"})
                    self.transition("blocked")
                    raise GateBlocked("Clarification declined or expired")
                self.connection.respond(message["id"], {"answers": answer["answers"]})
                continue
            if method and "id" in message and is_approval_method(method):
                self._answer_approval(message)
                if self.state in {"blocked", "waiting_for_permission"}:
                    raise GateBlocked(self.blocked_reason or "approval_refused")
                continue
            if method == TOOL_CALL_METHOD and "id" in message:
                result = self._answer_tool_call(message, bridge)
                if result is not None:
                    self.tool_results.append(result)
                    if result.get("tool") == "submit_result":
                        submission = result.get("arguments")
                continue
            if "id" in message and method:
                # Any other server->client request. Silence would strand the turn
                # waiting on a reply, so decline explicitly and journal it: an
                # unhandled request is a gap in this client, not a slow model.
                self.record("request.declined", {"method": method})
                self.connection.respond(message["id"],
                                        error={"code": -32601,
                                               "message": f"{method} is not handled by this gate"})
                continue
            if method is None:
                continue
            params = message.get("params", {})
            resolved = terminal_method(method, params)
            if self.record(resolved, params, key=self._event_key(method, params)) is None:
                continue  # a redelivered event must not run its tool call twice
            self._observe(method, params)
            if bridge is not None and method in {"item/started", "item/completed"}:
                result = bridge.handle(self, params)
                if result is not None:
                    self.tool_results.append(result)
                    if result.get("tool") == "submit_result":
                        submission = result.get("arguments")
            if resolved in TERMINAL_TURN_METHODS:
                self._accumulate_usage(params)
                if resolved == "turn/failed":
                    self.transition("failed")
                    raise ProtocolError(f"Turn {turn_id} failed: {json.dumps(params)[:300]}")
                if resolved == "turn/interrupted":
                    self.transition("cancelled")
                    raise GateCancelled(f"Turn {turn_id} was interrupted")
                return submission if submission is not None else self._final_message(params)

    USAGE_FIELDS = {"inputTokens": "input_tokens", "outputTokens": "output_tokens",
                    "totalTokens": "total_tokens", "cachedInputTokens": "cached_input_tokens",
                    "cacheWriteInputTokens": "cache_write_input_tokens",
                    "reasoningOutputTokens": "reasoning_output_tokens"}

    @classmethod
    def _normalise_usage(cls, reported):
        return {name: reported[key] for key, name in cls.USAGE_FIELDS.items()
                if isinstance(reported.get(key), int) and not isinstance(reported.get(key), bool)}

    def _observe(self, method, params):
        """Record what the run actually consumed, from where the server reports it.

        Usage arrives on `thread/tokenUsage/updated`, not on the terminal turn
        event. Its `total` is cumulative for the thread and `last` is one model request, so
        the total is *assigned* rather than summed: adding it once per turn would
        multiply the bill of any gate that runs more than one.
        """
        if method == "thread/tokenUsage/updated":
            reported = params.get("tokenUsage") or {}
            total = self._normalise_usage(reported.get("total") or {})
            if total:
                self.usage = total
                self.turn_usage = {key:value-self._turn_usage_start.get(key,0) for key,value in total.items()
                                   if value >= self._turn_usage_start.get(key,0)}
            else:
                # `last` alone cannot account for multiple tool round trips.
                self.turn_usage = {}
            self.context_window = reported.get("modelContextWindow")
        elif method == "account/rateLimits/updated":
            # The quota signal. On a subscription this is the only bound the
            # runtime can state, and it is not a dollar figure.
            self.rate_limits = params

    def _accumulate_usage(self, params):
        """Fallback for a server that reports usage on the turn itself."""
        reported = (params.get("turn") or {}).get("usage") or params.get("usage") or {}
        normalised = self._normalise_usage(reported) or {
            k: v for k, v in reported.items() if isinstance(v, int) and not isinstance(v, bool)}
        for key, value in normalised.items():
            if key not in self.turn_usage:
                self.turn_usage[key] = value
                self.usage[key] = self.usage.get(key, 0) + value

    def _final_message(self, params):
        """Fall back to the last agent message when no submit_result tool was used."""
        for entry in reversed(self.journal):
            if entry["kind"] in {"item/completed", "item/agentMessage"}:
                item = entry["payload"].get("item") or {}
                text = item.get("text") or item.get("message")
                if text:
                    return text
        return (params.get("turn") or {}).get("output")

    def _validate(self, submission, schema):
        if schema is None:
            return submission
        from .inference import validate_shape
        parsed = submission
        if isinstance(parsed, str):
            try:
                parsed = json.loads(parsed)
            except json.JSONDecodeError:
                self.transition("failed")
                raise ProtocolError("Gate output was not JSON; raw response retained in the journal")
        if not isinstance(parsed, dict):
            self.transition("failed")
            raise ProtocolError("Gate output must be a JSON object")
        try:
            validate_shape(parsed, schema)
        except ValueError as error:
            self.transition("failed")
            raise ProtocolError(f"Gate output failed schema validation: {error}") from None
        return parsed

    # --- cancellation and teardown -------------------------------------

    def cancel(self):
        """Interrupt the turn, then stop the process within a bounded deadline."""
        if self.state in {"cancelled", "completed", "failed"}:
            return
        self.transition("cancelling")
        try:
            # `turn/interrupt` is a request in ClientRequest, and it needs the turn
            # as well as the thread. Sent as a notification it is silently not a
            # cancellation: the turn keeps running while the UI says cancelled.
            if self.connection is not None and self.thread_id and self.turn_ids:
                self.connection.request("turn/interrupt",
                                        {"threadId": self.thread_id, "turnId": self.turn_ids[-1]})
        except (ProtocolError, OSError, ValueError):
            pass
        self.close()
        if self.state != "cancelled":
            self.transition("cancelled")

    def close(self):
        """Terminate only the process this executor owns."""
        deadline = self.spec.get("limits", {}).get("shutdown_seconds", 5)
        process, self.process = self.process, None
        if process is not None:
            try:
                if process.poll() is None:
                    self._signal_group(process, signal.SIGTERM)
                    try:
                        process.wait(timeout=deadline)
                    except subprocess.TimeoutExpired:
                        self._signal_group(process, signal.SIGKILL)
                        process.wait(timeout=deadline)
                self.record("process.stopped", {"pid": getattr(process, "pid", None),
                                                "returncode": process.poll()})
            except OSError:
                pass
            for stream in (getattr(process, "stdin", None), getattr(process, "stdout", None)):
                try:
                    stream and stream.close()
                except OSError:
                    pass
            if getattr(process, "qa_provider_guard", None) is not None:
                process.qa_provider_guard.close()
        if self._lease is not None:
            lease, self._lease = self._lease, None
            lease.__exit__(None, None, None)

    @staticmethod
    def _signal_group(process, sig):
        """Stop the whole owned tree, falling back to the parent alone.

        A shell tool spawns grandchildren, so signalling only the JSON-RPC parent
        leaves a command -- or its container client -- running after the gate has
        been told the turn is over. The launcher puts each server in its own
        process group precisely so this can target that tree and nothing else.
        """
        try:
            group = os.getpgid(process.pid)
        except (ProcessLookupError, OSError):
            return
        # Only signal the group when the child actually leads one. A child that
        # inherited our group would mean signalling ourselves -- this process and
        # every sibling -- which is a far worse outcome than a leaked grandchild.
        if group == process.pid:
            try:
                os.killpg(group, sig)
                return
            except (ProcessLookupError, PermissionError, OSError):
                pass
        try:
            process.send_signal(sig)
        except (ProcessLookupError, OSError):
            pass

    # --- receipt -------------------------------------------------------

    def receipt(self):
        """Everything needed to audit this gate without trusting its output."""
        return {"schema": "environment-qa.gate-receipt.v1",
                "run_id": self.spec["run_id"], "gate_id": self.spec["gate_id"],
                "attempt": self.spec["attempt"], "profile": self.spec.get("profile"),
                "profile_version": self.spec.get("profile_version"),
                "input_sha256": self.spec["input_sha256"],
                "state": self.state, "blocked_reason": self.blocked_reason,
                "human_approvals": sum(1 for e in self.journal
                                       if e["kind"] == "approval.decided" and e["payload"]["human"]),
                "process": {"pid": getattr(self.process, "pid", None), "server": self.server_info},
                "thread_id": self.thread_id, "turn_ids": list(self.turn_ids),
                "model": self.spec["model"], "effort": self.spec.get("effort"),
                "tools": sorted(self.spec.get("tools", [])),
                "usage": dict(self.usage), "turn_usage": dict(self.turn_usage),
                "context_window": self.context_window, "rate_limits": self.rate_limits,
                "events": list(self.journal), "tool_results": list(self.tool_results)}


def subprocess_launcher(command, env=None, cwd=None, codex_home=None):
    """Launch the real app server, matching the desktop client's spawn exactly.

    Two details are not optional. A configured `codex` is usually a
    `#!/usr/bin/env node` shim, so the launcher's own directory has to be on PATH
    or the sibling `node` is not found at runtime. And each server gets its own
    process group, so stopping a gate can terminate the tree it owns rather than
    orphaning whatever a shell tool spawned.
    """
    argv = [*command]
    if not any(a == "app-server" for a in argv):
        argv += ["app-server", "--listen", "stdio://"]

    def launch(spec):
        child = dict(env or {})
        path = os.environ.get("PATH", "")
        binary = shutil.which(argv[0]) or argv[0]
        directory = os.path.dirname(os.path.realpath(binary))
        child["PATH"] = f"{directory}{os.pathsep}{path}" if directory else path
        if codex_home:
            child["CODEX_HOME"] = str(codex_home)
        return subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                stderr=subprocess.DEVNULL, env=child,
                                cwd=cwd or spec.get("cwd") or None,
                                text=True, bufsize=1, start_new_session=True)
    return launch


def fake_launcher(scenario, script=None):
    """Launch the protocol fake server with a scenario, for tests."""
    path = script or (os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
                      + "/tests/fixtures/fake_app_server.py")

    def launch(spec):
        # Own session, like the real launcher, so teardown exercises the same path.
        return subprocess.Popen([sys.executable, path, json.dumps(scenario)],
                                stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                stderr=subprocess.DEVNULL, text=True, bufsize=1,
                                start_new_session=True)
    return launch
