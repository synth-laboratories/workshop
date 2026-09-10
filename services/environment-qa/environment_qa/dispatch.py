"""The one way an AI gate reaches a model.

Every gate that used to call `inference.request_json` calls this instead. The
signature is deliberately identical, so the migration is an import change at each
call site rather than a rewrite of prompt logic that already works -- and so the
existing tests that patch `request_json` in each module keep testing the same
seam.

What changes underneath is the transport and the isolation. A gate no longer makes
an HTTPS request from whatever thread it happens to be on; it gets a dedicated
Codex app-server process and thread, keyed by run/gate/attempt, and its turns run
inside that session. The agent loop in `harbor_agent` becomes several turns in one
session rather than several independent requests, which is what the pool bound is
counting.

There is no fallback. If no app server is configured or the session cannot start,
a gate fails or blocks; it never quietly reopens the direct provider path, because
a fallback that "keeps things working" is precisely how an unmetered, unisolated
call reaches a provider under a profile that promised neither.
"""
from __future__ import annotations

import json
import math
import os
import shlex
import threading
import shutil
from pathlib import Path

from . import accounting
from .codex_executor import (CodexGateExecutor, GateBlocked, GateCancelled, ProtocolError, ServerPool,
                             subprocess_launcher)
from .core import digest
from .harbor_bridge import HarborBridge

# The file-lock pool spans service and Harbor subprocesses, unlike a threading
# semaphore. Tests may still inject the in-process protocol pool directly.
from .process_pool import ProcessPool
POOL = ProcessPool(int(os.environ.get("QA_MAX_APP_SERVERS", "4")))


class NoAppServer(GateBlocked):
    """No app-server transport is configured, and there is nothing to fall back to."""


def configured_launcher(store=None):
    """Resolve the app-server command from the authorized environment.

    Absence is a hard stop rather than a degraded mode. The credential rules are
    unchanged by the transport switch: a project-local environment or the Workshop
    proxy, never a keychain or an implicit login.
    """
    command = os.environ.get("QA_CODEX_APP_SERVER", "").strip()
    if not command:
        raise NoAppServer(
            "No Codex app server is configured. Set QA_CODEX_APP_SERVER to the app-server "
            "command from an authorized environment; there is no direct-provider fallback.")
    argv = shlex.split(command)
    if not argv or not shutil.which(argv[0]):
        raise NoAppServer("Configured Codex binary is not executable")
    credential = os.environ.get("OPENROUTER_API_KEY") or os.environ.get("QA_PROVIDER_KEY")
    if not credential:
        raise NoAppServer("An authorized project-local OpenRouter credential is required; ambient login is not used")
    def launch(spec):
        if store is None:
            raise NoAppServer("A run-bound store is required for guarded provider dispatch")
        from .provider_guard import ProviderGuard
        workspace = Path(spec["cwd"])
        isolated = workspace / "runtime-config"
        isolated.mkdir(mode=0o700, parents=True, exist_ok=True)
        guard = ProviderGuard(store, spec, credential)
        base_url = guard.start()
        # Generated runtime configuration: no user config, skills, MCP, cached
        # auth or Keychain. The only credential visible to Codex is this gate's
        # local proxy token, which cannot authorize another run or endpoint.
        config = '\n'.join([
            'model_provider = "qa"', 'cli_auth_credentials_store = "file"',
            'web_search = "disabled"', 'project_doc_max_bytes = 0',
            '[features]', 'shell_tool = false', 'unified_exec = false', 'apply_patch_freeform = false',
            'code_mode_host = false', 'code_mode = false', 'code_mode_only = false',
            '[model_providers.qa]', 'name = "Bounded QA OpenRouter"',
            'base_url = ' + json.dumps(base_url), 'env_key = "QA_GATE_TOKEN"',
            'wire_api = "responses"', 'requires_openai_auth = false',
            'request_max_retries = 0', 'stream_max_retries = 0',
        ])
        (isolated / "config.toml").write_text(config)
        try:
            process = subprocess_launcher(argv, env={"QA_GATE_TOKEN":guard.token},
                                          codex_home=isolated)(spec)
        except BaseException:
            guard.close()
            raise
        process.qa_provider_guard = guard
        return process
    return launch


