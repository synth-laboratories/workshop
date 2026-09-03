# Workshop v0.8 right-panel CUA QA

Date: 2026-08-25  
Build: `Synth Workshop v0.8 · phasea-0825-clean`  
Surface: packaged Tauri application (`tauri://localhost`)  
Method: macOS Computer Use against the live application, using accessibility-tree inspection and screenshots.

## Coverage

- Visuals registry empty and populated states
- Creating and opening a blank visual
- Pointer and keyboard right-panel resizing
- Expanded visual mode and Escape handling
- All, Recent, and Live visual filters
- Real local experiment lineage and node inspector
- Baseline, variant, and result-node selection
- Report creation and visual-to-report destination discovery
- Cross-surface artifact identity and lifecycle consistency across Visuals, Data, Reports, and Chat Outputs
- Navigation-stack, workbench-shell, and local-versus-cloud authority coherence

## Findings

### RP-CUA-001 — Filtered empty state falsely says the registry has no visuals (P1)

After creating a draft visual, selecting `Live` produces:

> No visuals yet. Create one from chat, MCP, or New visual.

The registry still contains one draft visual and the page heading continues to expose a count of one. The empty state should say that no visuals match the active `Live` filter and offer `Clear filter`.

### RP-CUA-002 — Experiment DAG nodes are pointer-only to assistive technology (P0 accessibility)

The lineage canvas is exposed as one settable `listbox`, but its baseline, variant, and result nodes are exposed as static text rather than options or buttons. Focusing the list and pressing the arrow keys did not move selection. Pointer coordinate clicks did select the nodes and update the inspector.

Required: expose nodes as keyboard-focusable options, implement arrow/Home/End navigation, announce selection, and preserve visible focus.

### RP-CUA-003 — Result inspector renders large records as an unstructured JSON wall (P1)

Selecting the real comparison-result node places full config, metrics, artifact paths, limitations, and remediation into narrow definition-list values. The content becomes a dense wrapped JSON wall, pushes evidence below the viewport, and makes the useful failure explanation hard to discover.

Required: project typed fields into summary sections; collapse raw JSON behind `Technical details`; keep evidence and remediation above the fold.

Evidence: `experiment-result-json-wall.png`.

### RP-CUA-004 — Escape closes an expanded visual instead of restoring split view (P1)

Starting in split view, expanding the visual changes the control to `Restore split view`. Pressing Escape closes the visual pane entirely and returns to the Visuals library. This loses the user's open visual context and violates the expected escape hierarchy.

Expected first Escape: restore split view. A subsequent Escape may close the pane.

### RP-CUA-005 — Resize value and realized pane geometry do not clearly agree (P1)

Keyboard resizing changed the accessible splitter value from `420` to `340`; pointer resizing changed it to `546`. The visible pane changed much less than those values suggest, and the value is not readily reconcilable with the realized content width. This needs a DOM geometry assertion in the packaged build, not only browser fixtures.

Required: `aria-valuenow` must report the realized CSS-pixel pane width after min/max constraints. Persist that realized value, not the requested value.

Evidence: `visual-keyboard-resize.png` and `visual-pointer-resize.png`.

### RP-CUA-006 — Experiment detail header compresses provenance into a clipped metadata line (P2)

The long experiment description, missing model marker, and updated timestamp share one single-line region. The description is visibly truncated and the `— · updated` metadata is visually attached to the prose. There is no obvious disclosure for the full description.

Required: give description and immutable provenance separate rows; expose full text with disclosure or tooltip; keep local-only/status badges out of the title flow.

### RP-CUA-007 — Experiment canvas wastes most of its area for a three-node lineage (P2)

The three-node lineage occupies a small strip near the bottom-left of a very large bordered canvas. There are no visible zoom, fit, recenter, or layout controls. The inspector has useful visual contrast but consumes a fixed large column even when selected-node data is almost empty.

Required: fit small graphs to a centered readable bounds; add fit/recenter controls; allow the inspector to collapse or resize.

