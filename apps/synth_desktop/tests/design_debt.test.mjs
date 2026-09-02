/**
 * Static design-debt detectors. Failures here mean a known stub/smell is gone
 * (good — update the assertion) or a new one appeared (bad — investigate).
 *
 * Expected-fail style: we assert the smell *exists* so CI stays green while
 * documenting debt; flip to assert absence when the design is fixed.
 */
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const renderer = join(appRoot, "src/renderer/src");

function read(rel) {
	return readFileSync(join(renderer, rel), "utf8");
}

test("titlebar is trimmed; account entry lives in the sidebar footer", () => {
	const app = read("App.tsx");
	assert.doesNotMatch(app, /Account — stub/);
	assert.doesNotMatch(app, /data-testid="open-account-settings"/);
	assert.doesNotMatch(app, /data-testid="open-models-settings"/);
	assert.doesNotMatch(app, /data-testid="runtime-status"/);
	assert.doesNotMatch(app, /avatar-btn/);
	assert.match(app, /setView\(\{ kind: "settings", section: "account" \}\)/);
	assert.doesNotMatch(app, /Account menu — stub/);
	assert.doesNotMatch(app, /Downloads — stub/);
	assert.doesNotMatch(app, /Expand — stub/);
	assert.doesNotMatch(app, /aria-label="Account menu"/);
	assert.doesNotMatch(app, /aria-label="Expand"/);
});

test("Runtime settings no longer expose legacy Python migration UI", () => {
	const settings = read("components/SettingsPage.tsx");
	assert.doesNotMatch(settings, /LegacyMigrationSettings/);
	assert.doesNotMatch(settings, /runtime\.sqlite3/);
});

test("Landing Set up an agent card is removed; Laguna reload stays a typed bridge", () => {
	const app = read("App.tsx");
	const landing = read("components/LandingPage.tsx");
	assert.doesNotMatch(app, /Set up agent — stub/);
	assert.doesNotMatch(app, /onSetupAgent/);
	assert.doesNotMatch(landing, /quick-setup-agent/);
	assert.doesNotMatch(landing, /Set up an agent/);
	assert.doesNotMatch(landing, /onSetupAgent/);
	assert.match(app, /onReloadLaguna=\{c\.onReloadLaguna\}/);
	assert.doesNotMatch(app, /Reload Laguna — stub/);
	const bridge = read("runtime/desktopBridge.ts");
	assert.match(bridge, /fromGenerated\(spectaCommands\.lagunaReload\(\)\)/);
	const rust = readFileSync(join(appRoot, "src-tauri/src/lib.rs"), "utf8");
	assert.match(rust, /async fn laguna_reload/);
});