# The policy pins a provider-namespaced id; the app server names the same model
# without that namespace. Stripping a known prefix is an identifier resolution,
# not a model substitution -- anything else stays exactly as written, so a policy
# naming a different model reaches the runtime as that different model and fails
# there rather than being quietly rewritten into something that works.
RUNTIME_MODEL_PREFIXES = ("openai/",)


def runtime_model(policy_model):
    """The same model under the app-server runtime's identifier scheme."""
    for prefix in RUNTIME_MODEL_PREFIXES:
        if policy_model.startswith(prefix):
            return policy_model[len(prefix):]
    return policy_model


def gate_model(run, gate_id):
    """Model and effort for this gate, pinned by policy exactly as before."""
    policy = run["policy"]["pipeline"]
    gate = next((g for g in run["gates"] if g["id"] == gate_id), None)
    command_step = bool(gate) and gate.get("executor") in {"targeted_trial", "agent_trial"}
    effort = policy.get("agent_reasoning_effort") if command_step else policy.get("reasoning_effort")
    model = policy["model"]
    if "claude" in model.lower() or "anthropic" in model.lower():
        raise ValueError("Claude/Anthropic models are prohibited by user policy")
    return runtime_model(model), effort


class GateSessions:
    """One executor per (run, gate, attempt), reused across that gate's turns.

    Keyed by attempt on purpose: a re-run is a different attempt and must not
    inherit the previous one's conversation, which is the whole point of session
    isolation. Sessions are closed explicitly when the gate finishes.
    """

    def __init__(self):
        self._sessions = {}
        self._lock = threading.Lock()

    @staticmethod
    def key(run_id, gate_id, attempt):
        return (run_id, gate_id, attempt)

    def acquire(self, store, run, gate_id, attempt, *, launcher=None, secrets=(), bridge=None, response_schema=None):
        key = self.key(run["id"], gate_id, attempt)
        with self._lock:
            existing = self._sessions.get(key)
            if existing is not None:
                return existing
        model, effort = gate_model(run, gate_id)
        from .harbor_bridge import tool_definitions
        tools = ["read_evidence", "submit_result"] + (["container_exec"] if bridge is not None and bridge.exec_ is not None else [])
        workspace = store.root / "gate-workspaces" / run["id"] / gate_id / str(attempt)
        workspace.mkdir(parents=True, exist_ok=True)
        spec = {"run_id": run["id"], "gate_id": gate_id, "attempt": attempt,
                "profile": run["policy"]["pipeline"].get("profile", "tbench-non-hitl"),
                "profile_version": run["policy"]["pipeline"].get("profile_version"),
                "input_sha256": digest({"bundle": run["bundle"]["sha256"], "gate": gate_id, "policy": run["policy"],
                                        "attempt":attempt, "evidence":bridge.evidence if bridge else {},
                                        "tools":tools, "schema":response_schema}),
                "model": model, "policy_model": run["policy"]["pipeline"]["model"], "effort": effort,
                "tools": tools, "cwd": str(workspace),
                "dynamic_tools": [{"type":"function", "name": t["name"], "description": t["description"], "inputSchema": t["parameters"]}
                    for t in tool_definitions(response_schema or {"type":"object"}) if t["name"] in tools],
                "limits": {"turn_seconds": 240, "shutdown_seconds": 10},
                "approval_policy": {"mode": "hitl" if run["mode"] == "hitl" else "non_hitl",
                                    "preauthorized": [], "revision": run["revision"]}}
        # Durable, not process-local: a HITL gate's whole purpose is to pause, and a
        # decision made before a restart must still be honoured -- and one that is
        # no longer current must still be refused -- after it.
        from .approvals import StoreApprovalLedger
        from .runtime_interactions import wait_for_permission, wait_for_clarification
        executor = CodexGateExecutor(spec, launcher or configured_launcher(store), pool=POOL if launcher is None else ServerPool(4),
                                     approvals=StoreApprovalLedger(store), secrets=secrets,
                                     should_cancel=lambda: store.get(run["id"])["status"] in {"cancelled", "cancelling"},
                                     on_permission=lambda executor, message: wait_for_permission(store, executor, message),
                                     on_clarification=lambda executor, message: wait_for_clarification(store, executor, message),
                                     on_event=lambda entry: store.append_event(run["id"], f"codex.{entry['kind']}",
                                         {"gate_id": gate_id, "attempt": attempt, **entry}))
        executor.start()
        with self._lock:
            # Another thread may have started this gate's session while we launched.
            if key in self._sessions:
                executor.close()
                return self._sessions[key]
            self._sessions[key] = executor
        return executor

    def release(self, run_id, gate_id, attempt):
        with self._lock:
            executor = self._sessions.pop(self.key(run_id, gate_id, attempt), None)
        if executor is not None:
            executor.close()
        return executor

    def release_run(self, run_id):
        with self._lock:
            keys = [k for k in self._sessions if k[0] == run_id]
            executors = [self._sessions.pop(k) for k in keys]
        for executor in executors:
            executor.close()
        return executors


