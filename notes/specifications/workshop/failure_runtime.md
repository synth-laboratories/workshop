# Failure runtime

Governing specification for Workshop's single failure lifecycle and app-local structured log system.

Modules under `apps/synth_desktop/src-tauri/src/platform/`, `domains/`, `adapters/`, and `composition/` implement this document. Renderer and visual surfaces consume only `FailureView`.

## Authority

1. Domain authorities decide what failed and how canonical state changes.
2. `FailureRuntime` owns identity, lifecycle, persistence, causality, query, redaction, and delivery.
3. Logs are correlated supporting observations, never an alternate failure authority.
4. Tauri, MCP, Codex, visuals, chat, and renderer receive typed projections of the same durable failure.
5. Failure creation and domain-state consequences share one SQLite transaction.
6. Missing telemetry remains unavailable, never zero.
7. Recovery is an explicit plan with approval, idempotency, bounds, and a receipt.

## Non-goals

- Fallback classification, prose parsing, or `.get()` shape probing in core control flow.
- `From<String>` paths that turn prose into a domain failure.
- Renderer or visual classification policy.
- Logging and returning the same failure at every stack layer.
- Success-shaped responses from error handlers.
- Permanent legacy readers or dual-write paths.
- Fixtures, fake containers, scripted rollouts, fabricated receipts, canned streams, or simulated provider proof.
- macOS Keychain access from this subsystem.

## Domain hierarchy

One parent, domain-owned children:

```text
FailureKind =
  Admission | Approval | Authentication | Container | Evaluation |
  Persistence | Provider | Session | Telemetry | Visual | Contract
```

Every child implements `FailureDefinition`:

- `code` — stable snake_case wire spelling
- `category` — domain grouping
- `disposition` — `approval_required | repair_required | retryable | terminal | cancelled | programmer_error`
- `remediation` — typed action, not prose advice
- `state_effect` — what canonical records must change when this failure is raised

Adding a variant fails compilation until those five are exhaustive.

`AdmissionError` maps onto `FailureKind::Admission`. Container health, protocol, and selection failures that admission currently owns remain admission-coded when raised during admission, and container-coded when raised by the container authority.

## Lifecycle

Disposition and lifecycle are different.

```text
Open
  → AwaitingApproval → Repairing → Resolved
  → RetryScheduled → Retrying → Resolved | Open
  → Terminalized
  → Superseded
```

Illegal transitions are rejected by the lifecycle machine, not coerced.

- Terminal lifecycle never returns to live.
- Resolved requires a `resolved` transition with actor and reason.
- Approval denial writes a `denied` transition and leaves the operation blocked.
- Retries are lifecycle transitions or linked occurrences (`RetryOf`), not unrelated log lines.

## Operation context

Entry-point services receive `OperationContext`. Correlation is not reconstructed after failure. An operation record is inserted before work that can fail. `operation_id` is mandatory on log records when an operation exists.

## Causality

Exact relationships when known:

```text
CausedBy | ConsequenceOf | Supersedes | RepairOf | RetryOf
```

Diagnostic rank is not a substitute for a known relationship.

## Recovery

```text
FailureDefinition → RecoveryPlan → ApprovalRequest → RecoveryExecution → RecoveryReceipt
```

A failure object never contains an executable callback. Restarting a container, resuming a session, or reconnecting a stream is a `RecoveryAction` with:

- `recovery_id`
- `failure_id`
- `approval_requirement`
- `idempotency_key`
- `bounds`

Clickable approvals are generated from `FailureRemediation::Approve`, never inferred from chat text.

## Persistence

Authoritative SQLite tables:

| table | role |
|---|---|
| `operation_records` | in-flight and completed operations |
| `failure_occurrences` | durable failures |
| `failure_transitions` | immutable sequence-numbered lifecycle history |
| `failure_relationships` | typed causality |
| `log_records` | structured observations |
| `recovery_plans` | intended repair |
| `recovery_receipts` | executed repair |

Indexed foreign keys `current_failure_id` / `terminal_failure_id` hang off containers, runs, rollouts, visuals, and experiments. Copied error strings and JSON are not written.

Historical rows that cannot be classified become `historical_failure_unclassified`. There is no runtime heuristic reader for the old columns.

### Unit of work

```text
BEGIN IMMEDIATE
  insert operation (if new)
  insert failure occurrence
  insert failure.raised transition
  domain settlement port (state + FK)
  optional log record
COMMIT
```

The generic runtime contains no container- or evaluation-specific SQL. Domain authorities implement settlement ports.

### Invariants

- Terminal run implies `finished_at` and a terminal manifest.
- Failed terminal run implies `terminal_failure_id`.
- Terminal parent implies no child remains queued, starting, or running.
- Failed visual implies `current_failure_id`.
- Unhealthy container has a typed health observation and failure reference unless deliberately stopped.
- Resolved failure has a resolution transition.
- Unknown stored state is rejected, not treated as running.
- Missing telemetry remains unavailable.

## Logs

SQLite is authority. VictoriaLogs remains a disposable search index over diagnostics, not over the failure ledger.

Retention defaults, written to instance `observability.toml`:

| class | retention |
|---|---|
| unresolved failures | until resolved |
| terminal error failures | 90 days |
| warnings | 30 days |
| informational logs | 7 days |
| debug logs | 24 hours |
| total local log ceiling | 250 MB / instance |

Trimming produces a retention receipt. Secrets, headers, environment credential values, and Keychain material never enter logs or bundles.

`eprintln!` is forbidden except inside `platform/logging/emergency_sink.rs`.

## Bootstrap / emergency mode

```text
ObservabilityMode = Durable | EmergencyFile
```

Before SQLite is available, bounded redacted NDJSON is written to `{data_root}/logs/emergency.ndjson`. On recovery the file is imported in one transaction, an import receipt is recorded, the file is rotated, and the degraded interval is visible on the Errors & Logs surface. This is an explicit mode, never a silent fallback.

## Boundary contract

The only renderer/MCP/visual envelope is `FailureView` (`synth.failure-view.v1`). Malformed envelopes fail as `failure_contract_invalid`. Arbitrary transport prose is not shown.

## Shutdown

In-flight operations are either awaited, cancelled and awaited, or supervised with durable completion. A process exit mid-turn reconciles as `SessionFailure::Detached` with a restartable recovery plan. Background tasks must not log-and-forget a failure.

## First vertical slices

1. Container probe, health-authority conflict, restart approval, recovery execution, state settlement, Tauri/MCP result, error card, agent response, logs, diagnostics.
2. Evaluation admission (`FailureKind::Admission`) through terminal child reconciliation.
3. Session detach / reattach / resume.

Each slice has one writer. Dual-write is forbidden.
