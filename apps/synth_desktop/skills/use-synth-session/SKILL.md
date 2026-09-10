---
name: use-synth-session
description: Use when updating this conversation's title, mascot emotion, or a short on-screen summary.
---

# Use Synth Session

Codex advertises one compact custom tool, `mcp__synth_session__session_present`. In code mode call it as `tools.mcp__synth_session__session_present({ title?, emotion?, summary? })`. There is **no** top-level `method` field and **no** `session_id` argument — Desktop binds the tool to the current conversation through `SYNTH_SESSION_ID`.

Omit any field you are not changing. At least one field is required.

| Field | Rules |
| --- | --- |
| `title` | Manual CoreRuntime rename. Calls the same `set_title(..., Manual)` path as a sidebar rename, then `thread/name/set` when the Codex attachment is live. After understanding the task, replace the temporary automatic title with a concise, specific title. Call it again whenever the durable objective materially changes. Repeated calls update this chat; do not invent a second title store. |
| `emotion` | `idle` \| `thinking` \| `working` \| `success`. Overlay used when the host is **not** running a turn. While a turn is running, Desktop shows `working` if tools are open and `thinking` otherwise. |
| `summary` | At most **seven** whitespace-separated words. Longer values are rejected, never truncated. Shown under the optional mascot. |

Do not call this tool on every token. Set emotion when the task mood changes, summary when the user-visible gist changes, and title once the task is understood and again only when its durable objective materially changes.

Examples:

```
{"emotion":"thinking","summary":"Reading Craftax reward traces"}
{"emotion":"success","summary":"Reward curve flattened"}
{"title":"Craftax reward investigation"}
```
