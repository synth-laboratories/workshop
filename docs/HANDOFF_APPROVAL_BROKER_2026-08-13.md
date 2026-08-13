# Handoff — typed approval broker (v0.3)

**Status:** design scoped, not started. First pass then review.
**Scope:** v0.3. Not GO for v0.2 — do not mix into SYN-3202 / SYN-3215 or any release commit.
**Baseline:** workshop `josh/aug12-optimizers-workshop-visuals` @ `7abed18`. Re-anchor line numbers before editing; a concurrent agent commits into this tree.

---

## Why

We have a shell-approval relay and we are about to need approvals for things that are not shell commands. The optimizer redesign on `agent/aug12-modern-stack-completion` (`8a6aca6`) removed the recipe UI and with it the two consent checkboxes that gated paid compute — "I approve both bounded runs and their API usage". Consent moved into an agent conversation. That means **the surface that spends money currently has no typed gate on the mutation itself**, only prose assent in a chat turn.

Adding a `kind` field to the existing approval envelope does not fix this, for a structural reason described next.

## The constraint that decides the design

**Approvals today are not an approval system. They are a Codex elicitation relay.**

- Exactly one emitter: `session/codex/event_pump.rs:246` — `notify_codex_event(..., "approval.requested", safe)`.
- The pending record is `PendingApproval { rpc_id, available_decisions }` (`event_pump.rs:235-241`). The approval id is a handle on **an outstanding JSON-RPC request to the external Codex binary**.
- Resolution writes a JSON-RPC result back on Codex's stdin: `proto.rs:297-317` `perform_resolve_approval`.
- The decision vocabulary is **Codex's, not ours**. `select_approval_decision` (`proto.rs:~358`) translates our `"once"` into whichever of `["accept","approve","allow","yes"]` the peer advertised in `available_decisions`.
- The renderer-facing command is `codex_approval_resolve` (`lib.rs:1866`).
- `approval_policy` (`synth_config.rs:94,101,362-376`) is a **Codex config string** we write into Codex's config. There is no Workshop-owned approval policy.

A Workshop-originated approval — start a paid optimizer run, install a sidecar — has no `rpc_id`, no peer, and no peer-advertised decision list. The resolution mechanism is coupled to Codex by construction.

There is already a seam, but it is at the wrong level. `ProviderTransport` (`proto.rs:~325-345`) abstracts *which app-server*, and its own doc comment calls it "the extension point … not a product noun". It still assumes a JSON-RPC peer. We need a seam one level up.

## Target architecture

Separate the two things that are currently one object:

```text
                    ┌─────────────────────────────┐
   request()  ───▶  │      ApprovalBroker         │  owns: pending set, durability,
                    │  (Workshop, origin-neutral) │  restore, expiry, policy, safe payload
                    └──────────┬──────────────────┘
                               │ decision
              ┌────────────────┼────────────────────┐
              ▼                ▼                    ▼
      CodexResolver     OptimizerResolver     SidecarResolver
   (answer rpc_id on    (proceed/abort a      (proceed/abort a
    Codex stdin)         local mutation)       local mutation)
```

- **Broker** owns everything the card and the journal care about: the pending set, persist-before-publish, restore after restart, expiry on origin death, per-type policy, and the redacted payload. Knows nothing about Codex.
- **Resolver** is a small trait: given an approval id and a decision, deliver it. Codex's implementation is today's `perform_resolve_approval` moved behind it. Workshop-originated kinds resolve by unblocking a local `oneshot`, not by writing to a pipe.
- **Kind** is a typed request enum with a kind-specific payload and a kind-specific decision set.

`ProviderTransport::resolve_approval` should be **removed** from that trait once the broker exists — the transport should not know about approvals at all.

## Fix these four first — they are the reason to sequence this

The core has four known defects. Typed variants multiply every one of them, so a first pass that generalizes before repairing produces four broken kinds instead of one.

