import assert from "node:assert/strict";
import { readFileSync, existsSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const renderer = join(appRoot, "src/renderer/src");

function read(rel) {
  return readFileSync(join(renderer, rel), "utf8");
}

test("stable accessibility testids remain on core surfaces", () => {
  const files = [
    "App.tsx",
    "components/Sidebar.tsx",
    "components/Composer.tsx",
    "components/LandingPage.tsx",
    "components/VisualPane.tsx",
    "components/VisualHost.tsx",
    "components/VisualsPage.tsx",
    "components/InventoryPage.tsx",
    "components/CloudDesk.tsx",
  ];
  const blob = files
    .filter((f) => existsSync(join(renderer, f)))
    .map(read)
    .join("\n");

  for (const id of [
    "sidebar",
    "landing-page",
    "composer",
    "composer-input",
    "composer-send",
    "visual-pane",
    "inventory-page",
    "cloud-desk",
    "visuals-page",
    "open-visuals",
  ]) {
    assert.ok(blob.includes(`data-testid="${id}"`) || blob.includes(`'${id}'`), id);
  }
});

test("installed desktop authorizes its declared window drag regions", () => {
  const app = read("App.tsx");
  const sidebar = read("components/Sidebar.tsx");
  const permissions = readFileSync(join(appRoot, "src-tauri/capabilities/default.json"), "utf8");
  assert.match(`${app}\n${sidebar}`, /data-tauri-drag-region/);
  assert.match(permissions, /core:window:allow-start-dragging/);
});

test("execution targets include Laguna local + OpenRouter Luna/Laguna + Synth Cloud + Intern", () => {
  const types = read("types/landing.ts");
  const composer = read("components/Composer.tsx");
  const bridge = read("runtime/desktopBridge.ts");
  const capabilities = read("runtime/modelCapabilities.ts");
  assert.ok(types.includes("local-laguna") || types.includes("Laguna XS"));
  assert.ok(types.includes('label: "GPT 5.6 Luna"'));
  assert.ok(types.includes("laguna-s-2.1") || types.includes("Laguna S"));
  assert.ok(types.includes("synth-cloud-laguna-s"));
  assert.ok(types.includes("Synth Cloud · usage tracked"));
  assert.ok(types.includes("intern"));
  assert.ok(composer.includes('data-testid={`${knob.testId}-select`}'));
  assert.ok(composer.includes('data-testid={`${knob.testId}-menu`}'));
  assert.ok(capabilities.includes('testId: "reasoning-effort"'));
  assert.ok(bridge.includes("request: { sessionId, prompt, effort }"));
});

test("v0.1 model surfaces exclude Muse, GGUF, and DFlash", () => {
  const files = [
    "App.tsx",
    "types/landing.ts",
    "components/Composer.tsx",
    "components/ProviderMark.tsx",
    "components/SettingsPage.tsx",
    "runtime/desktopBridge.ts",
    "runtime/modelCapabilities.ts",
    "runtime/nativeCodex.ts",
    "runtime/sessionView.ts",
  ];
  const modelSurface = files.map(read).join("\n");
  assert.doesNotMatch(modelSurface, /Muse Spark|muse-spark|openrouter-muse|OPENROUTER_MUSE|GGUF|DFlash/i);
});

test("model knobs are registered once and consumed without model-specific UI or transport branches", () => {
  const registry = read("runtime/modelCapabilities.ts");
  const composer = read("components/Composer.tsx");
  const app = read("App.tsx");
  for (const target of ["local-laguna", "openrouter-luna", "openrouter-laguna-s", "synth-cloud-laguna-s"]) {
    assert.ok(registry.includes(`targetId: "${target}"`), target);
  }
  assert.ok(composer.includes("modelCapabilitiesForTarget(state.selectedTargetId)"));
  assert.ok(composer.includes("modelCapabilities?.knobs.map"));
  assert.ok(!composer.includes('state.selectedTargetId === "openrouter-luna"'));
  assert.ok(!composer.includes('state.selectedTargetId === "openrouter-laguna-s"'));
  // Match the call, not the caller's local name: the point is that effort comes
  // from the registry helper rather than a per-model branch at the send site.
  assert.match(app, /turnStartEffortForExecutionTarget\(\s*\w+\s*,\s*modelKnobValues\s*\)/);
  assert.ok(!app.includes('session.target.model === "openai/gpt-5.6-luna"'));
  assert.ok(!app.includes('session.target.model === "poolside/laguna-s-2.1"'));
});

test("renderer keeps the HTTP runtime bridge browser-only", () => {
  const bridge = read("runtime/desktopBridge.ts");
  assert.ok(bridge.includes("browserRuntimeBridge"));
  assert.ok(bridge.includes("if (!isTauri) window.synthRuntime ??="));
  assert.ok(!bridge.includes('invoke<T>("runtime_request"'));
  assert.ok(!bridge.includes('"runtime_subscribe"'));
});

test("renderer uses Tauri commands for desktop capabilities", () => {
  const bridge = read("runtime/desktopBridge.ts");
  assert.ok(bridge.includes('"workspace_choose_directory"'));
  assert.ok(bridge.includes('"laguna_get_status"'));
  assert.ok(!bridge.includes('"core_projects_list"'));
  assert.ok(bridge.includes('"intern_session_events_after"'));
  assert.ok(bridge.includes('listen<AppEvent>("runtime:event"'));
  assert.ok(bridge.includes('"codex_sessions_list"'));
});

test("parked Projects feature is absent from the active renderer and IPC bridge", () => {
  const app = read("App.tsx");
  const sidebar = read("components/Sidebar.tsx");
  const landing = read("components/LandingPage.tsx");
  const bridge = read("runtime/desktopBridge.ts");
  assert.ok(!sidebar.includes('data-testid="project-list"'));
  assert.ok(!sidebar.includes('data-testid="add-project"'));
  assert.ok(!landing.includes('data-testid="quick-add-project"'));
  assert.ok(!app.includes("selectedProjectId"));
  assert.ok(!bridge.includes("synthProjects"));
});

test("native Codex sessions use one sequence allocator and restore persisted sessions", () => {
  const app = read("App.tsx");
  const nativeCodex = read("runtime/nativeCodex.ts");
  assert.ok(app.includes("allocateNativeSequence(event.sessionId)"));
  assert.ok(app.includes("allocateNativeSequence(sessionId)"));
  assert.ok(app.includes('persisted.filter((session) => session.status !== "closed").map(restoreCodexSession)'));
	assert.ok(app.includes("await nativeCodex.start("));
	assert.ok(app.includes("threadId: typeof session.metadata.threadId"));
  assert.ok(nativeCodex.includes('eventKind = "run.failed"'));
  assert.ok(nativeCodex.includes('eventKind = "run.cancelled"'));
});

test("container skill keeps registry discovery separate from policy execution", () => {
  const skill = readFileSync(join(appRoot, "skills/use-synth-containers/SKILL.md"), "utf8");
  assert.ok(skill.includes("mcp__synth_containers"));
  assert.ok(skill.includes("container_list"));
  assert.ok(skill.includes("engine is not a policy"));
  assert.ok(!skill.includes("container_run_and_visualize"));
});

test("Intern agent messages are projected into transcript and removed from activity", () => {
  const sessionView = read("runtime/sessionView.ts");
  assert.ok(sessionView.includes('event.eventKind === "agent_message"'));
  assert.ok(sessionView.includes('role: isInternAgentMessage ? "assistant" : role'));
  assert.ok(sessionView.includes('event.eventKind === "intern.agent_message"'));
});

test("unconfigured Intern has explicit boot guidance and a disabled composer", () => {
  const composer = read("components/Composer.tsx");
  const settings = read("components/SettingsPage.tsx");
  assert.ok(composer.includes("Configure Synth Cloud in Settings → Account"));
  assert.ok(composer.includes('state.internMode !== "unconfigured"'));
  // v0.1 removal contract: the dormant composer guidance above is unreachable
  // because no Intern target can be selected, and Settings must carry no Intern
  // setup prompt or demo-boot instructions at all.
  assert.ok(!settings.includes("Settings → Account → Synth backend"));
  assert.ok(!settings.includes("SYNTH_INTERN_DEMO"));
});

test("Synth API settings keep routing in TOML while credentials remain in native custody", () => {
  const settings = read("components/BackendSettings.tsx");
  assert.ok(settings.includes('data-testid="backend-settings"'));
  assert.ok(settings.includes("Backend API"));
  assert.ok(settings.includes("Secrets env file"));
	assert.ok(!settings.includes('type="password"'));
	assert.ok(settings.includes("Credentials must already exist in a private env file read only by the native host."));
  assert.ok(settings.includes("Save and reconnect"));
  assert.ok(settings.includes('staging: "https://api-dev.usesynth.ai"'));
  assert.ok(settings.includes("Preserve explicitly customized endpoints"));
});

test("dormant native Intern cannot be selected from v0.1 App routes", () => {
  const app = read("App.tsx");
  assert.ok(app.includes('throw new Error("Enter an objective to start an Intern session")'));
  assert.ok(app.includes("objective: internObjective!"));
  assert.ok(app.includes("objectiveConsumed"));
  assert.ok(app.includes("if (!ensured.objectiveConsumed)"));
  // v0.1 removal contract: the dormant creation path above may remain, but the
  // entry points that selected an Intern target must not. See PRODUCT-NO-INTERN-V0P1.
  assert.ok(!app.includes('setSelectedTargetId("intern-sync")'));
  assert.ok(!app.includes('setSelectedTargetId("intern-async")'));
});

test("native Intern projection changes refresh the renderer session cache", () => {
  const app = read("App.tsx");
  assert.ok(app.includes('event.eventKind === "intern.projection_updated"'));
  assert.ok(app.includes('event.eventKind === "session.updated"'));
  assert.ok(app.includes('event.eventKind === "command.resolved"'));
});

test("migrated demo async pins cannot mask the Rust singleton", () => {
  // The App-side guard went out with the Async Intern pin (v0.1 removal
  // contract); sessionView is now the single place the singleton is enforced.
  const sessionView = read("runtime/sessionView.ts");
  assert.ok(sessionView.includes('const isRustIntern = session.metadata.runtime === "rust-intern"'));
  assert.ok(sessionView.includes("if (asyncIntern && !isRustIntern) continue"));
});

test("renderer exposes CoreRuntime visual registry bridge commands", () => {
  const bridge = read("runtime/desktopBridge.ts");
  assert.ok(bridge.includes('"visuals_list"'));
  assert.ok(bridge.includes('"visuals_create"'));
  assert.ok(bridge.includes('"visuals_show"'));
  assert.ok(bridge.includes("window.synthVisuals ??="));
  assert.ok(bridge.includes('"visual:show"'));
});

test("Rust run counts remain projected without a Runtime Settings surface", () => {
  const app = read("App.tsx");
  const settings = read("components/SettingsPage.tsx");
  assert.ok(app.includes("runs: core.runCount"));
  assert.ok(!settings.includes('id: "runtime"'));
  assert.ok(!settings.includes("health?.dataStore?.runs"));
  assert.ok(!app.includes("runs: 0"));
});

test("v0.2 Intern bridge remains typed while v0.1 creation stays unreachable", () => {
  const bridge = read("runtime/desktopBridge.ts");
  const app = read("App.tsx");
  for (const command of [
    "intern_sessions_list",
    "intern_session_create",
    "intern_session_send",
    "intern_session_control",
    "intern_session_events_after",
  ]) assert.ok(bridge.includes(`"${command}"`), command);
  assert.ok(bridge.includes('listen<AppEvent>("runtime:event"'));
	assert.ok(app.includes("nativeIntern.createSession"));
	assert.ok(!app.includes('setSelectedTargetId("intern-sync")'));
	assert.ok(!app.includes('setSelectedTargetId("intern-async")'));
  assert.ok(app.includes("nativeIntern.eventsAfter"));
  assert.ok(app.includes("appEventToRuntimeEvent"));
});

test("stale Tauri binaries produce an actionable restart message", () => {
  const app = read("App.tsx");
  assert.ok(app.includes("Desktop backend was updated; fully quit and reopen Synth Desktop."));
  assert.ok(app.includes("unknown command"));
});
