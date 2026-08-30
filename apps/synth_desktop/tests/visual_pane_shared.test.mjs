import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const root = join(dirname(fileURLToPath(import.meta.url)), "../src/renderer/src");
const visualHost = readFileSync(join(root, "components/VisualHost.tsx"), "utf8");
const visualChrome = readFileSync(join(root, "components/VisualPaneChrome.tsx"), "utf8");
const css = readFileSync(join(root, "styles/app.css"), "utf8");
const paneSource = visualHost.includes("export function VisualPane")
  ? visualHost.slice(visualHost.indexOf("const SHARED_URL_INVALID"))
  : visualHost;
const openShared = paneSource.match(/async function openSharedUrl\(\) \{[\s\S]*?\n\t\}/)?.[0] ?? "";

test("Open shared stays disabled unless the pasted value is an http(s) URL", () => {
  assert.match(paneSource, /function isSharedArtifactUrl/);
  assert.match(paneSource, /parsed\.protocol === "http:" \|\| parsed\.protocol === "https:"/);
  assert.match(visualChrome, /disabled=\{!sharedUrlValid \|\| busy\}/);
  assert.doesNotMatch(
    visualChrome,
    /disabled=\{!sharedUrl\.trim\(\)/
  );
  assert.match(paneSource, /Enter an http\(s\) private artifact URL/);
  assert.match(visualChrome, /role="alert"/);
  assert.match(visualChrome, /data-testid="visual-artifact-inspector"/);
  assert.match(visualChrome, /aria-haspopup="dialog"/);
});

test("v0.9 visual chrome keeps provenance, sharing, and diagnostics behind the inspector", () => {
  const header = visualChrome.match(/<header className="visual-pane-head">[\s\S]*?<\/header>/)?.[0] ?? "";
  const inspectorIndex = header.indexOf("visual-artifact-inspector");
  assert.ok(inspectorIndex > 0, "the artifact inspector is mounted from the compact header");
  const persistentHeader = header.slice(0, inspectorIndex);
  assert.doesNotMatch(persistentHeader, /Paste private artifact URL|visual-pane-identity|visual-pane-ops/);
  assert.match(persistentHeader, /artifact\.title/);
  assert.match(persistentHeader, /toggle-visual-expand/);
  assert.match(persistentHeader, /visual-pane-more/);
  assert.match(visualChrome.slice(inspectorIndex), /Paste private artifact URL/);
  assert.match(visualChrome.slice(inspectorIndex), /visual-pane-identity/);
  assert.match(visualChrome.slice(inspectorIndex), /<summary>Debug<\/summary>/);
});

test("healthy transport states stay diagnostic-only", () => {
  assert.match(visualHost, /const degradedConnection = \["reconnecting", "failed", "interrupted"\]\.includes\(connectionState\)/);
  assert.doesNotMatch(visualHost, /\{connectionState\}<\/p>/);
  assert.match(visualHost, /data-connection-state=\{connectionState\}/);
});

test("label placement renders a pin at stored percentages and stacks the toolbar", () => {
  assert.match(paneSource, /data-testid="visual-label-pin"/);
  assert.match(paneSource, /left: `\$\{labelPoint\.x \* 100\}%`/);
  assert.match(paneSource, /top: `\$\{labelPoint\.y \* 100\}%`/);
  assert.match(paneSource, /visual-label-form-stack/);
  assert.match(paneSource, /visual-label-status/);
  assert.match(paneSource, /visual-label-actions/);
  assert.match(css, /\.visual-label-pin\s*\{[^}]*position:\s*absolute/s);
  assert.match(css, /\.visual-label-pin\s*\{[^}]*var\(--color-accent\)/s);
  assert.match(css, /\.visual-label-form-stack\s*\{[^}]*flex-direction:\s*column/s);
});

test("shared-open failures stay public and never append (internal)", () => {
  assert.match(openShared, /publicError\(reason/);
  assert.doesNotMatch(openShared, /\(internal\)/);
  assert.doesNotMatch(paneSource.match(/const SHARED_URL_INVALID[\s\S]*openSharedUrl/)?.[0] ?? openShared, /\(internal\)/);
});

test("Close visual restores focus to the workbench landmark, not the detached expand control", () => {
  assert.match(paneSource, /closeVisualPane/);
  assert.match(paneSource, /restoreFocusAfterVisualPaneClose/);
  assert.match(visualHost, /data-testid="visuals-grid"/);
  assert.match(visualHost, /main\.main-pane/);
  const closeFn = paneSource.match(/function closeVisualPane\(\) \{[\s\S]*?\n\t\}/)?.[0] ?? "";
  assert.match(closeFn, /onClose\(\)/);
  assert.doesNotMatch(closeFn, /toggle-visual-expand/);
  assert.doesNotMatch(closeFn, /getElementById/);
  assert.doesNotMatch(visualHost.match(/function restoreFocusAfterVisualPaneClose[\s\S]*?\n\}/)?.[0] ?? "", /toggle-visual-expand/);
});
