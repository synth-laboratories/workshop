"""Prove the new profiles cannot reach a provider except through the executor.

The claim being audited is narrow and checkable: in a pipeline run, every AI gate
dispatches through `dispatch.request_json`, and no module it reaches can open an
authenticated provider connection itself.

Two failure modes matter more than the happy path.

The first is a fallback added later in good faith -- a `try: executor / except:
direct request` that keeps a demo alive and quietly restores the unmetered,
unisolated path. `importers` catches that, because reaching the direct transport
means importing it.

The second is this audit going stale. A brand-new provider call in a brand-new
module would satisfy an allowlist of known-bad names while being exactly the thing
the audit exists to find. So transports are *discovered* structurally -- a module
that both dials out and sends an Authorization header is a provider transport,
whatever it is called -- and the audit fails when that discovered set stops
matching the declared one. Adding a transport therefore forces a decision here.

`source_inventory` fetches public package metadata with no credentials, and is
classified as not a provider transport by the same rule rather than by exception.
"""
from __future__ import annotations

import ast
import json
from pathlib import Path

PACKAGE = Path(__file__).parent

# Transports that authenticate to a model provider directly. Declared, then checked
# against what is actually in the tree.
DECLARED_TRANSPORTS = {"inference", "review", "provider_guard"}

# Modules allowed to reference a direct transport at all. `worker` is the legacy
# non-pipeline path, which the new profiles do not use; it is named here so the
# audit reports it rather than pretending it is gone.
PERMITTED_REFERENCES = {"inference", "review", "worker", "dispatch"}

DIRECT_SYMBOLS = {"request_json": "inference", "call_ai": "review", "ProviderGuard":"provider_guard"}


def modules():
    return sorted(p for p in PACKAGE.glob("*.py") if p.name != "__init__.py")


def _tree(path):
    return ast.parse(path.read_text(), filename=str(path))


def dials_out(tree):
    """Makes outbound HTTP requests.

    Serving HTTP is not dialling out: `server.py` imports `http.server` and checks
    an auth header on requests it *receives*, which is the opposite of a provider
    transport. Only client machinery counts.
    """
    for node in ast.walk(tree):
        if isinstance(node, ast.Import) and any(a.name.startswith("urllib.request") or a.name in {"httpx", "requests"}
                                                for a in node.names):
            return True
        if isinstance(node, ast.ImportFrom) and (node.module or "").startswith(("urllib.request", "httpx", "requests")):
            return True
    return False


def sends_credentials(tree):
    """An Authorization header, or a bearer token, in a literal anywhere in the module."""
    for node in ast.walk(tree):
        if isinstance(node, ast.Constant) and isinstance(node.value, str):
            text = node.value.lower()
            if "authorization" in text or text.startswith("bearer "):
                return True
    return False


def discovered_transports():
    """Modules that both dial out and authenticate: provider transports, by structure."""
    found = {}
    for path in modules():
        tree = _tree(path)
        outbound, credentials = dials_out(tree), sends_credentials(tree)
        if outbound and credentials:
            found[path.stem] = {"outbound_http": True, "credentials": True}
    return found


def importers():
    """Which modules import a direct transport symbol from its defining module."""
    out = {}
    for path in modules():
        for node in ast.walk(_tree(path)):
            if not isinstance(node, ast.ImportFrom):
                continue
            source = (node.module or "").lstrip(".")
            for alias in node.names:
                if DIRECT_SYMBOLS.get(alias.name) == source:
                    out.setdefault(path.stem, []).append(f"{source}.{alias.name}")
    return out


def dispatch_users():
    """Modules that call request_json, and where each imports it from."""
    out = {}
    for path in modules():
        tree = _tree(path)
        origin = None
        for node in ast.walk(tree):
            if isinstance(node, ast.ImportFrom) and any(a.name == "request_json" for a in node.names):
                origin = (node.module or "").lstrip(".")
        calls = any(isinstance(n, ast.Call) and isinstance(n.func, ast.Name) and n.func.id == "request_json"
                    for n in ast.walk(tree))
        if calls or origin:
            out[path.stem] = origin
    return out


def audit():
    """Return a report. `passed` is false if any checked property does not hold."""
    discovered = discovered_transports()
    references = importers()
    users = dispatch_users()

    unexpected_transports = sorted(set(discovered) - DECLARED_TRANSPORTS)
    missing_transports = sorted(DECLARED_TRANSPORTS - set(discovered))
    illegal_references = {m: s for m, s in references.items() if m not in PERMITTED_REFERENCES}
    # Every module that calls request_json in a pipeline gate must take it from
    # dispatch. `dispatch` itself defines it; `inference` still owns the legacy one.
    wrong_origin = {m: origin for m, origin in users.items()
                    if m not in {"dispatch", "inference"} and origin != "dispatch"}

    checks = {
        "transports_are_declared": not unexpected_transports,
        "declared_transports_exist": not missing_transports,
        "no_gate_module_references_a_direct_transport": not illegal_references,
        "every_gate_dispatches_through_the_executor": not wrong_origin,
    }
    return {"schema": "environment-qa.fallback-audit.v1",
            "passed": all(checks.values()), "checks": checks,
            "discovered_transports": sorted(discovered),
            "declared_transports": sorted(DECLARED_TRANSPORTS),
            "unexpected_transports": unexpected_transports,
            "missing_transports": missing_transports,
            "direct_transport_references": references,
            "illegal_references": illegal_references,
            "request_json_origins": users,
            "wrong_origin": wrong_origin,
            "note": ("worker/review remain the legacy non-pipeline path and are reported, not hidden. "
                     "source_inventory fetches unauthenticated public metadata and is not a provider transport.")}


def main(argv=None):
    report = audit()
    print(json.dumps(report, indent=2))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
