"""Provider admission and settlement, independent of the transport that spends it.

Extracted from `inference.py` so a second transport cannot drift from the first.
Three rules are enforced here rather than left to each caller:

  - Admission is atomic and fenced. A reservation is taken inside the same
    transaction that checks the gate is still running under the expected attempt,
    so a stale worker cannot spend against a gate that moved on.
  - A failed call keeps its reservation. Charges for a request that errored are
    unknown, not zero, and releasing them would let a retry storm spend money the
    ledger says is already committed.
  - Only a provider-reported settled charge releases capacity. A locally computed
    estimate never does, because the estimate is what we were unsure about.
"""
import math
import uuid

from .core import Conflict


def reservation(prompt_bytes, completion_limit, rates):
    """Conservative pre-spend estimate: full prompt plus the whole completion allowance."""
    return ((prompt_bytes + 8192) * rates[0] + completion_limit * rates[1]) / 1_000_000


def admit(store, run_id, gate_id, attempt_token, reserve, request_sha256):
    """Reserve `reserve` against the run budget, or raise. Returns the call id."""
    call_id = uuid.uuid4().hex

    def apply(current):
        if current["status"] in {"paused", "cancelling", "cancelled"}:
            raise Conflict("Provider dispatch stopped")
        gate = next(g for g in current["gates"] if g["id"] == gate_id)
        if gate["status"] != "running" or not attempt_token or gate["attempt"] != attempt_token:
            raise Conflict("Stale provider dispatch attempt")
        budget = current["budget"]
        committed = sum(c.get("charged_usd", c["reserved_usd"]) for c in budget["calls"].values())
        if committed + reserve > budget["limit_usd"]:
            raise ValueError("Aggregate QA budget exhausted")
        budget["reserved_usd"] += reserve
        budget["calls"][call_id] = {"gate_id": gate_id, "request_sha256": request_sha256,
                                    "reserved_usd": reserve, "actual_usd": None}
        budget["actual_usd"] = None

    store.mutate(run_id, "provider.admitted", apply)
    return call_id


def record_failure(store, run_id, call_id, failure):
    """Attach a failure to the call and keep its reservation."""
    store.mutate(run_id, "provider.failed",
                 lambda current: current["budget"]["calls"][call_id].update(provider_error=failure))


def settle(store, run_id, call_id, usage, actual):
    """Close out a call. `actual` is the local estimate; a reported cost overrides it."""
    def apply(current):
        calls = current["budget"]["calls"]
        calls[call_id].update(actual_usd=actual, usage=usage)
        reported = usage.get("cost")
        if type(reported) in (int, float) and math.isfinite(reported) and reported >= 0:
            calls[call_id].update(charged_usd=reported, actual_usd=reported)
        current["budget"]["committed_usd"] = sum(c.get("charged_usd", c["reserved_usd"]) for c in calls.values())
        current["budget"]["actual_usd"] = (sum(c["actual_usd"] for c in calls.values())
                                           if all(c["actual_usd"] is not None for c in calls.values()) else None)

    store.mutate(run_id, "provider.settled", apply)


# --- token-denominated accounting -------------------------------------
#
# A subscription-metered runtime reports no dollar figure, so a USD ceiling there
# is a fabricated number rather than a bound. Tokens are what the runtime actually
# reports, so they are what the ceiling is denominated in. The discipline is
# unchanged: reserve before dispatch, keep the reservation on failure, and settle
# only against what the server said was consumed.

CHARS_PER_TOKEN = 4

# The prompt is not what the model is billed for. The runtime prepends its own
# system instructions, tool specifications and environment context, and on a
# measured live turn that came to ~14.5k tokens against a 275-token prompt -- a
# reservation 53x under the real consumption, which is a ceiling that does not
# bind. This is the floor a turn is assumed to cost before its own text is added.
DEFAULT_CONTEXT_OVERHEAD_TOKENS = 16_000


def estimate_tokens(prompt_text, max_output_tokens, overhead=DEFAULT_CONTEXT_OVERHEAD_TOKENS):
    """Conservative pre-spend estimate: runtime context, the prompt, and the whole allowance."""
    return int(overhead) + math.ceil(len(prompt_text) / CHARS_PER_TOKEN) + int(max_output_tokens)


def committed_tokens(calls):
    """What a call counts against the ceiling: its settled cost, else its reservation.

    `.get("actual", reserved)` is wrong here and quietly so: admission writes
    `actual: None`, so the key exists and the default never applies, yielding None
    instead of the reservation. An unsettled or failed call must still occupy its
    reservation -- that is what keeps the ceiling honest while work is in flight.
    """
    return sum(call["actual"] if call.get("actual") is not None else call["reserved"]
               for call in calls)


def token_ledger(budget):
    return budget.setdefault("tokens", {"limit": None, "reserved": 0, "actual": 0, "calls": {}})


def admit_tokens(store, run_id, gate_id, attempt_token, reserve, request_sha256, limit=None):
    """Reserve `reserve` tokens against the run's token ceiling, or raise.

    The ceiling is set once, on first use, and afterwards can only be lowered.
    A later call that could raise it would mean the bound moved to fit the spend.
    """
    call_id = uuid.uuid4().hex

    def apply(current):
        if current["status"] in {"paused", "cancelling", "cancelled"}:
            raise Conflict("Provider dispatch stopped")
        gate = next(g for g in current["gates"] if g["id"] == gate_id)
        if gate["status"] != "running" or not attempt_token or gate["attempt"] != attempt_token:
            raise Conflict("Stale provider dispatch attempt")
        ledger = token_ledger(current["budget"])
        if limit is not None:
            if ledger["limit"] is None:
                ledger["limit"] = int(limit)
            elif int(limit) > ledger["limit"]:
                raise ValueError("A token ceiling can be lowered but never raised mid-run")
        if ledger["limit"] is None:
            raise ValueError("No token ceiling is declared; refusing to spend without a bound")
        committed = committed_tokens(ledger["calls"].values())
        if committed + reserve > ledger["limit"]:
            raise ValueError(f"Token budget exhausted: {committed}+{reserve} exceeds {ledger['limit']}")
        ledger["reserved"] += reserve
        ledger["calls"][call_id] = {"gate_id": gate_id, "request_sha256": request_sha256,
                                    "reserved": int(reserve), "actual": None}

    store.mutate(run_id, "tokens.admitted", apply)
    return call_id


def record_token_failure(store, run_id, call_id, failure):
    """A failed call keeps its reservation: what it consumed is unknown, not zero."""
    store.mutate(run_id, "tokens.failed",
                 lambda current: token_ledger(current["budget"])["calls"][call_id].update(error=failure))


def settle_tokens(store, run_id, call_id, usage):
    """Close a call against what the server reported for THIS turn."""
    def apply(current):
        ledger = token_ledger(current["budget"])
        entry = ledger["calls"][call_id]
        total = usage.get("total_tokens")
        if total is None and {"input_tokens", "output_tokens"} <= usage.keys():
            total = usage["input_tokens"] + usage["output_tokens"]
        entry.update(actual=total, usage=dict(usage))
        if total is not None and total > entry["reserved"]:
            # Visible, not silent: an under-reservation means the ceiling did not
            # bind this call, and the overhead floor needs raising.
            entry["overrun"] = total - entry["reserved"]
        settled = [c for c in ledger["calls"].values() if c.get("actual") is not None]
        ledger["actual"] = sum(c["actual"] for c in settled)
        ledger["committed"] = committed_tokens(ledger["calls"].values())

    store.mutate(run_id, "tokens.settled", apply)
