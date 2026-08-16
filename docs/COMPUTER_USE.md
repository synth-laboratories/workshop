# Computer Use — an optional Workshop plugin

**Status:** Implementation contract. §5 and §7 are normative; §3 is the grading sheet.

**Target release: v0.5.** Branch `v0.5/cua`, cut from `origin/v0.5/implementation`
@ `ec4fae7b6465f9770e8ddac2097a53d1c3a4f47f`, worktree
`~/Documents/GitHub/workshop-cua`. The pull request targets **`v0.5/implementation`** —
never `dev` and never `main`. `v0.4.0` stays immutable.

Every claim in §4 and §8 was re-verified against `ec4fae7` at branch time and held.
Re-verify again before each phase: this branch is long-lived and the base moves.

**Scope:** one optional plugin that lets a Workshop agent observe and drive native
macOS apps in the background, under explicit per-app consent, with every action
receipted and replayable

**Related:** `../workspace_permissions.md`, `container_capability_contract.md`,
`../apps/synth_desktop/skills/use-synth-plugins/SKILL.md`

## Decision in one sentence

Workshop ships its own signed Computer Use helper as **plugin #2** behind the
existing plugin lifecycle, approval broker, and MCP-group machinery — mirroring the
DevEx and power of the Codex Computer Use suite without depending on ChatGPT.app,
Codex.app, or any OpenAI binary.

## Why we cannot reuse the reference implementation

The Codex suite installs a helper at
`~/.codex/computer-use/Codex Computer Use.app`. Its MCP handshake and `tools/list`
succeed from any caller, but the first real tool call returns:

```
Computer Use server error -10000: Sender process is not authenticated
```

Both nested helpers ship a `*_Parent.coderequirement` demanding
`team-identifier 2DC432GLL2`. This is enforced, not advisory. Every capability below
is ours to build.

---

## 1. Scope and non-goals

The reference suite is four surfaces. The same client binary runs
`computer-use mcp`, `record-and-replay mcp`, `messages mcp`, and
`computer-history mcp`, and a background **Skysight** process writes rolling
10-minute / 6-hour activity summaries into agent memory
(`Package_ComputerUse.bundle/Contents/Resources/SkysightMemoryInstructions.md`).

**In scope:** computer-use only — observe an app, act on it, consent, receipt, replay.

**Explicitly out of scope**, and not to be added without a separate decision:

| Out | Why |
|---|---|
| Record & Replay (record the human to synthesize a skill) | Different product, much worse privacy story, no dependency either way |
| Ambient activity summarization (Skysight) | A background process reading every window is not something Workshop should ship quietly |
| Messages / Contacts / per-app data TCC | Separate entitlements, separate consent model |
| Lock-screen authentication | See §7 — we deliberately refuse this capability |

Scope creep here is the main risk to the project's credibility. The value is a
research-engineering loop that can *see whether the software works*, not an
ambient observer.

---

## 2. Verified reference correspondence

Measured on the installed bundle, not from documentation.

| Design element | Verified in the shipped Codex suite |
|---|---|
| Signed, notarized `LSUIElement` helper holding TCC (G1) | `Identifier=com.openai.sky.CUAService`, `flags=0x10000(runtime)`, `Authority=Developer ID Application: OpenAI OpCo, LLC (2DC432GLL2)`, `spctl: accepted · source=Notarized Developer ID` |
| Permission rows with live state (§6 step 5, G4) | `PermissionRowRegistry`, `PermissionViewState`, `PermissionWindowController`, `_isAccessibilityGranted`, `_isScreenRecordingGranted` |
| Per-app allowlist with persisted approval scopes (G5) | `AppApprovalStore`, `_approvalPersistence`, `_approvalResult` — in module `ComputerUseClient` |
| Background driving, cursor untouched (G3) | `SystemFocusStealPreventer`, `SyntheticAppFocusEnforcer`, `VirtualCursor`, `EventTap`, `WindowServerSPI` |
| Content-level approval for hazard actions (G6) | `MessagesSendApprovalStore`, `MessagesPermissionGate` — approval is bound to recipient and text, not to "may use Messages" |
| Per-action AX + screenshot as raw material (G8) | `get_app_state` returns `{ screenshot, text }` per call, AX tree diffed by default |
| Session-scoped app allowlist (G5) | `~/.codex/computer-use/sessions/<uuid>.toml` → `[apps] allowed = [...]` |