Evidence: `experiment-baseline.png`.

### RP-CUA-008 — Failed experiment nodes repeat status without failure reason (P1)

Baseline, variant, and result all say `failed`, while the initial inspectors show metrics, cost, provenance, and evidence as em dashes. The actual reason only becomes discoverable after selecting the result and reading its raw provenance JSON.

Required: every failed node needs a one-line reason or explicit `Reason unavailable`; the result inspector should prioritize remediation and terminal receipt.

### RP-CUA-009 — An empty report is presented as ready to seal (P1, adjacent visual workflow)

Creating an untouched report immediately shows `Ready to seal`, `No blocking findings`, and an enabled `Seal report` action despite having no narrative, evidence, experiment records, or research-log entries.

Required: decide and document whether empty reports are valid. If not, require at least one substantive block plus evidence/limitation validation. If valid, change the copy so readiness does not imply research completeness.

### RP-CUA-010 — Optimizer visual QA is blocked in this packaged build (release blocker for coverage)

The sidebar renders `Optimizers — Not installed`, despite optimizer visual families existing in the source tree. Therefore GEPA, SFT, live eval, candidate, frontier, rollout, and optimizer DAG right-panel journeys were not reachable in this build.

Required: install/bundle the optimizer plugin in the v0.8 candidate used for final CUA, or explicitly remove those surfaces from the release acceptance scope.

### RP-CUA-011 — Invalid shared-artifact input enables submission and exposes internal error copy (P1)

Entering `not-a-url` immediately enables `Open shared`. Submitting it renders the inline text:

> private artifact URL is invalid (internal)

The field should validate before enabling submission. The error should be an announced public error, preserve useful remediation, and omit the implementation-oriented `(internal)` suffix.

### RP-CUA-012 — The two adjacent splitters have inconsistent keyboard direction and step sizes (P1 accessibility)

On the same Visuals surface:

- Left Arrow on `Resize visual pane` changed `420 → 380 → 340`.
- Left Arrow on `Resize visual list and preview` changed `560 → 564`.

The same key therefore decreases one reported width by 40 and increases the neighboring width by 4. Keyboard resizing needs one spatial model, documented increments, Home/End handling, and geometry-based tests.

### RP-CUA-013 — “Open canvas” does not expose an authoring canvas (P1)

Opening the blank canvas removes library/search chrome and renames the action to `Exit canvas`, but the main surface still only says:

> Blank canvas — No canvas document has been authored yet.

There are no authoring controls, insertion affordances, editable document, or explanation of how to author one. The action promises an editor but enters a largely empty presentation mode.

Evidence: `visual-open-canvas-no-editor.png`.

### RP-CUA-014 — Unresolved blank evidence can be attached to a report that remains “Ready to seal” (P0 truthfulness)

The untouched blank visual could be added to the empty report. The report then described it simultaneously as:

- `live`
- `available`
- `unresolved`
- `Frozen evidence attached to this revision`

Preflight still reported `Ready to seal` with no blocking findings, and `Seal report` remained enabled. A live unresolved blank canvas is not frozen research evidence. Report validation must require a sealable visual revision and reject unresolved/empty evidence unless explicitly documented as a limitation.

### RP-CUA-015 — Report filtered-empty copy falsely says no reports exist (P1)

With one draft report present, selecting the `Sealed` filter says:

> No reports yet. Create one to freeze narrative plus evidence.

This repeats the Visuals filtering defect. It should say no reports match the active filter and offer to clear it.

### RP-CUA-016 — Experiment search has no no-results state (P2)

Searching for `zzzz-no-match` removes the only experiment row and leaves only an empty table header. There is no result count, no-results explanation, or clear-search action.

### RP-CUA-017 — Report outline fragments leak across top-level routes (P2)

After selecting the report's `Research Log` outline link, navigating to Experiments and Data left the application URL at `tauri://localhost#research-log`. Top-level route changes should clear report-local fragments so focus restoration, deep-link semantics, and browser history do not point at unrelated hidden content.

