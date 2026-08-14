import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (path) => readFileSync(join(appRoot, path), "utf8");

test("Settings exposes Context and moves the existing subagent control", () => {
  const settings = read("src/renderer/src/components/SettingsPage.tsx");
  assert.match(settings, /id: "context", label: "Context"/);
  assert.match(settings, /ContextSettings subagents={<MultiAgentModelSettings \/>}/);
  const modelsBlock = settings.slice(settings.indexOf('section === "models"'), settings.indexOf('section === "context"'));
  assert.doesNotMatch(modelsBlock, /<MultiAgentModelSettings/);
});
test("Context controls call native mutations instead of updating decorative preferences", () => {
  const context = read("src/renderer/src/components/ContextSettings.tsx");
  for (const operation of ["updateWorkspaceAgents", "updateSkill", "updateMcpGroup", "installCookbooks", "setCookbooksEnabled", "uninstallCookbooks"]) {
    assert.match(context, new RegExp(`bridges\\.context!?\\.${operation}`), operation);
  }
  assert.match(context, /runs\/ is never checked out/);
});

test("Context uses one modal editor and keeps editors out of the page scroll", () => {
  const context = read("src/renderer/src/components/ContextSettings.tsx");
  assert.match(context, /function ContextEditorDialog/);
  assert.match(context, /showModal\(\)/);
  assert.doesNotMatch(context, /context-line-numbers/);
  assert.match(context, /context-advanced/);
  assert.match(context, /Not configured/);
  assert.match(context, /activeSkill\.name}\/SKILL\.md/);
});

test("new Codex homes apply skill, cookbook, and MCP context gates", () => {
  const home = read("src-tauri/src/session/codex/home.rs");
  assert.match(home, /context::skill_enabled/);
  assert.match(home, /context::cookbook_skill/);
  assert.match(home, /context::mcp_group_enabled\("bundled"\)/);
  assert.match(home, /home\.join\("AGENTS\.md"\)/);
});
