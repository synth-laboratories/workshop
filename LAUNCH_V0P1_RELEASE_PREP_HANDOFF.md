# Workshop v0.1 final release-prep handoff

**Prepared:** 2026-08-10  
**Scope:** final QA, evidence collection, artifact qualification, and release decision  
**Product changes:** out of scope except fixes required by a failing launch gate  
**Launch contract:** [`launch_v0p1.md`](launch_v0p1.md)  
**Scenario catalog:** [`qa_cua_end_to_end.md`](qa_cua_end_to_end.md)  
**Gate implementation:** [`launch_gate_implementation_plan.md`](launch_gate_implementation_plan.md)

## 1. Outcome required

Qualify one exact signed Workshop v0.1 artifact against committed Workshop and evals revisions. The candidate is releasable only when the configured release lane emits `GREEN`, all 37 manual/CUA items carry valid evidence, and the public web/download smoke passes against the artifact that will be published.

A green unit suite, debug app, fixture workflow, or unbound manual checklist is not a release decision.

## 2. Current state

The gate framework exists at `/Users/joshuapurtell/Documents/GitHub/evals/workshop` and is fail-closed.

Implemented:

- Readiness policy, source-cleanliness, artifact identity/signature checks, and secret scanning.
- Playwright, Bombadil, Rust, static, and gate-harness suite execution with pass-count floors where runner output permits them.
- Independently graded coding task through Synth slot and local MLX paths.
- Craftax registration/catalog, two substantively different rollouts, and live visual creation.
- Evidence hashing/verification, JSON/Markdown receipts, interrupted-run receipts, and cleanup checks.
- A 37-item artifact/revision-bound manual receipt and 10,000-transition seeded state fuzzing.

Not yet proven or implemented completely:

- Trace V5 correlation in the live Container → Visual → Trace path. This remains an intentional hard blocker.
- A fresh successful provider-parity run through both the claimed Synth slot and local MLX sidecar.
- Full local frontend + Clerk development/E2E auth matrix.
- SSE interruption/resume, cross-session/cross-seed binding, and broader service fault injection.
- Signed/notarized installed-artifact qualification.
- Per-format artifact verification needs a final audit: the current checker runs `codesign --verify` and `stapler validate` against every accepted `.app`, `.dmg`, `.pkg`, or `.zip`. A customer-distributed archive may require verification of both the outer distribution file and the contained `.app`; do not weaken or bypass the check merely to accept a ZIP.
- Website funnel, performance/memory, GEPA visualization, and all other manual evidence.
- A complete 37/37 manual receipt reviewed by a non-implementer.

The 2026-08-10 unconfigured `gate:current` audit returned `RED_PRODUCT` with nine blocking inputs missing. That receipt proves fail-closed behavior only; it does not assess a configured live topology.

## 3. Product surface to qualify

The release candidate must match these current contracts:

- First run offers **Continue locally** and **Sign in to Synth**. There is no setup-agent modal or card.
- Launch picker targets are local Laguna XS 2.1, OpenRouter GPT 5.6 Luna, OpenRouter Laguna S 2.1, and Synth Cloud Laguna S 2.1.
- Intern is absent from every reachable v0.1 surface, capture, and claim.
- Containers use `ContainerPane`; visual artifacts and Trace V5 share `VisualPane`. There is no three-tab peer panel switcher.
- Sidebar navigation is Chats, Connectors, Research → Visuals/Optimizers, Inventory → Containers · Traces · Usage, and Settings.
- The Rust host owns Synth credentials; v0.1 stores the paired key in its desktop-managed `0600` env file. Do not claim OS-keychain storage.

## 4. Inputs that must be pinned before running

Record these in the release room before starting:

- Workshop Git revision and confirmation that its tree is clean.
- Evals Git revision and confirmation that its tree is clean.
- Local frontend revision, URL, and Clerk development/E2E environment.
- Claimed synth-dev slot ID, owner, health URL, provider/model, backend revision, and budget.
- Named Workshop instance and eval-driver descriptor.
- MLX health URL, bearer configuration, exact Laguna model revision, and supported Mac hardware.
- Craftax base/health URL, service revision, catalog version, and rollout budget.
- Exact packaged artifact path, version/build/channel, and SHA-256.
- Named release driver, CUA tester, independent reviewer, no-go authority, and incident channel.

Do not let the gate claim, restart, reconcile, deploy, or clean a shared slot implicitly. Preflight is read-only.

## 5. Final execution sequence

### Step 1 — freeze and clean sources

1. Stop product/content edits or record the final cutoff.
2. Commit the exact Workshop and eval-gate revisions to qualify.
3. Confirm both trees are clean and revision-resolvable.
4. Pin the web, slot, MLX, and Craftax identities listed above.