SESSIONS = GateSessions()


def publish(store, run_id, executor, since=0):  # noqa: D401 - see below
    """Fold the gate's journal into the run's event log.

    The follower already tails that log for gate transitions, so turn-level detail
    -- tool calls, approvals, agent messages -- arrives on the same cursor rather
    than a second stream nobody is watching.
    """
    for entry in executor.journal[max(since, getattr(executor, "published_upto", 0)):]:
        store.append_event(run_id, f"codex.{entry['kind']}",
                           {"gate_id": executor.spec["gate_id"], "attempt": executor.spec["attempt"],
                            **{k: entry[k] for k in ("sequence", "at", "kind", "payload")}})
    executor.published_upto = len(executor.journal)
    return executor.published_upto


def request_json(store, run_id, gate_id, messages, max_tokens=4096, attempt_token=None,
                 response_schema=None, repair_attempt=0, launcher=None, publish_events=True, bridge=None):
    """Run one turn of `gate_id`'s session and return its validated result.

    Admission, failure handling and settlement are the same rules as the direct
    path, because they were extracted rather than reimplemented: a reservation is
    taken before dispatch under the gate's attempt fence, a failure keeps it, and
    only a reported charge releases capacity.
    """
    run = store.get(run_id)
    if launcher is None and run["budget"].get("transport"):
        from .provider_guard import reconcile_completed_gates
        reconcile_completed_gates(store, run_id)
        run = store.get(run_id)
    attempt = attempt_token or os.environ.get("QA_GATE_ATTEMPT")
    secrets = [v for k, v in os.environ.items()
               if k in {"QA_PROVIDER_KEY", "OPENROUTER_API_KEY", "OPENAI_API_KEY"} and v]
    scoped_bridge = bridge if bridge is not None else HarborBridge(evidence={"input": messages})
    # From the session's own watermark, not this turn's start. Handshake events --
    # which carry the thread id and runtime identity -- are journaled during
    # `start()`, so taking the mark here would publish every turn except the one
    # that says which session the gate is actually using.
    executor = None
    published = 0

    prompt = ('Read the supplied evidence below. Available read_evidence IDs: '
              + json.dumps(sorted(scoped_bridge.evidence))
              + '. Paths mentioned inside an evidence item are not separate tool IDs. '
              'Submit the result using submit_result. If refused for schema errors, correct the result '
              'within this turn; after successful submission, finish with a brief acknowledgement, '
              'not a second copy of the report.\n' + json.dumps(messages, ensure_ascii=False))
    unit, rates, ceiling = accounting_policy()
    if unit == "usd":
        reserve = accounting.reservation(len(prompt.encode()), max_tokens, rates)
        call_id = accounting.admit(store, run_id, gate_id, attempt, reserve, digest(messages))
    else:
        overhead = int(os.environ.get("QA_TOKEN_CONTEXT_OVERHEAD",
                                      accounting.DEFAULT_CONTEXT_OVERHEAD_TOKENS))
        reserve = accounting.estimate_tokens(prompt, max_tokens, overhead)
        call_id = accounting.admit_tokens(store, run_id, gate_id, attempt, reserve,
                                          digest(messages), limit=ceiling)
    try:
        executor = SESSIONS.acquire(store, run, gate_id, attempt, launcher=launcher, secrets=secrets,
                                    bridge=scoped_bridge, response_schema=response_schema)
        published = getattr(executor, "published_upto", 0)
        result = executor.run_turn(prompt, response_schema, scoped_bridge, final=False)
    except (ProtocolError, GateBlocked, GateCancelled, OSError, ValueError) as error:
        failure = {"type": type(error).__name__, "message": str(error)[:1000]}
        (accounting.record_failure if unit == "usd" else accounting.record_token_failure)(
            store, run_id, call_id, failure)
        if publish_events and executor is not None:
            publish(store, run_id, executor, published)
        raise
    if unit == "usd":
        usage = dict(executor.turn_usage)
        actual = ((usage.get("input_tokens", 0) * rates[0] + usage.get("output_tokens", 0) * rates[1]) / 1_000_000
                  if {"input_tokens", "output_tokens"} <= usage.keys() else None)
        accounting.settle(store, run_id, call_id, usage, actual)
    else:
        # `turn_usage` is this turn; `usage` is the thread's running total, and
        # settling a per-call ledger against a cumulative figure would count every
        # earlier turn again on each new one.
        accounting.settle_tokens(store, run_id, call_id, dict(executor.turn_usage))
    if publish_events:
        publish(store, run_id, executor, published)
    return result