### RP-CUA-018 — Usage cost presentation contradicts itself (P0 economics)

The same Usage screen reports:

- `DEVICE SPEND Unavailable`
- `No request in this window carried a price`
- the model row cost as `No charge`
- `Unpriced 0.0%`

There are six requests and 675K processed tokens. If no request carried a price, the row must say `Unavailable`, not `No charge`, and unpriced coverage should reflect the unpriced request/token population rather than `0.0%`. This violates the no-fabricated-cost rule.

Evidence: `usage-unpriced-contradictions.png`.

### RP-CUA-019 — Inference diagnostics do not identify cloud versus local authority (P1)

The Inference tab labels the surface `Laguna XS 2.1`, `BASE MODEL`, `UNLOADED`, and `IDLE`, but does not state whether the observation is local runtime state or Synth Cloud/Shoal state. This makes the diagnostics ambiguous precisely where Workshop now distinguishes local and cloud warming.

Required: show `Local` or `Synth Cloud · Shoal` as an explicit source, include the observation timestamp, and avoid presenting one source's unloaded state as global model state.

### RP-CUA-020 — Inference diagnostics stay fixed-width and waste most of the workspace (P2)

The inference card stays roughly 300px wide both with and without the right visual pane, leaving most of the main workspace empty. Charts and request history do not expand into the available area.

Evidence: `inference-with-visual-pane.png` and `inference-fixed-width-empty-space.png`.

### RP-CUA-021 — Narrow Visuals split view breaks its header and preserves an oversized panel (P1 responsive)

At an approximately 968px application width with the sidebar visible, the right panel remains open at its persisted `546` splitter value. The main Visuals header becomes too narrow:

- `+ New visual` wraps into three short lines.
- The page lede wraps into a tall four-line block.
- The shared-artifact input is heavily clipped.
- The visual splitter is exposed to accessibility but has almost no visible grab target.

The layout should automatically collapse secondary library chrome, constrain/restack the right panel, or offer a compact-sidebar mode before controls become word stacks.

Evidence: `narrow-visual-split.png`.

### RP-CUA-022 — Narrow experiment registry columns collide and clip authoritative fields (P1 responsive)

At the same window width, the experiment table renders `Result recorded3` with the Result and Runs values touching. The Updated timestamp is clipped at the right edge, while the full task description consumes a tall narrow column.

Required: switch to a compact card/priority-column layout, preserve status/result/run count/timestamp legibility, and put the full task description behind disclosure.

Evidence: `narrow-experiment-registry.png`.

### RP-CUA-023 — Narrow experiment DAG hides the result node entirely (P0 lineage completeness)

In the experiment detail view, the canvas shows baseline and variant plus an `evaluated` arrow at the right boundary, but the result node is outside the visible canvas. There is no horizontal scroll affordance, fit control, minimap, or keyboard path to reach it. The inspector consumes a fixed column while the lineage itself is incomplete.

An experiment viewer must never omit a lineage node merely because the desktop window is narrow.

Evidence: `narrow-experiment-dag-clipped.png`.

### RP-CUA-024 — Pin validation and seal validation disagree (P0 evidence integrity)

`Pin all evidence` correctly refuses the attached blank visual:

> cannot pin unresolved evidence blocks: visual-vis_b38ea7d7 (internal)

Yet the same report simultaneously remains `Ready to seal`, says `No blocking findings`, and keeps `Seal report` enabled. Pinning and sealing must consume the same evidence-validity predicate. The refusal should also use public-facing copy without `(internal)`.

Evidence: `report-unresolved-pin-error.png`.

### RP-CUA-025 — An unresolved blank visual is offered as evidence for a claim (P0 evidence integrity)

The Claims evidence selector lists `New visual` even though that visual is blank, live, unresolved, unpinned, and rejected by `Pin all evidence`. Claim construction must exclude evidence that cannot be pinned/sealed, or visibly mark it unavailable with the reason.

