---
name: workshop
description: Use the connected local Workshop instance for product operations, research visuals, native full-screen presentation, and configured ACP agents. Use when the user asks to work in Workshop or show Workshop visuals.
---

# Workshop

Use the connected Workshop MCP tools. The connection names a local instance;
its shared visual library may include outputs from multiple conversations.
Do not change connection settings or switch instances as part of an ordinary task.

Read the available tool descriptions before choosing a workflow. The server's
catalogue is authoritative for this runtime version; missing operations are not
available merely because another Workshop build supports them.

For visuals, inspect existing objects with `visual_list` and `visual_get`.
Use `visual_list_templates` to read registered template contracts before creating
a visual. Bind the user's actual data in the template's declared format. Bundled
examples are previews, not experiment results. For source-authored templates,
provide source in the format described by that template.

Update an existing visual with `visual_update` and its current revision. A
revision conflict means another writer changed it; read the new state and
reconcile the requested change before retrying.

Create with `visual_create`, reuse its returned ID, then call `visual_present`.
Set `fullscreen` when the user requests full-screen presentation. Shared visuals
need no Workshop chat or hosted session ID. Creation is not idempotent: after an
uncertain response, inspect the library before attempting the same creation again.

Presentation returns a request acknowledgement, not rendering proof. Read
`visual_observe` and compare the observed revision with the current visual
revision. A null or stale observation is not success. Semantic observations do
not replace inspecting a screenshot when the task requires visual quality review.
Use `visual_capture` for an actual PNG and native capture receipt on macOS.

Check the available operations before promising interaction replay, agent
wake-up, or background execution. Report an unavailable capability plainly;
do not invent tools, fabricate observations, borrow hosted session IDs, or fall
back to direct database writes.

Use `runtime_status` to inspect the connected process. `runtime_control` attaches
or detaches a desktop without replacing the runtime. Stop only when the task
calls for stopping all work in that instance. `visual_present` and `app_capture`
can attach the desktop; `app_capture` returns the actual app state and pixels.
Durable application events are available through `core_events_after`; retain
its sequence cursor instead of treating a screenshot as event history.

For hosted agents, inspect `agent_backends_list` first. Start only a registered
backend; use the returned Workshop session ID for send, cancel, close, and
explicit resume. Check `agent_sessions_list` and session journal events for
completion. A send acknowledgement is not a completed turn. Honor parent IDs
when delegating related work. Resume depends on the backend's loadSession
capability. Do not replay a prompt after an uncertain outcome without checking
its retained journal. Human permission requests must be answered by the user;
never attempt to invoke approval decisions or manufacture human evidence.

Use the named domain tools in the catalogue for container rollouts, visual
bindings/reviews, reports, annotations, diagnostics and managed browsing. They
share Workshop's existing handlers and approvals. A task-owned operation may
require an existing `session_id` for correlation; use an actual task you are
working on, never manufacture one to bypass an ownership check. Shared visual
creation and forks need no task.

Use `desktop_state_get` and `desktop_state_update` for persisted UI choices.
Preserve unrelated preference fields and pass the current expected revision.
Do not change approval or sandbox preferences. Use `app_present` to request a
page such as Settings, and inspect `app_capture` before describing its display.
Browser and Computer Use remain operator opt-ins in Settings → Context. Browser
origin approvals remain human-controlled. Managed browser sessions belong to
the runtime and survive MCP client disconnects; close sessions you created when
the task no longer needs them.
