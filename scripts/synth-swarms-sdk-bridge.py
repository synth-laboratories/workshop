#!/usr/bin/env python3
"""Narrow stdio bridge from Workshop to the pinned public Synth Swarms SDK.

The request, including credentials, is accepted on stdin so secrets never
appear in process arguments. Successful one-shot operations write one JSON
object. ``watch`` writes one JSON object per SDK-decoded SSE event.
"""

from __future__ import annotations

import argparse
import asyncio
import dataclasses
import json
import sys
from collections.abc import Mapping
from datetime import datetime
from enum import Enum
from typing import Any
from urllib.parse import urlsplit

from synth_ai.sdk.research.client import AsyncClient
from synth_ai.sdk.research.contracts.common import ProjectId, SwarmId
from synth_ai.sdk.research.contracts.swarms import (
    ActiveActorModel,
    ActorHarness,
    BoundRuntimeExecutionTarget,
    InferenceProvider,
    PlatformResolvedExecutionTarget,
    ResourceLimit,
    Runbook,
    SwarmSpec,
    WorkMode,
)


def _read_request() -> dict[str, Any]:
    value = json.load(sys.stdin)
    if not isinstance(value, dict):
        raise ValueError("bridge request must be a JSON object")
    return value


def _required_text(value: Any, name: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise ValueError(f"{name} must be a non-empty string")
    return value.strip()


def _optional_text(value: Any, name: str) -> str | None:
    if value is None:
        return None
    return _required_text(value, name)


def _wire(value: Any) -> Any:
    if hasattr(value, "to_wire"):
        return value.to_wire()
    if hasattr(value, "items") and hasattr(value, "next_cursor"):
        return {
            "items": [_wire(item) for item in value.items],
            "next_cursor": value.next_cursor,
            "has_more": value.has_more,
        }
    if dataclasses.is_dataclass(value):
        return {field.name: _wire(getattr(value, field.name)) for field in dataclasses.fields(value)}
    if isinstance(value, Enum):
        return value.value
    if isinstance(value, datetime):
        return value.isoformat()
    if isinstance(value, Mapping):
        return {str(key): _wire(child) for key, child in value.items()}
    if isinstance(value, (tuple, list)):
        return [_wire(child) for child in value]
    return value


def _limit(value: Any) -> ResourceLimit | None:
    if value is None:
        return None
    if not isinstance(value, Mapping):
        raise ValueError("spec.limit must be an object")
    allowed = {
        "maxSpendUsd": "max_spend_usd",
        "maxTokens": "max_tokens",
        "maxWallclockSeconds": "max_wallclock_seconds",
        "maxGpuHours": "max_gpu_hours",
        "maxConcurrentActors": "max_concurrent_actors",
    }
    unknown = set(value) - set(allowed)
    if unknown:
        raise ValueError(f"unsupported spec.limit fields: {', '.join(sorted(unknown))}")
    return ResourceLimit(**{allowed[key]: child for key, child in value.items()})


def _execution_target(value: Any):
    if value is None:
        return PlatformResolvedExecutionTarget()
    if not isinstance(value, Mapping):
        raise ValueError("spec.executionTarget must be an object")
    kind = value.get("kind")
    if kind == "platform_resolved":
        return PlatformResolvedExecutionTarget()
    if kind == "bound_runtime":
        attestation = value.get("attestation")
        if not isinstance(attestation, Mapping):
            raise ValueError("bound runtime requires an attestation object")
        return BoundRuntimeExecutionTarget(attestation=dict(attestation))
    raise ValueError("executionTarget.kind must be platform_resolved or bound_runtime")


def _spec(value: Any, *, hosted: bool = False) -> SwarmSpec:
    if not isinstance(value, Mapping):
        raise ValueError("spec must be an object")
    allowed = {
        "objective", "workMode", "runbook", "runbookPreset", "timeboxSeconds", "agentModel",
        "agentHarness", "provider", "requiredCapabilities", "idempotencyKey",
        "limit", "executionTarget",
    }
    unknown = set(value) - allowed
    if unknown:
        raise ValueError(f"unsupported spec fields: {', '.join(sorted(unknown))}")
    # Public hosted runbooks own the exact actor model, harness, and provider
    # through their reviewed preset. Raw selections are an internal/local
    # control and the production API rejects them. Keep the explicit fields on
    # loopback so Workshop's local-slot QA still proves the Codex app-server
    # path without weakening the public API boundary.
    providers = None if hosted else value.get("provider")
    if isinstance(providers, list):
        provider: Any = tuple(InferenceProvider(item) for item in providers)
    elif providers is None:
        provider = None
    else:
        provider = InferenceProvider(providers)
    return SwarmSpec(
        objective=_required_text(value.get("objective"), "spec.objective"),
        work_mode=WorkMode(value["workMode"]) if value.get("workMode") else None,
        runbook=Runbook(value["runbook"]) if value.get("runbook") and not hosted else None,
        runbook_preset=_optional_text(value.get("runbookPreset"), "spec.runbookPreset") if hosted else None,
        timebox_seconds=value.get("timeboxSeconds"),
        agent_model=(ActiveActorModel(value["agentModel"]) if value.get("agentModel") and not hosted else None),
        agent_harness=(ActorHarness(value["agentHarness"]) if value.get("agentHarness") and not hosted else None),
        provider=provider,
        required_capabilities=tuple(value.get("requiredCapabilities") or ()),
        idempotency_key=_optional_text(value.get("idempotencyKey"), "spec.idempotencyKey"),
        limit=_limit(value.get("limit")),
        execution_target=_execution_target(value.get("executionTarget")),
    )


async def _run(operation: str, request: dict[str, Any]) -> None:
    base_url = _required_text(request.get("baseUrl"), "baseUrl")
    hosted = not base_url.startswith(("http://127.0.0.1", "http://localhost", "http://[::1]"))
    deployment = f"hosted:{urlsplit(base_url).hostname}" if hosted else "local"
    api_key = _required_text(request.get("apiKey"), "apiKey")
    payload = request.get("payload") or {}
    if not isinstance(payload, Mapping):
        raise ValueError("payload must be an object")
    async with AsyncClient(api_key=api_key, base_url=base_url) as client:
        api = client.swarms
        project = payload.get("projectId")
        project_id = ProjectId(project) if project else None
        if operation == "capabilities":
            result = {
                "schemaVersion": "workshop.swarms-sdk-bridge.v1",
                "transport": "synth-ai-sdk",
                "stream": "sse",
                "deployment": deployment,
                "operations": [
                    "preflight", "create", "list", "retrieve", "configuration",
                    "status", "activity", "usage", "evidence", "pause", "resume",
                    "cancel", "watch",
                ],
            }
        elif operation == "preflight":
            result = _wire(await api.preflight(_spec(payload.get("spec"), hosted=hosted), project_id=project_id))
        elif operation == "create":
            handle = await api.create(
                _spec(payload.get("spec"), hosted=hosted),
                project_id=project_id,
                runtime_binding_context_id=payload.get("runtimeBindingContextId"),
            )
            result = _wire(handle.initial)
        elif operation == "list":
            if project_id is None:
                raise ValueError("list requires projectId")
            result = _wire(await api.list(project_id, limit=int(payload.get("limit", 100)), cursor=payload.get("cursor")))
        else:
            swarm_id = SwarmId(_required_text(payload.get("swarmId"), "payload.swarmId"))
            if operation == "retrieve":
                result = _wire(await api.retrieve(swarm_id))
            elif operation == "configuration":
                result = _wire(await api.configuration(swarm_id))
            elif operation == "status":
                result = _wire(await api.status(swarm_id))
            elif operation == "activity":
                result = _wire(await api.activity(swarm_id))
            elif operation == "usage":
                result = _wire(await api.usage(swarm_id))
            elif operation == "evidence":
                result = _wire(await api.evidence(swarm_id))
            elif operation == "pause":
                result = _wire(await api.pause(swarm_id))
            elif operation == "resume":
                result = _wire(await api.resume(swarm_id))
            elif operation == "cancel":
                result = _wire(await api.cancel(swarm_id))
            elif operation == "watch":
                async for event in api.events(
                    swarm_id,
                    transcript_cursor=payload.get("transcriptCursor"),
                    last_event_id=payload.get("lastEventId"),
                    timeout_seconds=payload.get("timeoutSeconds"),
                ):
                    print(json.dumps({"ok": True, "event": event.to_wire()}, separators=(",", ":")), flush=True)
                return
            else:
                raise ValueError(f"unsupported operation: {operation}")
        print(json.dumps({"ok": True, "result": result}, separators=(",", ":")), flush=True)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("operation")
    args = parser.parse_args()
    try:
        asyncio.run(_run(args.operation, _read_request()))
        return 0
    except Exception as error:  # bounded display-safe bridge error
        print(json.dumps({"ok": False, "error": type(error).__name__, "message": str(error)[:1000]}, separators=(",", ":")), flush=True)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