Two details worth carrying:

- **No stapled notarization ticket** — `stapler validate` fails; Gatekeeper accepts
  via online check. We staple ours, so a user offline at first launch does not hit
  a Gatekeeper failure.
- **The client/service split is load-bearing.** Permissions live service-side and
  are long-lived; approvals live client-side (`ComputerUseClient`) and churn per
  session. That is why their TCC grants stay stable while sessions turn over, and
  it maps onto our split: `session/approval.rs` owns approvals, the helper owns grants.

---

## 3. Success gates

A gate is a machine-graded receipt, not a checked box. Each carries tester, build
revision, artifact SHA-256, and timestamp, per `../qa_cua_end_to_end.md`.

| | Gate | Bar |
|---|---|---|
| **G1** | Grant Accessibility → rebuild → reinstall → **grants survive** | Fails today: `scripts/release-artifact.sh` ad-hoc signs, rotating the cdhash every build |
| **G2** | Cold install walks `not_installed → downloading → verifying → needs_permissions → ready`; receipt carries version, digest, `approval_receipt_id` | Proves the plugin lifecycle generalized |
| **G3** | Drive an app while the operator types in another: real cursor does not move, frontmost app unchanged, no Space switch | The product. Foreground automation is not |
| **G4** | Disabled or unauthorized: the tool stays advertised and refuses with typed `needs_permissions` naming the exact missing grant | Matches the `plugin_not_ready` precedent set in `session/codex/home.rs` |
| **G5** | An action on a non-allowlisted app raises an approval card; `Once` / `Session` / `Workspace` differ and survive restart; terminal-class apps refuse under every policy | Otherwise the filesystem sandbox is decorative |
| **G6** | Under `approval_policy = "never"`, a hazard-class action still refuses, and the card shows the **payload** — recipient, text, destination — not just the app | Closes `plugins/policy.rs` `("never", _) => approve everything` |
| **G7** | `remove` deletes the helper, runs `tccutil reset`, receipt states `retained_data`, nothing remains in Privacy | Uninstall residue is the standard failure of automation tools |
| **G8** | A session yields structured per-action before/after AX + screenshot, redacted, bound to a versioned run, replayable from the trace store | See the narrowed claim below |
| **G9** | The 37-item CUA manual gate in `evals/workshop/manual/CUA_MANUAL_GATE.md` runs machine-graded | The app QAs itself |
| **G10** | Element-indexed targeting is the primary path: a scripted task completes using `element_index` only, with coordinates never used | §5 — this is the mechanism that makes background driving reliable |
| **G11** | Lock mid-action: zero events delivered while locked, pending approvals do not expire, session resumes on unlock with a forced full AX re-read, and a lock beyond the ceiling terminalizes | §7 |

**G1 and G6 are release-blocking.** The rest is craft; those two are correctness.

### G8, stated narrowly

The reference suite *does* capture. Record & Replay records the human to synthesize
a skill; Skysight writes rolling prose summaries into agent memory. What nobody
ships is **structured per-action before/after state, redacted, bound to a versioned
run, for evaluation and training.** That is the differentiator. "They don't record
anything" is false and must not appear in any pitch.

---

## 4. Plugin system changes

Five generalizations. Each has an existing invitation in the code.

1. **Registry goes multi-plugin.** `plugins/registry.rs:1` — *"Only `optimizers` is
   registered in this cut."* Hardcoded `plugins/optimizers.json`;
   `OPTIMIZERS_PLUGIN_ID` baked into `PluginNotReady`; `CatalogEntry` carries
   optimizer-shaped fields (`algorithms`, `templates`, `recipe_schema_version`,
   `bounded_recipes`) that move behind a per-plugin payload.
