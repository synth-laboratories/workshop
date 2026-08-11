# Live Intern installed-app dogfood

## 2026-08-09 result

Transport, persistence, controls, and restart coverage passed in the installed
app against staging. Sync runtime `dc70d820-603a-4929-a763-056fbc268d3f`
accepted create, operator message, pause, and resume. Async runtime
`4fdb5314-e322-480b-902f-6088747bc4b7` accepted ensure, checkpoint instruction,
resume, and pause; one Rust-backed local singleton survived restart without
cursor regression or duplicate event IDs. SQLite integrity and foreign keys
remained clean, and no Python product runtime was present.

The remaining service-side assertion did not pass: staging emitted no
`agent_message` for Sync and no worker/checkpoint result after the accepted
Async instructions. The last observed service events were `operator_message`
for Sync and `async_resumed_with_instructions` for Async. Desktop ingested every
event through the reported remote cursor, so the open gate is worker execution,
not the Rust HTTP/journal/renderer transport.

Prepared against the read-only staging OpenAPI document at
`https://api-dev.usesynth.ai/openapi.json` on 2026-08-09. Do not use `curl` for
the steps below: exercise the installed Tauri application so the Rust core,
SQLite journal, renderer bridge, and restart path are all covered.

## Safety and preflight

1. Use a staging-capable account with a deliberately small test objective. Do
   not paste or print the API key in a terminal or test log.
2. In Settings -> Account -> Synth backend, select `staging`. Confirm the shown
   endpoint is `https://api-dev.usesynth.ai`, the API key is reported as
   configured, and click **Save and reconnect**.
3. In Settings -> Runtime, copy the reported database path. For the ordinary
   installed macOS app it should be:

   ```text
   ~/Library/Application Support/Synth Desktop/synth.sqlite3
   ```

   A named/development instance can use a different root; the Runtime value is
   authoritative. Set `DB_PATH` to that exact value in a separate terminal.
4. Keep the application open while running read-only `sqlite3` queries. WAL
   mode and a 30-second busy timeout make concurrent reads safe.
5. Before Async testing, check local cardinality:

   ```sql
   SELECT id, remote_id, status, created_at
   FROM sessions
   WHERE json_extract(target_json, '$.kind') = 'intern'
     AND json_extract(target_json, '$.mode') = 'async'
   ORDER BY created_at DESC;
   ```

   Run the Async ensure test only once on a database with no prior Async local
   session. Staging Async is one runtime per organization, while the desktop can
   currently create multiple local sessions and each new local binding starts
   its source cursor at zero. Repeating ensure against the same remote ledger
   risks replaying globally duplicate `event_id` values into the local journal.

Check database integrity before and after the run:

```sh
sqlite3 "$DB_PATH" 'PRAGMA integrity_check; PRAGMA foreign_key_check;'
```

Expected: one line `ok` and no foreign-key rows.

## Reusable latest-session evidence

Replace `sync` with `async` when inspecting the Background Intern.

```sql
WITH chosen AS (
  SELECT id FROM sessions
  WHERE json_extract(target_json, '$.kind') = 'intern'
    AND json_extract(target_json, '$.mode') = 'sync'
  ORDER BY created_at DESC LIMIT 1
)
SELECT s.id,
       json_extract(s.target_json, '$.mode') AS mode,
       s.remote_id,
       s.status,
       s.state_generation,
       s.latest_cursor AS local_cursor,
       s.active_run_id,
       json_extract(s.metadata_json, '$.objective') AS objective,
       COALESCE(json_extract(s.metadata_json, '$.intern.projection.status'),
                json_extract(s.metadata_json, '$.projection.status')) AS remote_status,
       c.cursor AS remote_cursor
FROM chosen x
JOIN sessions s ON s.id = x.id
LEFT JOIN source_cursors c ON c.session_id = s.id AND c.source = 'intern';
```

Cursor invariants for that session:

