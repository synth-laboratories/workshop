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
    "components/DataPage.tsx",
    "components/CloudDesk.tsx",
    // The Plugins section's test ids are declared once as data and rendered
    // through `data-testid={entry.testId}`, so the declaration is the surface.
    "runtime/pluginNav.ts",
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
    // Accept a declared id as well as an inline attribute: ids rendered from
    // data still have to exist, and quote style is not the invariant.
    assert.ok(
      blob.includes(`data-testid="${id}"`) || blob.includes(`'${id}'`) || blob.includes(`"${id}"`),
      id
    );
  }
});

test("installed desktop authorizes its declared window drag regions", () => {
  const app = read("App.tsx");
  const sidebar = read("components/Sidebar.tsx");
  const permissions = readFileSync(join(appRoot, "src-tauri/capabilities/default.json"), "utf8");
  assert.match(`${app}\n${sidebar}`, /data-tauri-drag-region/);
  assert.match(permissions, /core:window:allow-start-dragging/);
});

test("plugin navigation announces the active page and hides impossible pre-install actions", () => {
  const sidebar = read("components/Sidebar.tsx");
  const optimizers = read("components/OptimizersPage.tsx");
  assert.match(sidebar, /aria-current=\{active \? "page" : undefined\}/);
  assert.match(optimizers, /operation: "enable"[\s\S]*status\.phase !== "not_installed" && !status\.enabled/);
  assert.match(optimizers, /operation: "disable"[\s\S]*status\.phase !== "not_installed" && status\.enabled/);
});

test("hosted CISPO requires an explicit compatible retained SFT training state", () => {
  const optimizers = read("components/OptimizersPage.tsx");
  assert.match(optimizers, /data-testid="hosted-cispo-warm-start"/);
  assert.match(optimizers, /optimizerAlgorithm: "sft"/);
  assert.match(optimizers, /checkpointKind: "training"/);
  assert.match(optimizers, /provider: "tinker"/);
  assert.match(optimizers, /selectedWarmStart\.baseModel !== trainingModel/);
  assert.match(optimizers, /algorithm_config\.initial_state_path/);
  assert.match(optimizers, /never defaults to latest/);
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
  assert.ok(bridge.includes("clientMessageId: options?.clientMessageId"));
});

