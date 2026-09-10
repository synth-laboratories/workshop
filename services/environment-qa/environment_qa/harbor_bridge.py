"""The only tool surface an AI gate is given.

Harbor keeps doing what Harbor is for -- provisioning a container and running a
command in it -- and nothing here adds a second model loop on top of it. What this
module does is decide, for one gate, which of Harbor's capabilities are reachable
and with what arguments.

The published set is deliberately three verbs. A gate can read evidence it was
granted, run a command inside its own container, and submit its result. It cannot
reach a host shell, the credential filesystem, the Docker socket, reference labels,
or another gate's artifacts, because none of those are verbs it can name. Refusing
an unknown tool loudly matters more than it looks: a silently ignored tool call
becomes a gate that "found nothing", which is indistinguishable from a clean task.
"""
from __future__ import annotations

import json

TOOLS = ("read_evidence", "container_exec", "submit_result")


class ToolRefused(RuntimeError):
    """The gate asked for something outside its scope."""


def tool_definitions(schema):
    """What the gate is told it may call. `schema` types the submission."""
    return [
        {"name": "read_evidence", "description": "Read one evidence item granted to this gate.",
         "parameters": {"type": "object", "required": ["evidence_id"], "additionalProperties": False,
                        "properties": {"evidence_id": {"type": "string"}}}},
        {"name": "container_exec", "description": "Run one command inside this gate's container.",
         "parameters": {"type": "object", "required": ["command"], "additionalProperties": False,
                        "properties": {"command": {"type": "array", "items": {"type": "string"}},
                                       "timeout_seconds": {"type": "integer"}}}},
        {"name": "submit_result", "description": "Submit exactly one structured QA result.",
         "parameters": schema},
    ]


class HarborBridge:
    """Routes a gate's tool calls to the narrow set it is permitted.

    `evidence` maps an id to already-projected content: the projection happened
    before the gate started, so a gate cannot widen its own view by asking. `exec_`
    is Harbor's command runner for this gate's container, or None when the profile
    grants no execution.
    """

    def __init__(self, evidence=None, exec_=None, limits=None):
        self.evidence = dict(evidence or {})
        self.exec_ = exec_
        self.limits = {"exec_calls": 50, "timeout_seconds": 120, **(limits or {})}
        self.calls = 0

    def call(self, name, raw_arguments):
        """Run one published tool. Raises ToolRefused for anything out of scope."""
        try:
            arguments = json.loads(raw_arguments) if isinstance(raw_arguments, str) else (raw_arguments or {})
        except json.JSONDecodeError:
            raise ToolRefused(f"Tool {name!r} sent arguments that are not JSON")
        if not isinstance(arguments, dict):
            raise ToolRefused(f"Tool {name!r} arguments must be an object")
        if name not in TOOLS:
            raise ToolRefused(f"Tool {name!r} is not published to this gate")
        return getattr(self, f"_{name}")(arguments)

    def handle(self, executor, params):
        """Answer a tool-call item, or return None when the item is not one."""
        item = params.get("item") or {}
        if item.get("type") not in {"function_call", "functionCall", "tool_call", "toolCall", "custom_tool"}:
            return None
        name = item.get("name")
        result = self.call(name, item.get("arguments"))
        executor.record("tool.result", {"tool": name, "call_id": item.get("id"),
                                        "ok": result.get("ok", True)})
        return {"tool": name, "call_id": item.get("id"), **result}

    def _read_evidence(self, arguments):
        evidence_id = arguments.get("evidence_id")
        if evidence_id not in self.evidence:
            raise ToolRefused(f"Evidence {evidence_id!r} was not granted to this gate")
        return {"ok": True, "content": self.evidence[evidence_id]}

    def _container_exec(self, arguments):
        if self.exec_ is None:
            raise ToolRefused("This gate's profile grants no command execution")
        if self.calls >= self.limits["exec_calls"]:
            raise ToolRefused(f"Exceeded {self.limits['exec_calls']} command executions")
        command = arguments.get("command")
        if not isinstance(command, list) or not command or not all(isinstance(a, str) for a in command):
            raise ToolRefused("container_exec takes a non-empty argv list, never a shell string")
        requested = arguments.get("timeout_seconds", self.limits["timeout_seconds"])
        if type(requested) is not int or requested <= 0:
            raise ToolRefused("timeout_seconds must be a positive integer")
        self.calls += 1
        timeout = min(requested, self.limits["timeout_seconds"])
        return {"ok": True, **self.exec_(command, timeout)}

    def _submit_result(self, arguments):
        if self.exec_ is not None and self.calls and arguments.get('command'):
            raise ToolRefused('Commands already executed through container_exec. Submit an empty command to avoid duplicate side effects; retain your observations in rationale.')
        return {"ok": True, "arguments": arguments}