### RP-CUA-026 — Empty private-report URL has an enabled no-op action (P2)

`Open report` remains enabled while `Private Report URL` is empty. Activating it produces no visible response, focus movement, validation message, or diagnostic. Disable the action until the URL is valid and announce validation failures.

### RP-CUA-027 — Expanded visual mode still reserves the full application sidebar (P2)

At narrow width, `Expand visual` removes the Visuals library but retains the complete 260px application sidebar, leaving substantially less room for the supposedly expanded canvas. Consider a true focus mode that collapses the sidebar automatically while preserving an obvious restore action.

Evidence: `narrow-visual-expanded.png`.

### RP-CUA-028 — Internal report errors remain indefinitely in page chrome (P1)

After `Pin all evidence` failed, the internal error remained visible through window resizing, outline navigation, and leaving/returning to the report:

> cannot pin unresolved evidence blocks: visual-vis_b38ea7d7 (internal)

There is no dismiss action, timeout, public remediation, or association with the control that caused it. Present the error near evidence validation, remove internal wording, and provide deterministic dismissal/focus behavior.

### RP-CUA-029 — Back from a chat output report creates a new blank conversation (P0 navigation)

Opening `Untitled report` from the chat Outputs panel correctly navigates to Reports. Activating the report page's `Back` button did not return to the originating chat or the prior Outputs state. It opened a new blank `GPT 5.6 Luna` conversation instead.

This breaks context preservation and can make a user believe their active chat disappeared. Back must restore the exact originating route, chat, scroll position, and side-panel state.

### RP-CUA-030 — Selecting the Diagnostics tab closes the entire side panel (P1)

The Workbench side panel exposes `Outputs`, `Advanced`, and `Diagnostics` as sibling tabs. Selecting `Diagnostics` from a freshly inspected accessibility tree consistently removed the entire side panel rather than selecting or rendering diagnostics. This reproduced twice.

Required: render a diagnostics surface or expose a truthful unavailable state; never treat a tab selection as Close.

### RP-CUA-031 — Closing Outputs loses keyboard focus (P1 accessibility)

After activating `Close side panel`, accessibility focus moved to the root HTML content instead of returning to the `Outputs 1` disclosure that opened it. Keyboard users must traverse the page again to reopen the panel.

### RP-CUA-032 — Hiding the terminal also loses keyboard focus (P1 accessibility)

Activating the top-level `Hide terminal` action moved focus to the root HTML content instead of the corresponding `Show terminal` control. The same defect affects two independent disclosure surfaces, suggesting the application lacks a general focus-restoration contract.

### RP-CUA-033 — Terminal, composer, and Outputs panel create a heavily occluded workbench (P1 layout)

With Outputs open and the terminal visible, three persistent surfaces compete with the transcript:

- the right Outputs panel,
- the floating composer,
- the bottom terminal.

The composer overlays transcript content immediately above the terminal, while the Outputs panel is mostly empty. Receipt and cleanup text remain partially hidden behind the composer. The layout needs coordinated geometry rather than three independent overlays.

Evidence: `chat-output-terminal-overlap.png`.

### RP-CUA-034 — Chat Outputs does not preserve output context through report navigation (P1)

The Outputs panel lists the draft report, but opening it destroys the side-panel state. Returning does not restore Outputs; combined with RP-CUA-029 it creates a discontinuous output-review journey. Output details should preferably open in the right panel, or route navigation must persist and restore the exact panel tab and selection.

Evidence: `chat-outputs-panel.png`.

### RP-CUA-035 — Duplicate visual attachment is offered, then rejected with an internal validation error (P1)

After `New visual` was already attached to `Untitled report`, the Visuals page continued to offer the same report destination and an enabled `Add to report` action. Activating it produced:

> report validation duplicate_block_anchor: anchor visual-vis_b38ea7d7 is duplicated (internal)

The duplicate was not persisted, which is correct, but the UI should recognize `Already added`, disable the action, and offer `Open in report`. Users should not reach an internal schema-validation error through an ordinary enabled action.