test("v0.1 exposes remote Muse Spark while excluding local Muse Glimmer, GGUF, and DFlash", () => {
  const files = [
    "App.tsx",
    "hooks/useAppController.ts",
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
  assert.match(modelSurface, /Muse Spark 1\.2/);
  assert.match(modelSurface, /openrouter-muse-spark/);
  assert.doesNotMatch(modelSurface, /Muse Glimmer|muse-glimmer|GGUF|DFlash/i);
});

test("model knobs are registered once and consumed without model-specific UI or transport branches", () => {
  const registry = read("runtime/modelCapabilities.ts");
  const composer = read("components/Composer.tsx");
  const controller = read("hooks/useAppController.ts");
  for (const target of ["local-laguna", "openrouter-luna", "openrouter-laguna-s", "synth-cloud-laguna-s"]) {
    assert.ok(registry.includes(`targetId: "${target}"`), target);
  }
  assert.ok(composer.includes("modelCapabilitiesForTarget(state.selectedTargetId)"));
  assert.ok(composer.includes("modelCapabilities?.knobs.map"));
  assert.ok(!composer.includes('state.selectedTargetId === "openrouter-luna"'));
  assert.ok(!composer.includes('state.selectedTargetId === "openrouter-laguna-s"'));
  // Match the call, not the caller's local name: the point is that effort comes
  // from the registry helper rather than a per-model branch at the send site.
  assert.match(controller, /turnStartEffortForExecutionTarget\(\s*\w+\s*,\s*modelKnobValues\s*\)/);
  assert.ok(!controller.includes('session.target.model === "openai/gpt-5.6-luna"'));
  assert.ok(!controller.includes('session.target.model === "poolside/laguna-s-2.1"'));
});

test("renderer keeps the HTTP runtime bridge browser-only", () => {
  const bridge = read("runtime/desktopBridge.ts");
  assert.ok(bridge.includes("browserRuntimeBridge"));
  assert.ok(bridge.includes("if (!isTauri && import.meta.env.DEV) window.synthRuntime ??="));
  assert.ok(!bridge.includes('invoke<T>("runtime_request"'));
  assert.ok(!bridge.includes('"runtime_subscribe"'));
});

test("renderer uses Tauri commands for desktop capabilities", () => {
  const bridge = read("runtime/desktopBridge.ts");
  const protocol = read("generated/protocol.ts");
  assert.ok(protocol.includes("workspaceChooseDirectory"));
  assert.ok(protocol.includes("lagunaGetStatus"));
  assert.ok(protocol.includes("internSessionEventsAfter"));
  assert.ok(protocol.includes("codexSessionsList"));
  assert.ok(!bridge.includes('"core_projects_list"'));
  assert.ok(bridge.includes("EVENT_CHANNELS.RUNTIME") || bridge.includes("listen<AppEvent>(EVENT_CHANNELS.RUNTIME"));
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
  const controller = read("hooks/useAppController.ts");
  const codexBridge = read("hooks/useCodexEventBridge.ts");
  const nativeCodex = read("runtime/nativeCodex.ts");
  assert.ok(codexBridge.includes("allocateNativeSequence(event.sessionId)"));
  assert.ok(controller.includes("allocateNativeSequence(sessionId)"));
  assert.ok(controller.includes('persisted.filter((session) => session.status !== "closed").map(restoreCodexSession)'));
	assert.ok(controller.includes("await nativeCodex.start("));
	assert.ok(controller.includes("threadId: typeof session.metadata.threadId") || read("runtime/codexTurn.ts").includes("threadId: typeof session.metadata.threadId"));
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
  const controller = read("hooks/useAppController.ts");
  assert.ok(controller.includes('throw new Error("Enter an objective to start an Intern session")'));
  assert.ok(controller.includes("objective: internObjective!"));
  assert.ok(controller.includes("objectiveConsumed"));
  assert.ok(controller.includes("if (!ensured.objectiveConsumed)"));
  // v0.1 removal contract: the dormant creation path above may remain, but the
  // entry points that selected an Intern target must not. See PRODUCT-NO-INTERN-V0P1.
  assert.ok(!controller.includes('setSelectedTargetId("intern-sync")'));
  assert.ok(!controller.includes('setSelectedTargetId("intern-async")'));
});

test("native Intern projection changes refresh the renderer session cache", () => {
  const foreignBridge = read("hooks/useForeignSessionEventBridge.ts");
  assert.ok(foreignBridge.includes('event.eventKind === "intern.projection_updated"'));
  assert.ok(foreignBridge.includes('event.eventKind === "session.updated"'));
  assert.ok(foreignBridge.includes('event.eventKind === "command.resolved"'));
});

test("migrated demo async pins cannot mask the Rust singleton", () => {
  // sessionView is the single place the singleton is enforced (per-session slice).
  const sessionView = read("runtime/sessionView.ts");
  assert.ok(sessionView.includes('isRustIntern: session.metadata.runtime === "rust-intern"'));
  assert.ok(sessionView.includes("if (asyncIntern && !slice.isRustIntern) continue"));
});

test("renderer exposes CoreRuntime visual registry bridge commands", () => {
  const bridge = read("runtime/desktopBridge.ts");
  const protocol = read("generated/protocol.ts");
  const constants = read("bridge/protocolConstants.ts");
  assert.ok(protocol.includes("visualsList"));
  assert.ok(protocol.includes("visualsCreate"));
  assert.ok(protocol.includes("visualsShow"));
  assert.ok(bridge.includes("window.synthVisuals ??="));
  assert.ok(constants.includes("VISUAL_SHOW"));
});

test("Rust run counts remain projected without a Runtime Settings surface", () => {
  const controller = read("hooks/useAppController.ts");
  const settings = read("components/SettingsPage.tsx");
  assert.ok(controller.includes("runs: core.runCount"));
  assert.ok(!settings.includes('id: "runtime"'));
  assert.ok(!settings.includes("health?.dataStore?.runs"));
  assert.ok(!controller.includes("runs: 0"));
});

test("v0.2 Intern bridge remains typed while v0.1 creation stays unreachable", () => {
  const bridge = read("runtime/desktopBridge.ts");
  const protocol = read("generated/protocol.ts");
  const controller = read("hooks/useAppController.ts");
  const foreignBridge = read("hooks/useForeignSessionEventBridge.ts");
  for (const command of [
    "internSessionsList",
    "internSessionCreate",
    "internSessionSend",
    "internSessionControl",
    "internSessionEventsAfter",
  ]) assert.ok(protocol.includes(command), command);
  assert.ok(bridge.includes("EVENT_CHANNELS.RUNTIME") || bridge.includes("listen<AppEvent>(EVENT_CHANNELS.RUNTIME"));
	assert.ok(controller.includes("nativeIntern.createSession"));
	assert.ok(!controller.includes('setSelectedTargetId("intern-sync")'));
	assert.ok(!controller.includes('setSelectedTargetId("intern-async")'));
  assert.ok(foreignBridge.includes("nativeIntern.eventsAfter"));
  assert.ok(controller.includes("appEventToRuntimeEvent") || foreignBridge.includes("appEventToRuntimeEvent"));
});

test("stale Tauri binaries produce an actionable restart message", () => {
  const codexTurn = read("runtime/codexTurn.ts");
  assert.ok(codexTurn.includes("Desktop backend was updated; fully quit and reopen Synth Desktop."));
  assert.ok(codexTurn.includes("unknown command"));
});