Any subsequent product, gate, site, documentation, or artifact change invalidates the candidate evidence and requires the affected lanes to rerun.

### Step 2 — verify the gate harness

```bash
npm --prefix /Users/joshuapurtell/Documents/GitHub/evals/workshop test
npm --prefix /Users/joshuapurtell/Documents/GitHub/evals/workshop run typecheck
npm --prefix /Users/joshuapurtell/Documents/GitHub/evals/workshop run gate:negative-control
```

Expected:

- Gate tests pass, including the red-by-construction coding fixture and 10,000-transition state fuzzing.
- `NEGATIVE-CONTROL-SECRET` passes by detecting every canary class. Other missing-topology/manual checks in that composite command do not invalidate the scanner result, but must not be mistaken for a green release.

### Step 3 — run deterministic product verification

From `/Users/joshuapurtell/Documents/GitHub/workshop`:

```bash
npm run desktop:check
npm run desktop:verify
npm run desktop:build
```

Then run the consolidated deterministic gate:

```bash
npm --prefix /Users/joshuapurtell/Documents/GitHub/evals/workshop run gate:pr
```

Do not waive expected-fail, skip, fixme, or todo markers. `PRODUCT-NO-XFAIL` must pass.

### Step 4 — preflight the live topology

```bash
npm --prefix /Users/joshuapurtell/Documents/GitHub/evals/workshop run gate:preflight -- \
  --slot <slot-id> \
  --synth-dev-root /absolute/path/to/synth-dev \
  --instance <workshop-instance> \
  --frontend-url http://127.0.0.1:<frontend-port> \
  --slot-health-url http://127.0.0.1:<slot-port>/health \
  --mlx-health-url http://127.0.0.1:<mlx-port>/health \
  --craftax-url http://127.0.0.1:8098/health
```

Block immediately on a foreign slot claim, dirty/degraded slot contract, wrong frontend/auth target, missing MLX bearer boundary, mismatched Workshop descriptor, unhealthy Craftax, or unresolved identity.

### Step 5 — close the known gate blockers

Before expecting Gate B to turn green, implement and prove the Trace V5 correlation check currently hardcoded red in the live gate. It must correlate at least one real Craftax observation, action, reward, frame, and model/tool event, with matching rollout/seed/run identity.

Add a deterministic regression where practical. Do not replace the blocker with a constant pass or fixture-only proof.

Before Gate C, audit `RELEASE-ARTIFACT-SIGNATURE` against the actual distribution format. The release gate must verify the customer-delivered outer artifact and, when applicable, the contained `.app` signature/notarization/Gatekeeper behavior. If the current all-tools-on-one-path check cannot correctly assess that format, fix the gate and add a regression before qualification.

### Step 6 — run the configured local integration lane

```bash
npm --prefix /Users/joshuapurtell/Documents/GitHub/evals/workshop run gate:local -- \
  --slot <slot-id> \
  --synth-dev-root /absolute/path/to/synth-dev \
  --instance <workshop-instance> \
  --frontend-url http://127.0.0.1:<frontend-port> \
  --slot-health-url http://127.0.0.1:<slot-port>/health \
  --mlx-health-url http://127.0.0.1:<mlx-port>/health \
  --craftax-url http://127.0.0.1:8098
```

Required evidence:

- Independently graded coding task succeeds through Synth slot and local MLX.
- Session chronology, terminal status, provider/model identity, file changes, commands, export, and reopen are correct.
- Craftax catalog and two real rollouts pass, frames differ substantively by seed, live visual opens, and Trace V5 correlation passes.
- Created sessions/containers are cleaned up with exact IDs and no shared resource is disturbed.

### Step 7 — build and inspect the release artifact

```bash
npm run desktop:install:release
shasum -a 256 /absolute/path/to/<customer-release-artifact>
```

Record the actual packaging command and artifact location if the release pipeline produces a `.dmg`, `.app`, or other filename instead. Verify version, icon, code signature, Gatekeeper assessment, notarization/stapling, and the absence of a release eval-driver port/descriptor.

### Step 8 — create and execute the manual/CUA receipt

```bash
npm --prefix /Users/joshuapurtell/Documents/GitHub/evals/workshop run gate:manual:init -- \
  --out /absolute/path/to/manual-gate.json \
  --artifact-sha <artifact-sha256> \
  --workshop-revision <workshop-git-revision>
```

Execute all 37 items in `/Users/joshuapurtell/Documents/GitHub/evals/workshop/manual/CUA_MANUAL_GATE.md` against the exact installed artifact. Every passing item needs:

- `status: "pass"`
- tester identity
- ISO timestamp
- at least one existing absolute evidence-file path or HTTP(S) URL

Required golden journeys include:

