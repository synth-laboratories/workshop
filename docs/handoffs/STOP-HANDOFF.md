# STOP HANDOFF — Workshop cloud foundations

Stopped on September 11, 2026 at the user's explicit direction. This is a stop
record, not a completion or live-readiness claim. Do not resume automatically.
The stop instruction supersedes all earlier keep-working instructions.

## Exact repository state

- Repository: `/Users/joshuapurtell/GitHub/workshop`
- Shared Git directory: `/Users/joshuapurtell/GitHub/workshop/.git`
- Worktree: `/Users/joshuapurtell/GitHub/worktrees/workshop-cloud-foundations-20260911`
- Branch: `codex/workshop-cloud-foundations-20260911`
- HEAD: `04bf4b657761e90a27db1fdacdf0d4640fd54ad0`
- Original base: `1e17bf7006a1260df1ae5d1dcd0ec6d4c233b9ad`
- Tracked and untracked status was clean immediately before writing this file.
  This handoff is the only newly uncommitted file; no implementation changes were
  left uncommitted. Ignored logs and `.test-tmp` fixtures remain preserved.
- No reset, deletion, cherry-pick, push, merge, deployment, or new implementation
  commit was performed after the stop instruction. Other owners' worktrees were
  not modified.

## Implemented source; qualification is separate

Earlier commits introduced lazy cloud initialization, the private Rust SDK,
durable scoped storage and outbox/creation intent candidates, finite identity
observations, gated scope/history IPC, conservative settlement presentation,
and a read-only execution-location component. Production profile activation and
desktop migration registration remain gated. Local fixture work is documented
in the historical handoff and must not be represented as real-model acceptance.

Recent exact commits:

- `c6338f46316cf01fce4aefa2ba8b9f7084a4e369`: gated scope/history and fresh
  settlement reads.
- `fe9499c7cf9be55fa4556668a39ec9dffbea05b5`: scoped creation/first-send
  composition, deferred transport construction, and cancellation of superseded
  identity verification. Includes strict settlement proof consumers.
- `3870eff5dc625d46c9b534fd759555e32b5c0dd8`: scoped-owned rows rejected in
  legacy send/control/provider-start, skipped on resume, excluded from Async reuse.
- `ce638ee2a907be2f92bfef63a365f94693845bbd`: direct Async reuse regressions.
- HEAD `04bf4b657761e90a27db1fdacdf0d4640fd54ad0`: the newly approved
  historical account-transition policy. Historical rows remain readable but have
  no remote admission after restart or replacement. Fresh successful creation
  supplies only volatile provenance for the exact uninterrupted client epoch.
  Metadata, credential equality, and stored remote IDs do not establish ownership.
  Replacement cancels pending operations, preserves unknown outcomes, and stops
  historical poller resume/Async reuse. Failed configuration or persistence does
  not restore admission. No historical ownership-verification/rebind API exists.

HEAD also fixes review finding WE-01 with a second invalidation under the
configuration write lock. Its regression models a creation capturing the old
client after early invalidation but before writer acquisition, and requires
cancellation and late-admission refusal for both disable and replacement.

## Passing, failing, and unrun evidence

All of the following completed locally against the frozen source committed as
HEAD. The final command session `88024` was confirmed exited with code 0 after
the stop instruction; that confirmation did not start another test.

| Check | Result | Log under `artifacts/cloud-foundations/` |
| --- | --- | --- |
| CoreRuntime native tests | 6 passed, 0 failed | `legacy-transition-core-reviewed.log` |
| Intern API native tests | 13 passed, 0 failed | `legacy-transition-api-reviewed.log` |
| Cloud native tests | 76 passed, 0 failed | `legacy-transition-cloud-reviewed.log` |
| Non-test native library check | passed; 290 existing warnings | `legacy-transition-library-reviewed.log` |

Source and log SHA-256 values are recorded in
[HEAD source evidence](/Users/joshuapurtell/GitHub/worktrees/workshop-cloud-foundations-20260911/artifacts/cloud-foundations/legacy-account-transition-evidence.json).
These are 95 local native tests, not live provider or deployment qualification.

Earlier independently verified checkpoints:

- Integration evidence `b5b6a5e` closed WD-01/WD-02 offline for `fe9499c7`:
  13 scoped-runtime, 4 CoreRuntime, 14 renderer tests, and typecheck passed.
- Integration evidence `a118122`, integrated desktop `8a3952e8`, verified
  `3870eff5`: 5 CoreRuntime and 8 Intern API tests passed; scoped routing closed
  offline. This preceded the newly approved historical policy.