2. **A 14th phase: `needs_permissions`.** `installed` is wrong and `ready` is a lie
   when the binary exists but macOS has not granted Accessibility.
   `runtime/pluginPresentation.ts` already ends with *"`installed` and any phase the
   native side adds later"*, so this is a label, a tone, and a branch.
3. **`PluginStatus` grows a permissions block** — per-grant rows with state and a
   System Settings deep link, mirroring `PermissionRowRegistry`.
4. **`PluginRisk` grows `HandOff`**, unreachable from `auto_decision` under any
   policy. Three consumers today; far cheaper now than after shipping.
5. **Install becomes a strategy.** Today install means "pip package from pypi.org
   becomes a sidecar." Ours is "verify notarization and team ID, place a signed
   `.app`, then acquire OS grants." Same phases, different executor.

### Two deliberate carve-outs

**Human-only lifecycle.** `bin/synth_plugins_mcp.rs` advertises the full lifecycle —
`install`, `enable`, `start`, `remove` — to the *agent*. Defensible for optimizers.
Not defensible for the plugin that grants an agent control of the machine. Computer
Use stays **out** of the `plugin_id` enum in the MCP facade; the agent gets `status`
at most. This is a deviation from the optimizers precedent and is intentional.

**Its own MCP group, off by default.** `context.rs` gates registration by group
(`bundled` / `productivity` / `development`), user-toggleable via
`context_mcp_group_update`. Computer Use gets a `computer-use` group so it never
inherits `bundled`'s always-on default.

Note that `paused` (§7) is a **session** state, not a plugin phase. It does not
enter `PLUGIN_PHASES`.

---

## 5. Action vocabulary (normative)

Element-indexed targeting is not decoration — it is why background driving works.
Pixel grounding needs a foreground window and a stable cursor; accessibility actions
need neither. The agent-facing surface mirrors the reference window API:

```ts
list_apps(): Array<{ id, displayName?, isRunning?, lastUsedDate?, useCount? }>

get_app_state({ app, disableDiff? }): { app, screenshot: { url } | null, text }
  // `text` is the AX tree with element indexes.
  // Diffed against the previous read by default; `disableDiff` forces a full tree.
  // Launches the app in the background if it is not running.

click({ app, element_index?, x?, y?, mouse_button?, click_count? })
set_value({ app, element_index, value })
type_text({ app, text })
press_key({ app, key })                  // xdotool-style keysyms; app-scoped
scroll({ app, element_index, direction, pages? })
select_text({ app, element_index, text, prefix?, suffix?, selection_type? })
drag({ app, from_x, from_y, to_x, to_y })
perform_secondary_action({ app, element_index, action })
```

Rules the skill must state and G10 must enforce:

- **`element_index` first.** Coordinates are a fallback for canvas-style surfaces
  (Figma, Blender, WebGL, custom renderers), not a default.
- **Re-read state after acting.** Element indexes are invalidated by any UI change.
  Never reuse an index across an action boundary.
- **`press_key` and `type_text` are app-scoped** and therefore cannot invoke global
  shortcuts. This is a feature: it is what keeps the operator's session intact.
- **`perform_secondary_action` requires an action the element actually exposes.**
  Do not guess action names.
- **Auto-settling belongs to the runtime, not the agent.** Wait ~1s after an action,
  extending up to ~5s while a loading indicator or state churn is visible, before
  capturing the next state. The agent must not sleep.
- **An explicit foreground escape hatch** exists for apps that refuse background
  delivery, and using it is visible in the trajectory.

---

## 6. User activation

```
1  Sidebar → Plugins            Computer Use · Not installed
2  Click                        → ComputerUsePage · [Install]
3  Approval card                Synth Laboratories · v1.0.0 · sha256:… · 8 MB
                                <host> · "Install a signed helper that can observe
                                and control apps you allow"              [Approve]
4  downloading → verifying → needs_permissions
5  Permission rows              Accessibility        ○  [Open System Settings]
                                Screen Recording     ○  [Grant]
                                Apple Events         —  (asked per app, first use)
                                   …rows flip live as macOS grants
6  ready
7  In a conversation            app-scope chip, empty by default
                                first action on an app → approval card
                                [Once] [This session] [Always]
                                hazard-class actions additionally show the payload
8  While acting                 HUD overlay + synthetic cursor · Esc cancels
9  Off                          Settings → Context → MCP groups → computer-use ▢
                                Plugins → Disable  (advertised, refuses typed)
                                Plugins → Remove   (helper deleted + tccutil reset)
```