```sql
WITH chosen AS (
  SELECT id FROM sessions
  WHERE json_extract(target_json, '$.kind') = 'intern'
    AND json_extract(target_json, '$.mode') = 'sync'
  ORDER BY created_at DESC LIMIT 1
)
SELECT s.id,
       s.latest_cursor,
       COALESCE(MAX(e.session_sequence), 0) AS journal_head,
       c.cursor AS source_cursor,
       COALESCE(MAX(e.remote_sequence), 0) AS remote_head
FROM chosen x
JOIN sessions s ON s.id = x.id
LEFT JOIN events e ON e.session_id = s.id
LEFT JOIN source_cursors c ON c.session_id = s.id AND c.source = 'intern'
GROUP BY s.id, s.latest_cursor, c.cursor;
```

Expected: `latest_cursor = journal_head` and `source_cursor = remote_head`.

## 1. Sync create and objective

1. From the landing screen select **Live Intern**.
2. Enter a unique objective such as
   `DOGFOOD-SYNC-<timestamp>: answer with the word cobalt and one short sentence.`
3. Submit once. Do not immediately submit a second message.

UI evidence:

- A single Live Intern conversation opens.
- The prompt starts creation; it is not sent a second time as an operator
  message.
- The session receives a nonempty remote ID and leaves the initial loading
  state. Depending on remote timing, its durable status may already be
  `running`, `interrupted`, or `closed` rather than `ready`.

SQLite evidence:

- Latest Sync row has `remote_id IS NOT NULL`.
- Root `metadata_json.objective` equals the submitted objective, trimmed.
- `target_json.mode = 'sync'`.
- The journal contains one `session.created` followed by one `session.updated`.
- There is no local `runs` row solely for the creation objective; creation is
  the remote start operation.

```sql
WITH chosen AS (
  SELECT id FROM sessions
  WHERE json_extract(target_json, '$.kind') = 'intern'
    AND json_extract(target_json, '$.mode') = 'sync'
  ORDER BY created_at DESC LIMIT 1
)
SELECT e.session_sequence, e.kind, e.remote_sequence, e.command_id
FROM chosen x JOIN events e ON e.session_id = x.id
ORDER BY e.session_sequence;

WITH chosen AS (
  SELECT id FROM sessions
  WHERE json_extract(target_json, '$.kind') = 'intern'
    AND json_extract(target_json, '$.mode') = 'sync'
  ORDER BY created_at DESC LIMIT 1
)
SELECT COUNT(*) AS creation_should_have_no_local_run
FROM chosen x JOIN runs r ON r.session_id = x.id;
```

The second query should return `0` before any follow-up message.

## 2. Sync response ingestion and follow-up

1. Wait for the requested response to appear in chat.
2. Submit one unique follow-up message.
3. Wait for a second agent response.

The present Rust transport uses bounded REST polling, not the staging SSE
route. It polls event pages after the durable source cursor, with an idle
interval near 900 ms, and refreshes the remote projection about every 4 s.
Therefore “streamed” here means incrementally arriving durable mailbox events
reaching the renderer through `runtime:event`; it does not prove SSE usage.

UI evidence:

- An event of kind `agent_message` or `intern.agent_message` renders as an
  assistant chat message. `message.created` / `message.delta` /
  `message.completed` are also renderer-supported if staging emits them.
- A follow-up does not create a duplicate assistant response on event replay.

SQLite evidence:

- Remote events have `source = 'intern'`, a positive `remote_sequence`, and
  monotonically increasing `session_sequence`.
- The follow-up creates one local run and one command receipt. A 202 receipt
  with `received`, `delivered`, `applied`, or `noop` settles both to
  `completed`. A 202 receipt with `refused`, `superseded`, or `conflict` marks
  the local receipt `rejected` and the message run `failed`; the later agent
  response remains a mailbox event rather than keeping that local run active.
- The receipt response contains the staging command receipt, including
  `runtime_kind`, `runtime_id`, `status`, and `state_generation`.