### RP-CUA-036 — Report mutation persistence conflicts with the visible “Save draft” model (P1)

Attaching the visual persisted across route changes without activating `Save draft`, while the report remained revision 1 and continued to expose an enabled `Save draft` action. The UI provides no dirty/saved indicator or explanation of which mutations autosave.

Required: choose one coherent model:

- explicit save with unsaved-change protection, or
- autosave with a visible saved/saving/error state and no misleading primary Save action.

### RP-CUA-037 — Application fragment state remains stale even after returning to chat (P1 navigation)

The report-local `#research-log` fragment persisted not only across Experiments and Data, but all the way back to the active chat and its Outputs/Advanced panels. This raises the severity of RP-CUA-017: stale report focus state is global to the Tauri document rather than scoped to Reports.

### RP-CUA-038 — Standard zoom shortcuts provide no visible state or feedback (P2 accessibility)

Two `Command + Plus` actions and `Command + 0` produced no accessibility-tree change, visible zoom indicator, or menu/state feedback. If application zoom is intentionally unsupported, Workshop should rely on and validate platform text scaling. If it is intended, expose current zoom and ensure shortcuts work consistently in the packaged app.

### RP-CUA-039 — Dark theme leaves the application sidebar effectively unreadable (P0 accessibility)

After explicitly selecting Dark, the main Settings surface became dark but the persistent application sidebar remained light while much of its text switched to very pale gray/white. Chat headings, recents, plugin names, selection labels, and `Not installed` text became extremely low contrast.

This is a release-blocking theme defect because primary navigation becomes difficult to perceive.

Evidence: `experiment-dark-theme-mixed.png` and `visuals-dark-theme-mixed.png`.

### RP-CUA-040 — Dark theme is not propagated across visual surfaces (P0 accessibility)

The experiment detail retained its warm light canvas while the tab and node inspector were dark. The Visuals page main region became dark while the right artifact panel remained bright light. These are not intentional contrast regions: they split single workspaces across incompatible token sets and create severe luminance transitions.

Required: every visual family and right-panel host must consume the same theme contract and pass contrast checks in both modes.

### RP-CUA-041 — Dark Visuals preview contains nearly invisible dark-on-dark content (P0 accessibility)

Within the dark Visuals page, the preview's `Blank canvas` heading and empty-state text render in a very dark color on a dark card. The content is almost invisible, independently of the light right-panel mismatch.

Evidence: `visuals-dark-theme-mixed.png`.

### RP-CUA-042 — Back from Settings also creates a blank conversation (P1 navigation)

Settings was opened from an active visual/report QA context. Activating Settings `Back` returned to a new blank `GPT-5.6 Luna` conversation instead of the originating page. RP-CUA-029 is therefore a general navigation-stack defect, not something limited to report outputs.

### RP-CUA-043 — Report block movement controls are enabled at impossible boundaries (P2)

The report contains exactly one movable visual block, yet both `↑` and `↓` controls are enabled. Moving above the first or below the last block is impossible and should be disabled. With multiple blocks, the controls also need contextual accessible names such as `Move New visual up`.

### RP-CUA-044 — Report block controls lack contextual accessible names (P1 accessibility)

The visual evidence block exposes generic buttons named only `↑`, `↓`, and `Remove`. A report with several blocks would present repeated indistinguishable controls to assistive technology. Names must include the block title/type and position, and focus must follow the moved block.

### RP-CUA-045 — Closing the visual artifact pane loses keyboard focus (P1 accessibility)

Activating `Close visual` removed the pane but moved focus to the root HTML content rather than the visual card's `Open` control. Together with RP-CUA-031 and RP-CUA-032, every tested dismissible workbench region lacks focus restoration.

Required: build one shared disclosure/panel primitive that records the invoker and restores focus on close, Escape, route change, and render failure.

### RP-CUA-046 — Label placement provides no visible anchor marker (P1)

