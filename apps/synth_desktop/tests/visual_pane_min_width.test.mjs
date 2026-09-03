import assert from "node:assert/strict";
import { mkdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { buildSync } from "esbuild";

const testsDir = dirname(fileURLToPath(import.meta.url));
const appRoot = join(testsDir, "..");
const rendererRoot = join(appRoot, "src/renderer/src");

test("the visual pane keeps the 320px certification floor", () => {
  const css = readFileSync(
    join(dirname(fileURLToPath(import.meta.url)), "../src/renderer/src/styles/app.css"),
    "utf8"
  );
  assert.match(css, /\.visual-pane\s*\{[^}]*min-width:\s*320px/s);
  assert.match(css, /minmax\(320px,\s*min\(var\(--visual-pane-width/);
  assert.match(
    css,
    /\.workbench\.with-side-panel\.with-visual\s*\{[^}]*minmax\(320px,\s*min\(var\(--visual-pane-width/s
  );
  assert.match(
    css,
    /\.workbench\.with-side-panel\.with-visual\s*\{[^}]*minmax\(320px,\s*min\(var\(--visual-pane-width[^}]*minmax\(260px,\s*min\(var\(--side-panel-width/s
  );
  assert.match(css, /\.inventory-workbench\.with-visual \.visual-pane\s*\{[^}]*min-width:\s*320px/s);
  assert.match(
    css,
    /\.visual-pane-body\s*\{[^}]*container-type:\s*inline-size;[^}]*container-name:\s*visual-pane;/s
  );
  assert.match(css, /\.visual-pane:not\(\.visual-pane-expanded\) \.visual-pane-head\s*\{[^}]*flex-direction:\s*row/s);
  assert.match(css, /\.visual-pane:not\(\.visual-pane-expanded\) \.visual-pane-title\s*\{[^}]*text-overflow:\s*ellipsis/s);
  assert.match(css, /\.visual-pane:not\(\.visual-pane-expanded\) \.trace-workbench-layout\s*\{[^}]*--tw-main-columns:\s*minmax\(0,\s*1fr\)/s);
  assert.match(css, /\.visual-pane:not\(\.visual-pane-expanded\) \.cv-overview-grid[\s\S]*grid-template-columns:\s*minmax\(0,\s*1fr\)/s);
});

test("the Visuals inventory is a compact list and its pane can consume nearly the full workspace", () => {
  const css = readFileSync(join(rendererRoot, "styles/app.css"), "utf8");
  const routes = readFileSync(join(rendererRoot, "routes.tsx"), "utf8");
  assert.match(css, /\.inventory-workbench\.with-visual \.visual-pane\s*\{[^}]*var\(--visual-pane-width, 720px\)[^}]*calc\(100% - 167px\)/s);
  assert.match(css, /\.visuals-layout:not\(\.reports-layout\) > \.visuals-grid\s*\{[^}]*display:\s*flex;[^}]*flex-direction:\s*column/s);
  assert.match(css, /\.visuals-layout:not\(\.reports-layout\) > \.visuals-grid > \.visuals-card\s*\{[^}]*grid-template-columns:/s);
  assert.match(routes, /chatRoute \? \(showSidePanel \? 680 : 260\) : 160/);
});

test("the unified workbench side panel preserves a draggable boundary", () => {
  const routes = readFileSync(join(rendererRoot, "routes.tsx"), "utf8");
  const css = readFileSync(join(rendererRoot, "styles/app.css"), "utf8");
  assert.match(routes, /"--side-panel-width": `\$\{inventoryContainerWidth\}px`/);
  assert.match(routes, /ariaLabel="Resize workbench side panel"/);
  assert.match(routes, /<PaneResizeHandle[\s\S]*ariaLabel="Resize workbench side panel"[\s\S]*<WorkbenchSidePanel/);
  assert.match(css, /\.workbench\.with-side-panel\s*\{[^}]*7px[^}]*var\(--side-panel-width, 420px\)/s);
});

test("narrow windows cap the visual pane at min(40vw, persisted) then overlay via compact-workbench", () => {
  const css = readFileSync(
    join(dirname(fileURLToPath(import.meta.url)), "../src/renderer/src/styles/app.css"),
    "utf8"
  );
  const tokens = readFileSync(
    join(dirname(fileURLToPath(import.meta.url)), "../src/renderer/src/styles/tokens.css"),
    "utf8"
  );
  const shell = readFileSync(
    join(dirname(fileURLToPath(import.meta.url)), "../src/renderer/src/hooks/useShellLayout.ts"),
    "utf8"
  );
  assert.match(tokens, /--visual-pane-compact-max:\s*40vw/);
  assert.match(tokens, /html\.sidebar-hidden/);
  assert.match(css, /@media \(max-width: 1100px\)/);
  assert.match(
    css,
    /minmax\(320px,\s*min\(var\(--visual-pane-compact-max,\s*40vw\),\s*var\(--visual-pane-width/
  );
  assert.match(css, /min\(var\(--visual-pane-compact-max,\s*40vw\),\s*var\(--visual-pane-width/);
  assert.match(css, /@media \(max-width: 860px\)/);
  assert.match(css, /html\.compact-workbench/);
  assert.match(
    css,
    /html\.compact-workbench[\s\S]*\.visual-pane:not\(\.visual-pane-expanded\)[\s\S]*position:\s*absolute/s
  );
  assert.match(css, /\.visuals-page header[\s\S]*flex-wrap:\s*nowrap/s);
  assert.match(css, /\.visuals-tabs button[\s\S]*white-space:\s*nowrap/s);
  assert.match(
    css,
    /\.chat-transcript-scroll\s*\{[^}]*margin-bottom:\s*var\(--composer-clearance/s
  );
  assert.match(css, /html\.sidebar-hidden is the existing sidebar toggle/);
  assert.match(css, /html\.visual-expanded is Expand visual/);
  assert.match(css, /html\.visual-expanded \.sidebar/);
  assert.match(
    css,
    /html\.visual-expanded \.composer-dock\s*\{[^}]*display:\s*none/s
  );
  assert.match(
    css,
    /\.workbench\.with-side-panel:has\(\.visual-pane-expanded\)\s*\{[^}]*grid-template-columns:\s*minmax\(0,\s*1fr\)/s
  );
  assert.match(
    css,
    /\.workbench\.with-side-panel:has\(\.visual-pane-expanded\) > \.chat-transcript,[\s\S]*> \.pane-resize-handle\s*\{[^}]*display:\s*none/s
  );
  assert.doesNotMatch(css, /html\.compact-workbench\.sidebar-hidden/);
  assert.match(shell, /classList\.toggle\("compact-workbench"/);
  assert.match(shell, /matchMedia\("\(max-width: 860px\)"\)/);
});

test("an 820px stacked workbench still keeps a 320px visual floor so the composer stays in the transcript column", () => {
  const css = readFileSync(
    join(dirname(fileURLToPath(import.meta.url)), "../src/renderer/src/styles/app.css"),
    "utf8"
  );
  const workbenchRule = css.match(
    /\.workbench\.with-visual\s*\{\s*grid-template-columns:[^}]+\}/s
  )?.[0] ?? "";
  assert.match(workbenchRule, /minmax\(320px,\s*1fr\)/);
  assert.match(workbenchRule, /minmax\(320px,\s*min\(var\(--visual-pane-width/);
  const transcriptPlusGutterPlusPane = 320 + 7 + 320;
  assert.ok(transcriptPlusGutterPlusPane <= 820, "320+7+320 must fit the 820px compact width");
});

test("bombadil grouped Craftax uses a bundled fixture stream", () => {
  const harness = readFileSync(
    join(dirname(fileURLToPath(import.meta.url)), "bombadil/run.mjs"),
    "utf8"
  );
  assert.match(harness, /kind: "fixture", source: "examples\/events\.json"/);
  assert.equal(harness.includes("data: { events: [] }"), false);
});

test("routes.tsx keeps the window host and tabbed dock visual hosts distinct", () => {
  const source = readFileSync(
    join(dirname(fileURLToPath(import.meta.url)), "../src/renderer/src/routes.tsx"),
    "utf8"
  );
  assert.equal((source.match(/<VisualPane/g) ?? []).length, 2);
  assert.match(source, /key="window-visual-host"/);
	assert.match(source, /key=\{`dock-visual-\$\{artifact\.id\}`\}/);
	assert.match(source, /\.\.\.openVisualTabs\.map/);
  assert.match(source, /view\.kind === "reports"/);
  assert.match(source, /const paneHost = inventoryHost \|\| chatRoute \|\| settingsWithPane/);
  assert.match(source, /<ReportsPage initialReportId=\{view\.reportId\} onBack=\{leaveReports\} \/>/);
  assert.doesNotMatch(source, /Chat still remounts/);
  assert.doesNotMatch(source, /onBack=\{\(\) => openChat/);
  assert.doesNotMatch(source, /crypto\.randomUUID\(\)/);
	// The standalone pane renders for a chat without the dock and for the
	// inventory surfaces that own visuals; independent destinations never
	// inherit a previously opened artifact.
	assert.match(source, /const inventoryOwnsVisualPane = view\.kind === "visuals"/);
	assert.match(source, /const visualPaneVisible = Boolean\(openArtifact && \(\s*\(chatRoute && !showSidePanel\)\s*\|\| inventoryOwnsVisualPane\s*\)\)/);
	assert.match(source, /id: `visual:\$\{artifact\.id\}`/);
	assert.match(source, /activeTabId=\{sidePanelTab === "visual"/);
	assert.match(source, /setSidePanelTab\("visual"\)/);

  const controller = readFileSync(
    join(dirname(fileURLToPath(import.meta.url)), "../src/renderer/src/hooks/useAppController.ts"),
    "utf8"
  );
  assert.match(controller, /view\.kind === "reports"/);
  assert.match(controller, /view\.kind === "settings" && Boolean\(openArtifactId\)/);
});

test("Escape hierarchy intercepts labeling, inspector, and expanded before pane close, and Back restores origin", () => {
  const root = join(dirname(fileURLToPath(import.meta.url)), "../src/renderer/src");
  const host = readFileSync(join(root, "components/VisualHost.tsx"), "utf8");
  const escapeHandler =
    host.match(/if \(event\.key !== "Escape"\) return;[\s\S]*?addEventListener\("keydown"/)?.[0] ?? "";
  assert.match(escapeHandler, /if \(labeling\)/);
  assert.match(escapeHandler, /if \(inspectorOpen\)/);
  assert.match(escapeHandler, /if \(expanded\)/);
  assert.match(escapeHandler, /preventDefault/);
  assert.match(escapeHandler, /stopPropagation/);
  assert.ok(
    escapeHandler.indexOf("if (labeling)") < escapeHandler.indexOf("if (inspectorOpen)") &&
      escapeHandler.indexOf("if (inspectorOpen)") < escapeHandler.indexOf("if (expanded)"),
    "labeling and inspector must close before expanded restore"
  );
  assert.doesNotMatch(escapeHandler, /onClose|dispatchVisualRevision|type: "close"/);
  assert.match(host, /cancelLabeling/);
  assert.match(host, /moreButtonRef\.current\?\.focus\(\)/);
  assert.match(host, /key="window-visual-host"|moreButtonRef/);
  assert.match(host, /classList\.toggle\("visual-expanded"/);
  assert.match(host, /<VisualPane|export function VisualPane/);

  const controller = readFileSync(join(root, "hooks/useAppController.ts"), "utf8");
  const paneEscape =
    controller.match(/if \(e\.key === "Escape"\) \{[\s\S]*?dispatchVisualRevision\(\{ type: "close" \}\)/)?.[0] ?? "";
  assert.match(paneEscape, /defaultPrevented/);

  assert.match(controller, /sidePanelTab === "diagnostics"/);
  assert.match(controller, /sidePanelTab === "outputs"/);
  assert.match(controller, /sidePanelTab === "trace"/);

  const routes = readFileSync(join(root, "routes.tsx"), "utf8");
  assert.match(routes, /id: "diagnostics"/);
  assert.match(routes, /tabId === "diagnostics"/);
  assert.match(routes, /const leaveInventory = \(fallbackOrigin: MainView \| null\)/);
  assert.match(routes, /key="window-visual-host"/);
  assert.match(routes, /<ReportsPage initialReportId=\{view\.reportId\} onBack=\{leaveReports\} \/>/);
  assert.match(routes, /originStackRef/);
  assert.match(routes, /sidePanelOpen: showSidePanel/);
  assert.match(routes, /setSidePanelTab\(frame\.layout\.sidePanelTab\)/);
  assert.match(routes, /origin\?\.kind === "chat"/);
  assert.match(routes, /openChat\(origin\.chatId\)/);
  assert.match(routes, /"research-log"/);
  assert.match(routes, /history\.replaceState/);
  const leaveBacks = routes.match(/onBack=\{\(\) => leaveInventory\(inventoryOriginRef\.current\)\}/g) ?? [];
  assert.ok(
    leaveBacks.length >= 5,
    `Settings/Visuals/Experiments/Optimizers/Data Back must use leaveInventory; found ${leaveBacks.length}`
  );
  assert.match(routes, /<SettingsPage[\s\S]{0,900}onBack=\{\(\) => leaveInventory\(inventoryOriginRef\.current\)\}/);
  assert.match(routes, /<VisualsPage[\s\S]{0,900}onBack=\{\(\) => leaveInventory\(inventoryOriginRef\.current\)\}/);
  assert.doesNotMatch(routes, /onBack=\{\(\) => openChat/);
  assert.doesNotMatch(routes, /<CloudDesk\b/);
  const chatRestore = routes.indexOf('origin?.kind === "chat"');
  const landingFallback = routes.indexOf('setView({ kind: "landing" })', routes.indexOf("const leaveInventory"));
  assert.ok(chatRestore >= 0 && landingFallback > chatRestore, "chat origin must restore before landing fallback");
});

const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });
const compiledHandle = join(compiledDir, "PaneResizeHandle.mjs");
buildSync({
  entryPoints: [join(rendererRoot, "components/PaneResizeHandle.tsx")],
  bundle: true,
  format: "esm",
  target: "es2022",
  platform: "neutral",
  jsx: "automatic",
  outfile: compiledHandle,
  external: ["react", "react/jsx-runtime"]
});
const {
  PANE_KEYBOARD_STEP_PX,
  PANE_KEYBOARD_SHIFT_STEP_PX,
  applyKeyboardResize,
  keyboardWidthDelta,
  paneKeyboardValueText,
  realizedPaneWidth
} = await import(pathToFileURL(compiledHandle).href);

test("aria-valuenow reports realized CSS-pixel width after min/max, not the requested drag value", () => {
  assert.equal(realizedPaneWidth(546, 320, 720, 320), 320);
  assert.equal(realizedPaneWidth(420, 320, 720, 418.4), 418);
  assert.equal(realizedPaneWidth(560, 280, 960, null), 560);
  assert.equal(realizedPaneWidth(120, 280, 960), 280);
  assert.equal(realizedPaneWidth(2000, 280, 960), 960);
  const handle = readFileSync(join(rendererRoot, "components/PaneResizeHandle.tsx"), "utf8");
  assert.match(handle, /aria-valuenow=\{reported\}/);
  assert.match(handle, /persistRealized/);
  assert.match(handle, /namedPaneElement/);
});

test("Left shrinks the named pane width for both the visual pane and the visuals list", () => {
  assert.equal(PANE_KEYBOARD_STEP_PX, 40);
  assert.equal(PANE_KEYBOARD_SHIFT_STEP_PX, 64);
  assert.equal(keyboardWidthDelta("ArrowLeft"), -40);
  assert.equal(keyboardWidthDelta("ArrowRight"), 40);
  assert.equal(keyboardWidthDelta("ArrowLeft", true), -64);
  assert.equal(keyboardWidthDelta("ArrowRight", true), 64);
  assert.equal(applyKeyboardResize({ key: "ArrowLeft", value: 420, min: 320, max: 720 }), 380);
  assert.equal(applyKeyboardResize({ key: "ArrowLeft", value: 560, min: 280, max: 960 }), 520);
  assert.equal(applyKeyboardResize({ key: "ArrowRight", value: 420, min: 320, max: 720 }), 460);
  assert.equal(applyKeyboardResize({ key: "Home", value: 420, min: 320, max: 720 }), 320);
  assert.equal(applyKeyboardResize({ key: "End", value: 420, min: 320, max: 720 }), 720);
  assert.equal(applyKeyboardResize({ key: "ArrowLeft", shiftKey: true, value: 560, min: 280, max: 960 }), 496);
  assert.match(paneKeyboardValueText(420), /40 pixels/);
  assert.match(paneKeyboardValueText(420), /Home and End/);
  const handle = readFileSync(join(rendererRoot, "components/PaneResizeHandle.tsx"), "utf8");
  assert.doesNotMatch(
    handle,
    /direction === "sidebar" \|\| direction === "primary"[\s\S]{0,200}ArrowLeft \? delta/
  );
});