- Public usesynth.ai → signup/download → installed artifact → pairing → first Synth response.
- Continue locally → On-device models → Laguna download/load → real task → cancel/recover/unload.
- Agent → Container inspector → real Craftax rollout → live visual → Trace V5 correlation → export/reopen.
- Real GEPA import/lineage/comparison and honest GELO/SFT `[alpha]` behavior.
- Intern absence, signing/Gatekeeper, responsive/keyboard polish, security/redaction, upgrade, and performance/memory.

### Step 9 — run the exact release gate

Run within 24 hours of publication:

```bash
npm --prefix /Users/joshuapurtell/Documents/GitHub/evals/workshop run gate:release -- \
  --slot <slot-id> \
  --synth-dev-root /absolute/path/to/synth-dev \
  --instance <workshop-instance> \
  --frontend-url http://127.0.0.1:<frontend-port> \
  --slot-health-url http://127.0.0.1:<slot-port>/health \
  --mlx-health-url http://127.0.0.1:<mlx-port>/health \
  --craftax-url http://127.0.0.1:8098 \
  --artifact /absolute/path/to/<customer-release-artifact> \
  --manual /absolute/path/to/manual-gate.json
```

Then independently revalidate the receipt and evidence hashes:

```bash
npm --prefix /Users/joshuapurtell/Documents/GitHub/evals/workshop run gate:verify -- \
  --receipt /absolute/path/to/results/<run-id>/gate-receipt.json
```

Only `GREEN` on both commands qualifies the artifact.

### Step 10 — production smoke and publish

Production Clerk does not support the development `+clerk_test`/`424242` path. Use the standing real test mailbox.

Before publish:

- Verify production device-init returns JSON rather than redirecting.
- Pair the published artifact, confirm account state, and perform one capped cloud action.
- Confirm the local-only path still works without authentication.
- Verify download version, filename, SHA-256, signing/notarization copy, release notes, requirements, docs, pricing/privacy/support, blog, videos, and social metadata.
- Confirm usesynth.ai contains no Intern launch claim and no removed setup-modal footage or instructions.

After publish, repeat the thin production smoke against the promoted download and retain evidence.

## 6. Hard no-go conditions

Do not ship if any of these is true:

- Release or verification receipt is not `GREEN`.
- Artifact, Workshop revision, evals revision, or public download differs from the qualified identity.
- Either source tree is dirty or unresolvable during the release run.
- Trace V5 correlation remains hardcoded, absent, fixture-only, or cross-bound.
- Real Synth-slot or local-MLX coding task fails independent grading.
- First run, pairing, first response, restart/reopen, or local-only use cannot complete.
- Craftax frames/events/traces do not match the selected rollout and seed.
- Cancellation creates false success, orphan compute, or duplicate paid/destructive work.
- A secret, credential, workspace path, staging host, or private data leaks into evidence or UI.
- Composer or primary controls clip/overlap, focus traps, or the installed app is materially unusable.
- Signing, notarization, Gatekeeper, checksum, or download identity fails.
- Any required manual item is pending, stale, unbound, or lacks verifiable evidence.
- Marketing, docs, blog, or video claims functionality the artifact does not expose.

Infrastructure failure is not a product pass. Fix the topology and rerun; never convert `RED_INFRA` or `RED_HARNESS` into a waiver.

## 7. Release-room evidence index

The final handoff packet must contain:

- Exact Workshop, evals, frontend, backend/slot, MLX/model, and Craftax identities.
- Exact artifact path/URL, version/build, SHA-256, signature, Gatekeeper, and notarization results.
- `gate:pr`, configured preflight, `gate:local`, `gate:release`, and `gate:verify` logs/receipts.
- Completed 37-item manual receipt plus screenshots/videos and hashed local evidence.
- Provider-parity workspaces/results, Craftax rollout IDs/seeds/frames, Trace V5 evidence, and exports.
- Local model hardware, load time, TTFT/TPS, memory/load/unload/recovery results.
- Web funnel, pairing, download, blog/video/docs, mobile/desktop, and production-smoke evidence.
- Known limitations, release notes, rollback artifact/path, incident owner, and first-hour monitoring plan.

## 8. Operator sign-off

| Role | Name | Decision/time | Evidence pointer |
|---|---|---|---|
| Release driver | TBD |  |  |
| Desktop/artifact owner | TBD |  |  |
| Web/auth owner | TBD |  |  |
| Slot/API owner | TBD |  |  |
| Laguna owner | TBD |  |  |
| Evals/Craftax owner | TBD |  |  |
| CUA tester | TBD |  |  |
| Independent reviewer | TBD |  |  |
| No-go authority | TBD |  |  |

Final decision: **NO-GO until the exact release artifact has a fresh verified `GREEN` receipt and 37/37 evidence-backed manual checks.**