Entering Label mode and clicking the visual updates text to `Placed at 50%, 6%` and outlines the whole panel with a dashed border, but no pin, dot, crosshair, callout, or other marker appears at the selected location. The user cannot verify what the durable coordinates refer to before saving.

Evidence: `visual-label-placement-no-marker.png`.

### RP-CUA-047 — Label controls collapse into unreadable narrow fragments (P1 responsive)

At the persisted right-panel width, the placement status wraps vertically as separate fragments (`Placed`, `at`, `50%`, `6%`) beside a compressed note field and actions. Label mode should use a stacked compact layout instead of forcing every control into one row.

### RP-CUA-048 — Cancelling label placement loses focus (P1 accessibility)

Activating the explicit `Cancel` action exits label mode but moves focus to the root document rather than the `Label` button that invoked it. This is another manifestation of the missing shared focus-restoration contract.

### RP-CUA-049 — Escape from label mode closes the entire visual pane (P1 interaction)

With label mode active, pressing Escape does not merely cancel label placement. It closes the complete visual artifact pane and loses context. The expected hierarchy is:

1. Cancel label placement.
2. Restore split mode if expanded.
3. Close the visual pane only on a later Escape.

### RP-CUA-050 — “Templates” is a misleading instance filter, not a template library (P2)

Selecting `Templates` continues to display ordinary draft visual instances such as `New visual · blank.canvas.v1`; it does not present the registered template catalog or a template-selection workflow. The implementation filters instances by `rendererKind === "template"`, but the user-facing label implies reusable templates.

Rename it to `Template visuals` or provide an actual template library.

### RP-CUA-051 — Repeated New visual actions create indistinguishable drafts (P1)

Activating `+ New visual` twice creates two immediately persisted records with the same visible identity:

- `New visual`
- `Draft · rev 1`
- `blank.canvas.v1`

Only their timestamps differ. The right artifact panel also shows only `New visual · rev 1`, so users cannot reliably tell which draft is open.

Evidence: `duplicate-blank-visuals.png`.

### RP-CUA-052 — Draft visuals have no visible rename, archive, or delete lifecycle (P1)

Each registry card exposes only `Open`. The preview offers report attachment and `Open canvas`, while the right panel offers Label, Seal, Expand, and Close. There is no visible way to rename or clean up accidental blank drafts, making the one-click creation behavior permanently clutter the local registry.

### RP-CUA-053 — The artifact pane omits instance identity when titles collide (P1 evidence identity)

With two identically named `blank.canvas.v1` revisions, the artifact header does not show the visual ID or content digest, even behind a disclosure. A revision-oriented evidence UI must make immutable identity available anywhere a user can label, seal, or attach the artifact.

### RP-CUA-054 — One artifact has contradictory state vocabularies across the app (P0 system integrity)

The same blank visual is presented as `Draft · rev 1` in Visuals, `live · available · unresolved` inside Reports, a plain `New visual · blank.canvas.v1` record in Data, and attached to a report listed under `Saved reports` in Chat Outputs. The report then describes it as `Frozen evidence attached` while pin validation rejects it as unresolved.

This is not a copy defect. Workshop lacks one canonical artifact lifecycle and projection contract. Define a single typed state machine—at minimum draft, resolved, sealed/frozen, unavailable, and superseded—and require every surface and action guard to derive from it.

### RP-CUA-055 — Workshop navigation is route replacement, not contextual navigation (P0 workflow continuity)

Back from reports and Settings repeatedly returned to a new blank conversation rather than the exact originating surface. Report fragments persisted globally, and opening a report from Chat Outputs discarded the active output tab and selection. Users cannot form a dependable mental model of where Back goes or whether their working context survives inspection.

Required: introduce an explicit navigation state carrying origin route, selected record, panel tab, scroll/focus anchor, and owning URL fragment. Do not use `landing` or a blank chat as a universal fallback.

### RP-CUA-056 — Right panels are duplicated route plumbing rather than one workbench primitive (P0 architecture)

