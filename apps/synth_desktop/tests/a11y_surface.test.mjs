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
  ]) {
    assert.ok(blob.includes(`data-testid="${id}"`) || blob.includes(`'${id}'`), id);
  }
});

test("execution targets include Laguna local + OpenRouter Luna/Laguna + Intern", () => {
  const types = read("types/landing.ts");
  assert.ok(types.includes("local-laguna") || types.includes("Laguna XS"));
  assert.ok(types.includes("openrouter") || types.includes("kimi-k2.5") || types.includes("Luna"));
  assert.ok(types.includes("laguna-s-2.1") || types.includes("Laguna S"));
  assert.ok(types.includes("intern"));
});

test("renderer installs a Tauri runtime bridge with a browser fallback", () => {
  const bridge = read("runtime/desktopBridge.ts");
  assert.ok(bridge.includes('invoke<T>("runtime_request"'));
  assert.ok(bridge.includes('listen<RuntimeEventEnvelope>("runtime:subscription"'));
  assert.ok(bridge.includes("browserRuntimeBridge"));
  assert.ok(bridge.includes("window.synthRuntime ??="));
});

test("renderer uses Tauri commands for desktop capabilities", () => {
  const bridge = read("runtime/desktopBridge.ts");
  assert.ok(bridge.includes('"project_choose_directory"'));
  assert.ok(bridge.includes('"laguna_get_status"'));
  assert.ok(bridge.includes('"runtime_subscribe"'));
  assert.ok(bridge.includes('"runtime_unsubscribe"'));
  assert.ok(bridge.includes('{ subscriptionId }'));
  assert.ok(bridge.includes('"codex_sessions_list"'));
});

test("native Codex sessions use one sequence allocator and restore persisted sessions", () => {
  const app = read("App.tsx");
  const nativeCodex = read("runtime/nativeCodex.ts");
  assert.ok(app.includes("allocateNativeSequence(event.sessionId)"));
  assert.ok(app.includes("allocateNativeSequence(sessionId)"));
  assert.ok(app.includes('persisted.filter((session) => session.status !== "closed").map(restoreCodexSession)'));
  assert.ok(app.includes("await nativeCodex.start({"));
  assert.ok(app.includes("threadId: typeof session.metadata.threadId"));
  assert.ok(nativeCodex.includes('eventKind = "run.failed"'));
  assert.ok(nativeCodex.includes('eventKind = "run.cancelled"'));
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
  assert.ok(composer.includes("Configure SYNTH_API_KEY or start with SYNTH_INTERN_DEMO=1"));
  assert.ok(composer.includes('state.internMode !== "unconfigured"'));
  assert.ok(settings.includes("SYNTH_API_KEY=… npm run dev:desktop"));
  assert.ok(settings.includes("SYNTH_INTERN_DEMO=1 npm run dev:desktop"));
});