```sql
WITH chosen AS (
  SELECT id FROM sessions
  WHERE json_extract(target_json, '$.kind') = 'intern'
    AND json_extract(target_json, '$.mode') = 'sync'
  ORDER BY created_at DESC LIMIT 1
)
SELECT r.id AS run_id, r.status AS run_status, r.started_at, r.completed_at,
       cr.command_id, cr.kind, cr.status AS receipt_status,
       json_extract(cr.response_json, '$.runtime_kind') AS receipt_runtime_kind,
       json_extract(cr.response_json, '$.runtime_id') AS receipt_runtime_id,
       json_extract(cr.response_json, '$.state_generation') AS receipt_generation
FROM chosen x
JOIN runs r ON r.session_id = x.id
LEFT JOIN command_receipts cr ON cr.run_id = r.id
ORDER BY r.created_at DESC;

WITH chosen AS (
  SELECT id FROM sessions
  WHERE json_extract(target_json, '$.kind') = 'intern'
    AND json_extract(target_json, '$.mode') = 'sync'
  ORDER BY created_at DESC LIMIT 1
)
SELECT e.remote_sequence, e.session_sequence, e.kind,
       substr(CAST(e.payload_json AS TEXT), 1, 180) AS payload_preview
FROM chosen x JOIN events e ON e.session_id = x.id
WHERE e.source = 'intern' AND e.remote_sequence IS NOT NULL
ORDER BY e.remote_sequence;
```

## 3. Sync control

Use **Pause**, wait for the UI to show paused, then **Resume**. Avoid Close until
restart coverage is complete.

Expected transitions:

```text
UI/remote active -> pause command 202 -> projection paused
SQLite session interrupted -> wire/UI paused
UI paused -> resume command 202 -> projection active/ready/thinking
SQLite session running or ready
```

Each control creates a `command_receipts` row with `kind = pause|resume`, first
`accepted` and then either `completed` or `rejected` according to the remote
semantic status, plus matching `command.accepted` and `command.resolved`
journal events. Controls do not create a new run unless they are attached to an
already active one.

```sql
WITH chosen AS (
  SELECT id FROM sessions
  WHERE json_extract(target_json, '$.kind') = 'intern'
    AND json_extract(target_json, '$.mode') = 'sync'
  ORDER BY created_at DESC LIMIT 1
)
SELECT cr.command_id, cr.kind, cr.status, cr.remote_cursor,
       json_extract(cr.response_json, '$.status') AS remote_receipt_status,
       json_extract(cr.response_json, '$.decision_code') AS decision_code
FROM chosen x JOIN command_receipts cr ON cr.session_id = x.id
WHERE cr.kind IN ('pause', 'resume')
ORDER BY cr.created_at;
```

Do not require the remote receipt `status` to equal `applied`: staging permits
`received`, `delivered`, `applied`, `noop`, `refused`, `superseded`, and
`conflict`. The first four map to local `completed`; the last three map to local
`rejected`. An unknown status is a protocol failure. Confirm the subsequent
projection agrees with the receipt.

## 4. Sync quit/reopen and cursor resume

1. Record the Sync `source_cursors.cursor` value `C` and the count of remote
   events.
2. Quit the installed application normally after the follow-up command receipt
   is complete and `active_run_id` is null.
3. Reopen it. Open the same Live Intern conversation.
4. Wait at least 5 seconds for provider reattachment and projection refresh.

UI evidence:

- The session and previous chat messages are restored from SQLite before any
  new network events are needed.
- Any events produced while closed appear once after reopen.
- The remote session is not recreated; its `remote_id` is unchanged.

SQLite evidence:

- Cursor never falls below `C`.
- Polling resumes from `C`; old remote events are not duplicated.
- `source_cursor = MAX(events.remote_sequence)` after catch-up.
- No duplicate `(session_id, source, remote_sequence)` rows exist.

```sql
SELECT session_id, source, remote_sequence, COUNT(*) AS copies
FROM events
WHERE source = 'intern' AND remote_sequence IS NOT NULL
GROUP BY session_id, source, remote_sequence
HAVING COUNT(*) > 1;
```

Expected: no rows.

Optional crash-window evidence: if the app is forcibly quit while a local run
still has an `accepted` receipt, restart intentionally fails that receipt and
marks the run `interrupted` with
`desktop_restart_reconciliation`. This is fail-closed behavior because staging
has no command-status lookup endpoint. Do not treat it as successful command
delivery.