The renderer repeats `PaneResizeHandle + VisualPane` branches for Visuals, Experiments, Optimizers, Data, and Chat, while Chat separately hosts `WorkbenchSidePanel`. The live defects follow those boundaries: different Escape behavior, different sharing rules, inconsistent resize constraints, focus loss, and route-specific clipping.

Create one workbench-slot host with a panel stack, common resize geometry, focus restoration, Escape hierarchy, responsive collapse policy, and route persistence. Visuals, containers, outputs, inference, trace, and diagnostics should be typed occupants of that host rather than bespoke siblings.

### RP-CUA-057 — Evidence validity is recomputed independently by each feature (P0 evidence integrity)

Attachment, claim selection, pinning, preflight, and sealing disagree about whether the same unresolved blank visual is admissible. Fixing each button independently would preserve future drift.

Required: one evidence-admission service must return a typed decision with reasons and remediation. All five workflows must consume that decision, and a sealed report must persist the exact decision and immutable evidence identities used.

### RP-CUA-058 — Local and cloud authority are visually co-located without a transmission boundary (P0 OSS trust)

Visuals describes a local registry and Experiments says records are not uploaded, yet the same workspaces prominently expose `Private artifact URL` and `Open shared`. Inference diagnostics do not identify local versus Shoal/cloud authority. The UI never clearly states which action reads local state, contacts Synth Cloud, or can transmit an artifact.

Required: show an authority badge on every relevant panel and an explicit transmission checkpoint before a local artifact crosses the boundary. A private URL opener must not imply that the local registry itself is cloud-backed.

### RP-CUA-059 — Artifact lifecycle management is incomplete at the product level (P1)

Creating a visual immediately produces a durable local record, but there is no visible rename, archive, delete, duplicate, or supersede workflow. Reports, Data, Visuals, and Chat Outputs expose different subsets of the same objects. Accidental drafts therefore accumulate permanently and become ambiguous evidence candidates.

Define ownership, retention, cleanup, revision, and recovery semantics once, then expose consistent actions wherever the artifact appears.

### RP-CUA-060 — Immutable identity disappears at cross-surface handoffs (P0 reproducibility)

Human titles and revision numbers dominate the UI while visual IDs, content digests, experiment/candidate identities, and receipt identities are missing from the points where users attach, pin, seal, compare, or reopen records. Duplicate titles make this immediately visible, but the deeper risk is that a user cannot prove that the artifact inspected in one surface is the artifact sealed in another.

Every handoff should preserve and optionally reveal the canonical ID and digest; evidence receipts should reference those values rather than titles.

### RP-CUA-061 — The app has overlapping artifact entry points without a unified information architecture (P1)

Visuals, Data → Visuals, Reports, and Chat Outputs all act as artifact browsers, but use different grouping, metadata, actions, and return behavior. Reports sometimes behave as editors and sometimes as output viewers. The result is multiple partial registries rather than one comprehensible system.

Define the role of each surface: registry/discovery, contextual inspection, authoring, or immutable review. Reuse one artifact summary model and breadcrumb/origin affordance across them.

### RP-CUA-062 — Error presentation leaks implementation layers and lacks stable remediation (P1)

Invalid URLs, duplicate attachments, and unresolved pin attempts surface persistent inline messages that include `(internal)` or feature-specific prose. There is no consistent error code, owning layer, retryability, or link to diagnostics. This will be especially damaging for the Workshop → backend → Shoal → Modal state machine.

Adopt typed public errors with operation ID, source (`local`, `backend`, `shoal`, or `modal`), retryability, user action, and a diagnostics link. Preserve internal details only in local logs/trace views.

### RP-CUA-063 — Capability status in the packaged build does not match the product surface (P0 release readiness)

Optimizer visual families exist in source, but the tested v0.8 build reports Optimizers as `Not installed`; Computer Use is also presented as an optional unavailable capability. A release candidate cannot prove the flagship experiment-to-PR journey while the required capabilities are absent or ambiguous.