Step 9's middle rung matters: **disable does not hide the tool.** The agent can then
say "I can't — Accessibility isn't granted, open Settings → Plugins → Computer Use"
instead of silently lacking the capability and improvising a worse path.

---

## 7. Lock-screen behavior: pause and resume

**Decision:** the session pauses when the screen locks and resumes when it unlocks.
It does not abort on lock, and it never authenticates through the lock screen.

The reference suite builds the opposite capability — `LockScreenGuardianCoordinator`,
`LockScreenLoginAuthorizationBroker`, `LockScreenLoginAuthorizationApprover`, an XPC
protocol, and a socket at `/tmp/com.openai.sky.CUAService/LockScreenLoginAuthorization.sock`.
**We build none of it.** An agent that can get through a lock screen is a
credential-bypass primitive, and refusing it is a feature.

Required behavior:

| On | Behavior |
|---|---|
| **Lock detected** (`com.apple.screenIsLocked`; corroborate with `CGSSessionScreenIsLocked`) | Stop delivering events immediately, mid-sequence if necessary. Mark the session `paused` and emit a durable event. Never post an event while locked — a synthesized keystroke can reach the login window. |
| **While paused** | Suspend approval expiry. Pending approvals must survive the lock rather than time out against `PLUGIN_APPROVAL_TIMEOUT`; expiring them silently converts a coffee break into a failed run. Capture continues to be refused, not queued. |
| **Unlock** | Force a full, non-diffed `get_app_state` before any action. Invalidate every cached `element_index`, screenshot id, and coordinate — displays may have been disconnected, windows moved, dialogs raised. The agent re-derives indexes from the fresh tree. |
| **Pause ceiling exceeded** (default 30 min, configurable) | Terminalize the session, expire pending approvals with reason `locked_too_long`, and receipt it. A machine locked overnight must not resume against a world that has moved on. |

The trajectory records the pause and resume as first-class events, so a replay shows
the gap rather than an unexplained state discontinuity.

---

## 8. File tree

**New:**

```
apps/synth_desktop/src-tauri/src/computer_use/
    mod.rs · helper.rs · client.rs · permissions.rs
    allowlist.rs · policy.rs · trajectory.rs · lock.rs
apps/synth_desktop/src-tauri/src/bin/synth_computer_use_mcp.rs
apps/synth_desktop/skills/use-computer-use/SKILL.md
apps/synth_desktop/src/renderer/src/components/ComputerUsePage.tsx
apps/synth_desktop/src/renderer/src/components/ComputerUsePermissions.tsx
apps/synth_desktop/src/renderer/src/runtime/computerUse.ts
helpers/synth-computer-use/          # signed LSUIElement helper; forked cua-driver AX/event layer
    Cargo.toml · src/ · Info.plist · entitlements.plist
docs/COMPUTER_USE.md                 # this file
```

**Modified:**

```
src-tauri/src/plugins/types.rs           + needs_permissions phase; permissions block
src-tauri/src/plugins/registry.rs        single-plugin → N (plugins/<id>.json)
src-tauri/src/plugins/policy.rs          + PluginRisk::HandOff; per-plugin service_effect
src-tauri/src/plugins/service.rs         install path pluggable: pip sidecar | signed .app
src-tauri/src/session/approval.rs        + ApprovalKind::ComputerUse { app, action, payload }
src-tauri/src/session/codex/home.rs      + ("synth_computer_use", "synth-computer-use-mcp")
src-tauri/src/context.rs                 + "computer-use" MCP group
src-tauri/src/visuals_ipc.rs             /v1/plugins/optimizers/* → /v1/plugins/{id}/*
src-tauri/src/bin/synth_plugins_mcp.rs   plugin_id enum — see the human-only carve-out
renderer/runtime/pluginNav.ts            + entry (id union, icons, Sidebar row handlers)
renderer/runtime/pluginPresentation.ts   + needs_permissions label and tone
renderer/routes.tsx                      + computer-use route
src-tauri/Info.plist                     + NSAppleEventsUsageDescription
src-tauri/tauri.conf.json                + nested helper, entitlements
scripts/release-artifact.sh, desktop.sh  ad-hoc → Developer ID + hardened runtime
                                         + notarize + staple + sign nested helper
```