def provider_rates():
    try:
        rates = (float(os.environ["QA_INPUT_USD_PER_MILLION"]), float(os.environ["QA_OUTPUT_USD_PER_MILLION"]))
        if not all(math.isfinite(rate) and rate > 0 for rate in rates):
            raise ValueError("Rates must be finite and positive")
        return rates
    except (KeyError, ValueError):
        raise ValueError("Provider token rates are not configured; refusing to spend without a cost bound")


def accounting_policy():
    """Which ceiling governs this run: dollars, or tokens.

    A metered API-key provider reports a price, so dollars are meaningful. A
    subscription-metered runtime reports only tokens, and inventing a dollar rate
    for it would produce a bound that means nothing. Exactly one must be declared;
    refusing when neither is set is the point, not an inconvenience.
    """
    has_rates = any(os.environ.get(key, "").strip() for key in ("QA_INPUT_USD_PER_MILLION", "QA_OUTPUT_USD_PER_MILLION"))
    if has_rates:
        if os.environ.get("QA_TOKEN_BUDGET", "").strip():
            raise ValueError("Declare exactly one accounting unit, not both USD and tokens")
        return "usd", provider_rates(), None
    ceiling = os.environ.get("QA_TOKEN_BUDGET", "").strip()
    if not ceiling:
        raise ValueError(
            "No accounting policy is declared. Set QA_INPUT_USD_PER_MILLION and "
            "QA_OUTPUT_USD_PER_MILLION for a metered provider, or QA_TOKEN_BUDGET for a "
            "subscription-metered runtime that reports no price. Refusing to spend without a bound.")
    try:
        tokens = int(ceiling)
    except ValueError:
        raise ValueError("QA_TOKEN_BUDGET must be a whole number of tokens")
    if tokens <= 0:
        raise ValueError("QA_TOKEN_BUDGET must be positive")
    return "tokens", None, tokens