Publish an explicit capability manifest in About/Diagnostics and in test evidence. The final candidate must declare which capabilities are bundled, optional, unsupported, or externally configured, and acceptance must run against that exact manifest.

### RP-CUA-064 — Workspace layout has no global space-allocation policy (P0 responsive architecture)

The application sidebar, route content, visual/container pane, Chat side panel, terminal, and composer independently reserve space. At constrained widths this produces clipped DAG nodes, broken headers, vertical label controls, and terminal/composer/Outputs overlap.

Implement a single layout arbiter with minimum viable widths and deterministic priorities—for example overlay or collapse secondary panels before clipping authoritative lineage or composer content. Test a matrix of window sizes, sidebar states, terminal states, and panel combinations.

### RP-CUA-065 — Operational state is not correlated end to end (P1 observability)

Experiment facts, inference warming, artifact evidence, report validation, and diagnostics are displayed as separate local concepts. The tested surfaces do not expose one operation/attempt identity that a user or engineer can follow from Workshop through backend and Shoal to Modal and back into receipts.

Propagate a correlation ID and typed state transitions through chat activity, inference diagnostics, experiment nodes, artifact receipts, and public errors. The right panel should answer both `what is happening now?` and `which durable record proves what happened?` without requiring backend log access.

## What worked

- Visual creation immediately populated the local registry.
- Recent filtering recovered the newly created visual.
- Pointer dragging changed the right-panel width.
- Expand and restore controls were visibly labeled.
- Report destination discovery included the newly created report.
- Experiment node pointer selection consistently updated the inspector.
- Local-only experiment labeling was visible.
- Unknown cost rendered as an em dash rather than a fabricated zero.

## Test artifacts created

The QA run created two local drafts:

- Visuals: two distinct drafts both named `New visual`, `blank.canvas.v1`, revision 1; the second was created to test duplicate visible identity and draft cleanup
- Report: `Untitled report`, revision 1; the blank visual was attached while testing report validation

No visual or report was sealed, shared, published, or uploaded.

Screenshots are stored locally under:

`apps/synth_desktop/test-results/cua-right-panel-20260825/`

## Recommended fix order

1. Freeze one canonical artifact/evidence lifecycle, immutable identity contract, and evidence-admission decision used by every surface.
2. Replace route-specific right-panel plumbing with one focus-safe, responsive workbench panel stack.
3. Implement contextual navigation state that restores the exact origin, selection, panel, URL fragment, scroll, and focus.
4. Make local, backend, Shoal, and Modal authority explicit and propagate one operation ID/state timeline end to end.
5. Add a global workspace space-allocation policy and test the complete sidebar/panel/terminal/window-size matrix.
6. Make every experiment lineage node visible and keyboard-operable at every supported width.
7. Block unresolved/live/blank evidence from satisfying report seal validation.
8. Fix unavailable-cost truthfulness and cost-quality coverage.
9. Replace raw result JSON with typed result, failure, remediation, and evidence sections.
10. Fix expanded-mode Escape behavior.
11. Correct filter-aware empty and search states.
12. Normalize splitter keyboard behavior and bind ARIA values to realized geometry.
13. Add responsive compact layouts for Visuals, experiments, reports, and inference.
14. Repair chat-output report navigation and implement shared focus restoration for panels.
15. Implement the Diagnostics tab and coordinate terminal/composer/side-panel geometry.
16. Make report attachment idempotent and clarify autosave versus explicit-save semantics.
17. Scope URL fragments to their owning route and restore exact origin state on Back.
18. Repair dark-theme propagation and contrast before release.
19. Add contextual report-block actions and boundary-aware reordering.
20. Repair label-mode marker visibility, responsive layout, and Escape hierarchy.
21. Add visual draft naming, cleanup, and immutable-identity affordances.
22. Clarify template-instance filtering versus template discovery.
23. Bundle optimizer visuals into the final QA build and rerun the full visual-family matrix.