- `ce638ee2` passed 5 local Async tests. Independent verification was requested;
  this handoff does not assert a final independently verified result for it.
- Earlier broader frontend/SDK checks and hashes are in
  [dispatch evidence](/Users/joshuapurtell/GitHub/worktrees/workshop-cloud-foundations-20260911/artifacts/cloud-foundations/dispatch-composition-evidence.json).
  Do not treat those older revisions as verification of HEAD's new policy.

No current local test failure was observed for HEAD. Historical initial compile
failures from incorrect fixture imports/types were corrected; their logs remain
historical, not current failures. Known unrelated baseline limitations remain:
the full contract suite has the optimizer default version `0.2.19` versus
contract `0.2.20` discrepancy, and aggregate CSS lint has existing debt. Earlier
focused contract tests explicitly excluded the optimizer-version assertion.
These issues were not changed or claimed fixed.

HEAD's independent integration review/test request was sent immediately before
the stop instruction. A stop message was then delivered to integration. Its
final verification/WE-01 closure is **unfinished/unconfirmed here**. Do not infer
closure from local passing tests or the preliminary source review.

No live Cloud activation, real provider run, production deployment, PostgreSQL
qualification, applied live migration, or full application acceptance was run
in this owner lane for HEAD. The location-component check was a widget fixture,
not packaged application acceptance. QA/integration own database qualification;
the latest integration report described manager disk admission blocking startup
and all four PostgreSQL cases unrun. Verify their stop handoff for final cleanup
state; this lane did not claim or operate their resources.

## Process and resource stop status

- No running owner build/test remained: final session `88024` exited 0.
- A process snapshot found no matching cargo test/check, native test binary, or
  old execution-location fixture HTTP server (only the inspection command itself).
- The earlier temporary widget browser tab and HTTP server on port 17642 had
  already been closed/stopped. Earlier packaged Local app/daemon fixtures had
  already been stopped.
- This lane owns no live disposable QA slot, database, provider, or cloud resource.
  No cleanup was performed on another lane's resources.
- Integration was explicitly told to stop the newly dispatched review/testing,
  preserve changes, and follow the parent stop instruction. Delivery succeeded;
  this file does not claim an unobserved cancellation acknowledgment.
- All source, evidence, ignored logs, and fixtures are retained. No further
  implementation, tests, reviews, or monitoring will be started by this task.

## Outstanding scope and resumption instructions

The historical account policy is now approved; its implementation is local and
tested but still needs independent integration review. Broader product acceptance
is unfinished. Public ownership verification/rebind, qualified profile admission,
desktop migration registration, full UI/native acceptance, and end-to-end SDK
resource/accounting evidence remain separate work. Database qualification and
the fake pinned harness belong to their assigned QA/cloud owners; do not duplicate
them. PostgreSQL disk admission remains an external blocker in the latest report.

Only after an explicit user instruction to resume:

1. Read this file and the coordinating task's final stop handoff. Inspect current
   Git status before changing anything; preserve this handoff and all other work.
2. Reconfirm HEAD and compare the source hashes in the HEAD evidence. Retrieve
   integration's final stop state before restarting any review or build.
3. Finish independent review/qualification of `04bf4b65`, especially WE-01,
   replacement failures, cancellation-owned requests, historical Async refusal,
   restart receipts, and the limited same-epoch creation allowance. Run only
   checks justified by outstanding review or changed source; current local logs
   already record the passing suite above.
4. Coordinate remaining QA/cloud tasks through the parent. Do not restart services,
   migrations, paid runs, or deployment merely because source tests pass. Keep
   credentials in authorized mechanisms; do not use Keychain.
5. Preserve unknown remote outcomes and historical history. Never infer ownership
   or silently rebind old conversations to a new account.

Detailed historical context:
[owner handoff](/Users/joshuapurtell/GitHub/worktrees/workshop-cloud-foundations-20260911/docs/handoffs/2026-09-11-workshop-cloud-foundations.md),
[integration policy review](/Users/joshuapurtell/GitHub/worktrees/workshop-v011-integration-qa-20260911/docs/internal/workshop-v011-integration-20260911/LEGACY-EPOCH-POLICY-REVIEW.md).

Coordination IDs: parent `01a09132-34bc-7010-b309-26c341127bf3`; integration
`01a09242-89e8-76c2-be25-f24339cafab3`; cloud owner
`01a091c5-9b09-75f3-ad94-2e3d7be81763`; this owner
`01a091c5-9b09-75f3-ad94-2e5fdb6295a3`.