test("empty conversation keeps a quiet, icon-free model surface", () => {
	const app = read("App.tsx");
	const landing = read("components/LandingPage.tsx");
	const composer = read("components/Composer.tsx");
	const styles = read("styles/app.css");
	assert.match(app, /showTabIcon=\{c\.view\.kind !== "landing"\}/);
	assert.match(app, /showCloseTab=\{c\.view\.kind !== "landing"\}/);
	assert.match(app, /c\.view\.kind === "landing" \? "New conversation"/);
	assert.doesNotMatch(landing, /ProviderMark|providerMarkForTarget/);
	assert.match(landing, /Start a new conversation with Workshop/);
	assert.doesNotMatch(landing.slice(landing.indexOf("export function LandingPage")), /<ModelPicker/);
	assert.doesNotMatch(composer, /ProviderMark|providerMarkForTarget/);
	assert.match(composer, /data-testid="composer-add-menu-trigger"/);
	assert.match(composer, /Commands and skills/);
	const landingRule = styles.match(/\.landing \{[\s\S]*?\n\}/)?.[0] ?? "";
	assert.doesNotMatch(landingRule, /background-image|background-size/);
	assert.match(styles, /\.landing \.composer,[\s\S]*?max-width: 980px/);
	assert.match(styles, /\.landing \.composer \{[\s\S]*?min-height: 148px;[\s\S]*?border-radius:var\(--radius-composer\)/);
	assert.doesNotMatch(styles, /#9bb5ed/);
});

test("design debt: CloudDesk leave-safe is projection-driven from AsyncInternPin", () => {
	const desk = read("components/CloudDesk.tsx");
	assert.match(desk, /props\.intern\.leaveSafe === true/);
	assert.ok(!desk.includes("const leaveSafe = !isSync"));
});

test("App carries no stub copy and no mounted CloudDesk route", () => {
	const app = read("App.tsx");
	assert.ok(!/\bstub\b/i.test(app));
	// v0.1 removal contract: CloudDesk is the Intern surface and stays unmounted.
	// Its unknown-action honesty was App's `onCloudAction` toast; that handler
	// goes with the route and returns with it in v0.2, so nothing asserts it now.
	assert.ok(!/<CloudDesk\b/.test(app));
});

test("Async Intern Respond opens an intervention control instead of a stub toast", () => {
	const desk = read("components/CloudDesk.tsx");
	assert.match(desk, /data-testid="intern-intervention-input"/);
	assert.ok(!desk.includes('onAction("Provide input")'));
});

test("design debt: agent-authored analysis shell normalizes persisted type-block payloads", () => {
	const shell = readFileSync(join(appRoot, "../../visuals/families/analysis/analysis.visual.v1/shell.tsx"), "utf8");
	assert.match(shell, /normalizeBlock/);
	assert.match(shell, /block\.type/);
	assert.match(shell, /if \(kind === "note"\)/);
	// Malformed ranked-bars without items must not cast-and-map (CUA crash).
	assert.match(shell, /kind === "ranked-bars"/);
	assert.match(shell, /!Array\.isArray\(block\.items\)/);
});

test("composer approval policy control is wired and test-addressable", () => {
	const composer = read("components/Composer.tsx");
	assert.match(composer, /className="permission-select"/);
	const triggerLine = composer.split("\n").find((line) => line.includes('className="permission-select"')) ?? "";
	assert.ok(triggerLine.includes("onClick="));
	assert.ok(triggerLine.includes('data-testid="approval-mode-select"'));
	assert.match(composer, /data-testid="approval-mode-menu"/);
});

test("intended design: preview variants come from one template classifier, not inline heuristics", () => {
	// The debt this used to track is paid: the Craftax substring heuristic was
	// copied between VisualHost and sessionView, and harbor/live-eval surfaces
	// fell to "generic" by omission. One classifier now decides, and both
	// consumers read it.
	const host = read("components/VisualHost.tsx");
	assert.match(host, /previewVariantForTemplate/);
	assert.ok(!host.includes('templateId.includes("craftax")'));
	const classifier = read("runtime/templatePresentation.ts");
	assert.match(classifier, /includes\("harbor"\)/);
	assert.match(classifier, /includes\("craftax"\)/);
	const session = read("runtime/sessionView.ts");
	assert.match(session, /previewVariantForTemplate/);
});

test("intended design: deferred LoRA support leaves no fixture catalog or placeholder UI", () => {
	const composer = read("components/Composer.tsx");
	const settings = read("components/SettingsPage.tsx");
	const landing = read("types/landing.ts");
	assert.ok(!composer.includes("Laguna LoRAs"));
	assert.ok(!composer.includes("open-finetunes-settings"));
	assert.ok(!settings.includes('id: "finetunes"'));
	assert.ok(!settings.includes("Adapters"));
	assert.ok(!landing.includes("LoraAdapter"));
	assert.ok(!landing.includes("AVAILABLE_LORAS"));
	assert.ok(!landing.includes("selectedLoraId"));
});

test("intended design: Inventory Attach defaults to GameBench Craftax :8098", () => {
	const inventory = read("components/DataPage.tsx");
	assert.match(inventory, /127\.0\.0\.1:8098/);
	assert.match(inventory, /data-testid="attach-container"/);
	assert.match(inventory, /open-trace-\$\{t\.id\}/);
	assert.ok(!inventory.includes("data-testid=\"import-trace-v5\""), "v0.3 inspects sealed traces; it does not ship a catalog import control");
	assert.ok(!inventory.includes("127.0.0.1:8100"), "demo :8100 placeholder should not be the Attach default");
});

test("intended design: styles must not retain a LoRA picker affordance", () => {
	const composer = read("components/Composer.tsx");
	const styles = read("styles/app.css");
	assert.ok(!composer.includes("is-lora"));
	assert.ok(!styles.includes(".is-lora"));
});

test("intended design: Playwright workers isolate renderer ports and Vite caches", () => {
	const fixture = readFileSync(join(appRoot, "tests/playwright/browser.fixture.ts"), "utf8");
	const viteConfig = readFileSync(join(appRoot, "vite.config.ts"), "utf8");
	assert.ok(!fixture.includes("127.0.0.1:1420"));
	assert.match(fixture, /reserveLoopbackPort/);
	assert.match(fixture, /--strictPort/);
	assert.match(fixture, /SYNTH_DESKTOP_VITE_CACHE_DIR: cacheDir/);
	assert.match(viteConfig, /cacheDir: process\.env\.SYNTH_DESKTOP_VITE_CACHE_DIR/);
});

test("Codex thread compaction uses the native app glyph and divider", () => {
	const transcript = read("components/ChatTranscript.tsx");
	const sessionView = read("runtime/sessionView.ts");
	assert.match(sessionView, /event\.eventKind === "thread\/compacted"/);
	assert.match(sessionView, /kind: "context_compaction"/);
	assert.match(sessionView, /Model switch - context compacted/);
	assert.match(sessionView, /case "model_switch"/);
	// Non-manual compact stays in the before-stream so post-switch tools render below the divider.
	assert.match(sessionView, /placement: source === "manual" \? "after" : "before"/);
	assert.match(sessionView, /formatTokensAsMillions/);
	assert.match(sessionView, /contextCompactionTokenSummary/);
	assert.match(transcript, /function IconContextCompaction/);
	assert.match(transcript, /M12\.666 3\.50098/);
	assert.match(transcript, /className="context-compaction-divider/);
	assert.match(transcript, /context-compaction-toggle/);
	assert.match(transcript, /line\.placement !== "after"/);
	assert.match(transcript, /line\.placement === "after"/);
});

test("Trace V5 viewer keeps async resolution revision-safe and failures explicit", () => {
	const host = read("components/VisualHost.tsx");
	assert.match(host, /bindTemplateSlots\(template, bindings, \{ loadTraceV5/);
	assert.match(host, /resolveTraceProjection\(source, "rollout-inspector"\)/);
	assert.match(host, /if \(cancelled\) return;/);
	assert.match(host, /artifact\.revision/);
	assert.match(host, /status: "loading", props: \{\}/);
	for (const state of [
		"Trace is quarantined",
		"Unsupported trace schema",
		"Trace extractor unavailable",
		"Sealed trace archive missing",
		"Trace resolver unavailable"
	]) assert.match(host, new RegExp(state));
});

test("Data Inspect persists trace identity and digest binding without projection payload", () => {
	// The binding shape itself moved to runtime/traceInspector.ts and is now
	// asserted behaviourally in trace_inspector_identity.test.mjs — including
	// that a re-sealed trace does not reuse the old archive's inspector, which
	// matching source text could never have caught. What remains DataPage's own
	// responsibility is checked here.
	const inspector = read("runtime/traceInspector.ts");
	assert.match(inspector, /templateId: TRACE_INSPECTOR_TEMPLATE/);
	assert.match(inspector, /slot: "projection"/);
	assert.match(inspector, /kind: "trace_v5"/);
	assert.match(inspector, /source: trace\.digest/);
	assert.match(inspector, /traceRecordId: trace\.id/);

	const inventory = read("components/DataPage.tsx");
	assert.match(inventory, /bridges\.visuals\.list\(\{ templateId: TRACE_INSPECTOR_TEMPLATE/);
	// A projection payload must never be inlined into the visual: the sealed
	// archive stays the source and the projection is resolved on demand.
	assert.doesNotMatch(inventory, /payload:\s*projection/);
	assert.doesNotMatch(inspector, /payload:\s*projection/);
});

/**
 * P0-2 lock — the renderer does not write a run status.
 *
 * `TrainingWorkspace` marked a run `failed` when its *poll* threw, and
 * `TerminalPanel` marked a shell `failed` when its *stream* errored. Neither
 * knew anything about the process; both produced a durable-looking terminal
 * word from a transport problem. Transport state belongs in a `connection`
 * field, and the durable status has exactly one writer: the host.
 */
test("components never write a terminal status themselves", () => {
	const componentsDir = join(renderer, "components");
	const offenders = [];
	for (const name of readdirSync(componentsDir)) {
		if (!name.endsWith(".tsx") && !name.endsWith(".ts")) continue;
		const source = readFileSync(join(componentsDir, name), "utf8");
		source.split("\n").forEach((line, index) => {
			// Prose about the rule is not a violation of it.
			const code = line.replace(/\/\*.*?\*\//g, "").split("//")[0];
			if (/^\s*\*/.test(line)) return;
			if (/status:\s*"(failed|cancelled|completed)"/.test(code)) {
				offenders.push(`components/${name}:${index + 1}: ${line.trim().slice(0, 120)}`);
			}
		});
	}
	assert.deepEqual(
		offenders,
		[],
		`the renderer must not write a terminal run status; use a renderer-local \`connection\` field:\n  ${offenders.join("\n  ")}`
	);
});

/**
 * P0-2 lock — one status vocabulary, and it comes from Rust.
 *
 * `normalizeRunStatus` used to accept nine spellings no producer ever emitted
 * and read anything unrecognised as `running`, which is how a settled run kept
 * a spinner turning. The map is now keyed by the generated `OptimizerRunStatus`
 * union so `tsc` refuses a status Rust does not have — and refuses to omit one
 * Rust adds.
 */
test("run status normalization is keyed by the generated Rust union", () => {
	const types = read("runtime/runProgress/types.ts");
	assert.match(
		types,
		/import type \{ OptimizerRunStatus \} from "\.\.\/\.\.\/generated\/protocol"/,
		"the producer vocabulary must come from the generated bindings"
	);
	assert.match(
		types,
		/const PRODUCER_STATUS: Record<OptimizerRunStatus, RunProgressStatus>/,
		"the map must be exhaustive over the generated union"
	);
	assert.match(types, /return producer === null \? "unknown" : PRODUCER_STATUS\[producer\]/);
	for (const consumerOnly of ["terminated", "disconnected", "stalled", "prepared"]) {
		assert.doesNotMatch(
			types,
			new RegExp(`"${consumerOnly}"`),
			`${consumerOnly} is not a status any producer writes; it must not be normalized`
		);
	}
});
