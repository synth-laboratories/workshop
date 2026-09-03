---
name: use-synth-diagnostics
description: Use when a Workshop surface failed and you need to know why — an empty or wrong visual, a dropped live stream, a refused rollout, an MCP error, an optimizer that stopped, or a provider that went quiet.
---

# Use Synth diagnostics

`synth-diagnostics-mcp` is the local record of what failed in this Workshop
instance and how those failures are related. Everything is on this machine.
Codex exposes it as the `mcp__synth_diagnostics` namespace, through one tool:
`diagnostics_manage`.

Never diagnose by shell, by reading the SQLite database, by tailing a log file,
or by taking screenshots to see whether something looks wrong. Those recover
guesses; this returns the record.

## Explain first, query second

This is the whole skill. **Start from an identity you already hold and call
`explain`.** You almost always have one: the visual you just created, the
rollout you just started, the task you are in, the trace you just sealed.

```json
{"operation": "explain", "arguments": {"visual_id": "vis_9", "since": "20m"}}
```

`explain` expands one hop through the correlations it finds, then orders what it
finds by *cause*, not by time. It returns:

- `cause` — the most upstream failure, which is usually **not** the one the user
  saw. A blank visual is reported as a symptom of the container capability
  rejection or stream timeout that produced it.
- `symptoms` — everything downstream of that cause.
- `remediation` — what to do, as text. Follow it before forming a theory.
- `identities` — every id discovered along the way, including ones you did not
  supply. These are your next lookups.

Reaching for `query` first is the common mistake: you page through symptoms in
timestamp order and never reach the cause.

## Then narrow

Once `explain` names a code, `query` gets you every occurrence of it:

```json
{"operation": "query", "arguments": {"code": ["container_capability_rejected"], "since": "2h", "limit": 50}}
```

Useful arguments: `scope` (`visuals`, `containers`, `streams`, `mcp`,
`optimizers`, `providers`, `renderer`, `session`), `severity`, `code`, `event`,
any correlation id, `since` (max `7d`), `limit` (max 500), and `cursor` from a
previous response to page.

`tail` is `query` for the newest events with no paging. Use it while something
is running; use `query` afterwards.

## Read the status before you trust an absence

```json
{"operation": "status"}
```

`state` is `ready`, `degraded`, `starting`, or `stopped`. **Degraded does not
mean evidence is missing** — queries answer from the authoritative journal
either way, and only speed changes. But `index_lag` and `queue.depth` tell you
whether something recent may not be searchable yet, and `stored_events` of 0 for
a window means the failure genuinely was not recorded, not that the query was
wrong.

If a diagnostic you expect is absent, that surface is probably not instrumented.
Say so rather than inventing a cause.

## Bundling

```json
{"operation": "bundle", "arguments": {"since": "2h", "severity": ["error"]}}
```

Writes a redacted local file and returns its path. It uploads nothing and shares
nothing; hand the path to the user if they want to attach it somewhere.

## What this tool will not do

- No LogsQL, no SQL, no file paths, no URLs. There is no parameter for them.
- No prompt text and no credentials: they are redacted before they are ever
  stored, so a query cannot return them and a bundle cannot leak them.
- No range beyond 7 days, no more than 500 rows, no unbounded response.

If you need something outside those bounds, the answer is a narrower question,
not another tool.
