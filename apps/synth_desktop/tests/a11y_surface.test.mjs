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

test("preload exposes synthRuntime bridge", () => {
  const preload = readFileSync(join(appRoot, "src/preload/index.ts"), "utf8");
  assert.ok(preload.includes("synthRuntime"));
  assert.ok(preload.includes("subscribe"));
});

test("main process spawns local runtime daemon", () => {
  const main = readFileSync(join(appRoot, "src/main/index.ts"), "utf8");
  assert.ok(main.includes("synth_local_runtime"));
  assert.ok(main.includes("runtime:request"));
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