| # | Defect | Where |
| --- | --- | --- |
| 1 | The in-memory `approvals` map is **never drained** on interrupt, turn exit, or process exit. The JSON-RPC peer is left hanging and the map grows for the process lifetime. | `event_pump.rs:127`, `:235` |
| 2 | After a restart, `sessions` is empty, so resolution cannot find the session — **the card renders with dead buttons**. | `session/codex/manager.rs:778-786` |
| 3 | `if (!running) return []` strips approval lines from every non-running render path, so a **restored pending approval is invisible**. | `components/ChatTranscript.tsx:541` |
| 4 | The Bombadil approval spec **never clicks Approve or Reject** — it asserts `approveOnce` text, `cardAboveWorking` geometry, and no horizontal overflow. `window.synthCodexOauth` is absent for that spec, the fixture provider is `local-laguna`, and the stub has no `resolveApproval`. | `tests/bombadil/approval-card.spec.ts:25-26,:46-47`, `tests/bombadil/run.mjs:309-329` |

Defect 1 is also the one that most clearly *needs* the typed lifecycle: "what happens to a pending approval when the thing that requested it dies" has a different right answer per kind, and today it has no answer at all.

Note 2 and 3 are coupled — `!running` is simultaneously the bug and the mechanism implementing "stale approval cards hide after the turn stops". Do not just delete the guard. Fix it at the source: on interrupt or turn exit, the broker emits a terminal `approval.expired` for each still-pending approval, and the renderer stops inferring staleness from run status. `v02-approval-ux.spec.ts:116` currently pins the buggy behavior and must be edited in the same change.

**That spec is tagged `[v0.2]`**, so it runs in the G3 deterministic slice (`v02-e2e-gates.sh` greps `-g '\[v0\.2\]'`). Fixing defects 2 and 3 therefore edits a v0.2 release gate. This is a further reason the work is v0.3 and must not land before GO: it cannot be done quietly, and doing it mid-release turns a gate red for reasons unrelated to the release.

## Kinds and decision vocabularies

The vocabularies genuinely differ. That is the argument for typing rather than reusing Reject / Approve-once.

| Kind | Origin | Decisions | Rememberable |
| --- | --- | --- | --- |
| `shell_command` | Codex | reject · once · always-this-session · always-this-workspace | yes — already is |
| `paid_compute` | Workshop (optimizer start) | reject · approve-with-cap | **no** — or only under an explicit cap that the decision itself carries |
| `sidecar_lifecycle` | Workshop (install/start/stop/uninstall) | reject · approve | yes |
| `credential_access` | Workshop (broker/OAuth) | reject · once | **never** |

**The remember column is the safety-critical part of this whole design.** Persistent approval preferences already exist for shell. If `paid_compute` inherits a blanket "remember this", one remembered yes becomes unbounded spend. That is the same failure mode as the GEPA budget gate counting unknown cost as `$0` — and two independent paths to unbounded paid work is one too many.

Make the policy a property of the kind, enforced in the broker, not a per-call flag a caller can pass. A caller must not be able to request "remember" for a kind that forbids it.

`paid_compute` decisions should carry the cap they were granted under, so the approval is falsifiable after the fact: a run that exceeded its approved cap is a receipt violation, not a judgment call.

## Migration

Do not big-bang it. Codex approvals keep working throughout.

1. Introduce the broker alongside the existing path. Codex still emits, but through `broker.request(ApprovalKind::ShellCommand{..})`, and `CodexResolver` wraps today's `perform_resolve_approval` unchanged.
2. Move durability, expiry, and restore into the broker. Fix defects 1–3 there. Behavior for shell is identical; the tests from defect 4 are what prove it.
3. Add `paid_compute` with `OptimizerResolver`. Wire it at optimizer run start, so the mutation blocks on the decision.
4. Add `sidecar_lifecycle` and `credential_access`.
5. Remove `resolve_approval` from `ProviderTransport`. Rename the renderer command off `codex_*` — note this touches `contract/commands.rs`, `collect_commands!`, `protocolConstants.ts`, and regenerates `generated/protocol.ts`, so coordinate with whoever owns the contract-drift lane.

Step 5 is deliberately last and is not first-pass scope.

## First pass — what to build

**In scope:**