## 5. Async ensure and objective

Only after the cardinality preflight passes:

1. Select **Background Intern**.
2. Submit a unique bounded objective such as
   `DOGFOOD-ASYNC-<timestamp>: produce one short checkpoint, then wait.`
3. Verify the leave-safe banner and Background Intern desk.

UI evidence:

- The first prompt is consumed by Async ensure and is not sent again through
  `/async/messages`.
- The desk shows the returned projection phase/cycle as events arrive.
- Closing the window does not send pause/cancel.

SQLite evidence:

- Latest Async session has the exact trimmed objective at
  `metadata_json.objective`.
- `remote_id` is the returned `async_runtime_id`.
- The projection contains both `async_runtime_id` and the deprecated
  `async_assignment_id`; the current Rust client requires equality if both are
  present.
- Source-cursor invariants match the Sync assertions.
- No local run exists until a later explicit message is sent.

## 6. Async checkpoint/control and quit/reopen reconciliation

1. Click **Checkpoint** and wait for its command receipt.
2. Optionally Pause then Resume.
3. Record Async cursor `A`, quit the app without cancelling the Background
   Intern, wait long enough for at least one remote event, and reopen.

Expected control evidence:

- `request_checkpoint`, `pause`, and `resume` appear as completed local command
  receipts with the full remote receipt in `response_json`.
- Projection status maps into the canonical local states: active work becomes
  `running`; `awaiting_input`/`paused` becomes `interrupted` (shown as paused by
  the wire model); completed/cancelled becomes `closed`.

Expected reopen evidence:

- The same local session and singleton `remote_id` return.
- The desktop does not call Async ensure during bootstrap. It reattaches the
  poller using the persisted source cursor.
- Cursor advances from `A` to the remote head without replay duplicates.
- Events generated while the desktop was closed appear once in the UI.
- With no active local run, restart adds no reconciliation failure.

Use the reusable queries with `mode = 'async'`, then inspect controls:

```sql
WITH chosen AS (
  SELECT id FROM sessions
  WHERE json_extract(target_json, '$.kind') = 'intern'
    AND json_extract(target_json, '$.mode') = 'async'
  ORDER BY created_at DESC LIMIT 1
)
SELECT cr.command_id, cr.kind, cr.status,
       json_extract(cr.response_json, '$.status') AS remote_receipt_status,
       json_extract(cr.response_json, '$.runtime_kind') AS runtime_kind,
       json_extract(cr.response_json, '$.runtime_id') AS runtime_id,
       json_extract(cr.response_json, '$.state_generation') AS state_generation
FROM chosen x JOIN command_receipts cr ON cr.session_id = x.id
ORDER BY cr.created_at;
```

## Staging contract checked read-only

- Sync create: `POST /smr/research-intern/sync-sessions`, required body
  `objective` (1..20000 chars) and `idempotency_key` (1..512), returns 202
  `SyncSessionResponse`.
- Sync command: `POST /smr/research-intern/sync-sessions/{id}/commands`, returns
  202 `InternRuntimeCommandReceipt`.
- Sync replay: `GET /smr/research-intern/runtimes/sync/{id}/events`, accepts
  `after_sequence >= 0`, `limit 1..500`, returns a 200 array of
  `InternRuntimeEventResponse`.
- Async ensure: `POST /smr/research-intern/async/ensure`, same required objective
  and idempotency key, returns 202 `AsyncRuntimeResponse`.
- Async message/control: `POST /async/messages` and `POST /async/commands`, each
  returns a 202 command receipt.
- Async replay: `GET /async/events` has the same cursor/limit contract.
- Both modes expose replay-then-tail SSE `/events/stream` routes accepting
  `after_sequence` or `Last-Event-ID`, but the desktop does not yet consume
  them.
- Event responses require stable event/runtime identity, sequence >= 1,
  state-generation fields, event kind, command ID, payload, and timestamp.
- Unauthenticated read-only probes of staging Async projection/events returned
  401, confirming the routes are protected. No authenticated request or remote
  mutation was made while preparing this checklist.