Line numbers are deliberately omitted: the base branch moves several times a day.
Locate by symbol.

---

## 9. Phases

**Phase 0 runs in parallel with everything else and is not a code task.** Apple
Developer Program enrollment has external lead time measured in days-to-weeks, so
it starts on day one and finishes whenever Apple finishes. Phases 2, 5, and 6 are
pure Rust and TypeScript and do not wait on it. Phases 3, 4, and the G1 receipt do.

| Phase | Work | Est. | Status |
|---|---|---|---|
| **0** | Developer ID, hardened runtime, notarization, stapling, stable helper bundle ID. **Blocking for 3/4/G1; external lead time** | ~1 wk | Not started — needs an operator |
| **1** | Spike: drive the installed Synth Desktop.app. *Does a Tauri/WebKit window expose a usable AX tree?* | ~2 d | Needs an operator TCC grant |
| **2** | Generalize `plugins/` to N; add `needs_permissions` and permission rows; add `PluginRisk::HandOff` | ~1 wk | **In progress** |
| **3** | Helper app + MCP proxy + caller authentication (`SecCode` / audit token against our team ID) | ~2–3 wk | Blocked on Phase 0 |
| **4** | Permission wizard from plugin phases; install / uninstall including `tccutil reset` | ~1 wk | Blocked on Phase 0 |
| **5** | `ApprovalKind::ComputerUse` with payload; app allowlist; terminal-class denial; lock pause/resume | ~1.5 wk | Unblocked |
| **6** | Trajectory capture into `event_journal` + `content_store`, redacted | ~1 wk | Unblocked |
| **7** | Point it at the 37-item CUA manual gate | — | Blocked on 3 |

**~5–7 weeks.** Signing is the long pole; the driver is the easy part. The reference
service binary is 20 MB and most of it is permission state machines, focus-steal
prevention, and a synthetic cursor — not the automation itself.

---

## 10. Open questions

1. **Developer ID** — does one exist under the Synth entity, or is this a fresh
   Apple Developer Program enrollment? Gates Phase 0 and has lead time.
2. **`cua-driver` license** — the repo is MIT; confirm `libs/cua-driver` carries no
   separate terms before vendoring.
3. **Terminal-class denial** — recommended and assumed throughout. Confirm it is a
   hard policy rather than an approvable action.
4. **Pause ceiling** — 30 minutes is a proposal, not a measured number.

---

## 11. Readiness

| | State |
|---|---|
| Design | Settled. §5 and §7 are normative; §3 is gradeable |
| Phase 0 (Developer ID) | **Not started, external lead time, needs an operator.** Nothing an engineer can unblock |
| Phase 1 (AX spike) | Ready to run; needs a TCC grant from the operator and approval to install a third-party helper for comparison |
| Phase 2 | In progress on `v0.5/cua` |
| Phases 3–7 | Ready to write; 3, 4, and 7 land after Phase 0 |

Two things are unknown in a way no amount of planning resolves, and both are cheap
to answer:

1. **Does a Tauri/WebKit window expose a usable AX tree?** If it does not, G9 —
   pointing this at Workshop's own CUA gate — is unreachable, and driving third-party
   apps stays fine. Worth knowing before Phase 2, not after.
2. **Do TCC grants actually survive a Developer ID rebuild in our setup?** G1 is
   release-blocking and asserts they do. Verify on the first signed build.