- The `ApprovalBroker` and `ApprovalResolver` trait, with `CodexResolver` as the only implementation.
- Typed `ApprovalKind` / `ApprovalDecision` with the four kinds declared, even though only `shell_command` is wired.
- Per-kind remember policy enforced in the broker, with a test that a forbidden kind cannot be remembered.
- Defects 1, 2, 3 fixed in the broker, plus the `approval.expired` event.
- Defect 4: one Bombadil spec that actually clicks Approve and asserts the resolver was called with `(sessionId, approvalId, decision)`, plus the negative control that fails if the OAuth/permissions bridge is missing rather than silently exercising the disconnected branch.
- Kind-specific safe payloads. `safe_approval_payload` (`event_pump.rs:529`) is the existing pattern; `paid_compute` shows cost and cap, `credential_access` never shows the credential.

**Out of scope for the first pass:** wiring `paid_compute` into the optimizer, the `codex_*` command rename, and any change to Codex's own `approval_policy` config.

## Done means

- Shell approvals behave identically to today, proven by a spec that resolves rather than one that measures pixels.
- Killing the app with an approval pending, restarting, and reopening the session shows the card **and the buttons work** — or shows it resolved as expired. Never a live-looking card with dead buttons.
- A pending approval whose origin dies is terminalized, not orphaned. The `approvals` map returns to empty; assert it.
- A test asserts that `paid_compute` cannot be remembered, and that a decision carries its cap.
- `grep -rn "approval" ` over the transport layer shows the transport no longer owns approval semantics (or a written note on why step 5 is deferred).

## Open questions for review — decide, don't guess

1. **Does `paid_compute` gate the MCP `start_recipe` call, the optimizer mutation, or both?** Gating the mutation is safer (an agent cannot route around it) but means the approval fires below the tool call, which changes what the agent sees on rejection.
2. **What is the cap unit** — dollars, rollouts, or both? The GEPA cost lane is concurrently making cost nullable, so a dollar cap must define its behavior when cost is unknown. Suggest: unknown cost blocks a capped run rather than counting as `$0`, consistent with the `UnknownCost` budget state in the v0.2 cost work.
3. **Is "always-this-workspace" still correct for shell** once the broker owns policy, or should it become a workspace-scoped preference rather than an approval decision?
4. **Do restored-but-expired approvals stay visible in the transcript** as resolved history, or disappear? Affects whether `approval.expired` is a journaled event or a local state change.
5. **Who owns approval for agent-initiated container mutations** (`container_start_prepared_rollout`)? Not listed above; it spends compute too.

## Files

| Path | Role |
| --- | --- |
| `apps/synth_desktop/src-tauri/src/session/codex/event_pump.rs` | `:127` map, `:235` insert, `:246` emit, `:516` auto-approve, `:529` safe payload |
| `apps/synth_desktop/src-tauri/src/session/codex/proto.rs` | `:297` resolve, `:~333` `ProviderTransport`, `:~358` decision translation |
| `apps/synth_desktop/src-tauri/src/session/codex/manager.rs` | `:778-786` resolve entry, `:799` event notify |
| `apps/synth_desktop/src-tauri/src/lib.rs` | `:1866` `codex_approval_resolve` command |
| `apps/synth_desktop/src-tauri/src/synth_config.rs` | `:94,:101,:362-376` Codex `approval_policy` |
| `apps/synth_desktop/src/renderer/src/components/ChatTranscript.tsx` | `:541` the `!running` guard, `:127-136` card |
| `apps/synth_desktop/src/renderer/src/runtime/sessionView.ts` | `:1001` resolve-id field matching, `:1314` `approvalId` on the activity line |
| `apps/synth_desktop/tests/bombadil/{run.mjs,approval-card.spec.ts}` | stubs and the spec to make fail-closed |
| `apps/synth_desktop/tests/playwright/v02-approval-ux.spec.ts` | `:116` pins the current staleness bug — **`[v0.2]`-tagged, runs in the G3 gate** |

## Do not

- Do not infer approval staleness from run status.
- Do not let a caller pass a "remember" flag for a kind whose policy forbids it.
- Do not surface a credential or a token in an approval payload; extend `safe_approval_payload` per kind.
- Do not add explanatory prose to the approval card. It is label-only today and must stay that way. The two first-person paid-compute consent labels are the exception and are correct — reuse that phrasing when `paid_compute` lands.
- Do not mix this into any v0.2 release commit.
