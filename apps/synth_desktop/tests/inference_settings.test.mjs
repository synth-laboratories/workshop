/**
 * Behavioural tests for the daemon runtime-settings editor and its deep link.
 *
 * The renderer has no DOM test runtime, so components are compiled with the
 * esbuild that ships with Vite and rendered through `react-dom/server`.
 * Effects do not run under static rendering, so the fetch/commit lifecycle is
 * proven directly against the exported pure functions the hook is built from,
 * and view states are rendered through the injectable `controller` prop.
 */
import assert from "node:assert/strict";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { transformSync } from "esbuild";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });

function compile(relative, outName) {
	const source = join(appRoot, relative);
	const compiled = join(compiledDir, outName);
	writeFileSync(
		compiled,
		transformSync(readFileSync(source, "utf8"), {
			loader: "tsx",
			jsx: "automatic",
			format: "esm",
			target: "es2022",
			sourcefile: source
		}).code
	);
	return pathToFileURL(compiled).href;
}

const {
	CALIBRATED_LOCAL_CODING_SETTINGS,
	InferenceSettings,
	OUTPUT_TOKEN_OPTIONS,
	commitSettings,
	interpretSnapshot,
	rejectionMessage,
	settingsFromBody
} = await import(compile("src/renderer/src/components/InferenceSettings.tsx", "InferenceSettings.mjs"));

const { InferencePanel } = await import(
	compile("src/renderer/src/components/InferencePanel.tsx", "InferencePanelGear.mjs")
);

/* ------------------------------------------------------------- fixtures */

const daemonSettings = {
	default_temperature: 1.0,
	default_top_p: 1.0,
	default_top_k: 20,
	default_reasoning_effort: "none",
	default_max_output_tokens: 4096,
	idle_unload_after_seconds: 300,
	prompt_cache_slots: 4,
	queue_capacity: 8
};

function envelope(settings = daemonSettings) {
	return {
		schema_version: "1.0",
		settings,
		source: { path: "/tmp/laguna/settings.toml", loaded_at: 1_754_700_000 }
	};
}

const invalidSettingBody = {
	error: {
		code: "invalid_setting",
		message: "top_k must be an integer between 0 and 8192",
		retryable: false,
		details: { field: "default_top_k" }
	},
	request_id: "req-42"
};

function fakeTransport({ snapshot, update }) {
	const calls = { snapshot: 0, update: 0, patches: [] };
	return {
		calls,
		transport: {
			snapshot() {
				calls.snapshot += 1;
				return Promise.resolve(snapshot);
			},
			update(patch) {
				calls.update += 1;
				calls.patches.push(patch);
				return Promise.resolve(update);
			}
		}
	};
}

function controller(overrides = {}) {
	return {
		view: { state: "ready", settings: daemonSettings },
		rejection: null,
		commit: () => undefined,
		commitPatch: () => undefined,
		retry: () => undefined,
		...overrides
	};
}

function render(props) {
	return renderToStaticMarkup(createElement(InferenceSettings, props));
}

/* ----------------------------------------------------- protocol wiring */

test("a supported GET resolves to the daemon's settings", async () => {
	const fake = fakeTransport({
		snapshot: { supported: true, status: 200, body: envelope() }
	});
	const exchange = await fake.transport.snapshot();
	const view = interpretSnapshot(exchange);
	assert.equal(fake.calls.snapshot, 1);
	assert.equal(view.state, "ready");
	assert.deepEqual(view.settings, daemonSettings);
});

test("a 404 is feature-detected as unsupported, never an error", () => {
	const view = interpretSnapshot({ supported: false, status: 404, body: null });
	assert.deepEqual(view, { state: "unsupported" });
});

test("a malformed success body is an error state, not a broken form", () => {
	const view = interpretSnapshot({ supported: true, status: 200, body: { nope: true } });
	assert.equal(view.state, "error");
	assert.match(view.message, /HTTP 200/);
	assert.equal(settingsFromBody("not an object"), null);
});

test("a commit PUTs the partial patch and reconciles to the effective settings", async () => {
	// The daemon clamps the committed value: the response is the only truth.
	const effective = { ...daemonSettings, default_top_k: 8192 };
	const fake = fakeTransport({
		update: { supported: true, status: 200, body: envelope(effective) }
	});
	const outcome = await commitSettings(fake.transport, { default_top_k: 9000 });
	assert.equal(fake.calls.update, 1);
	assert.deepEqual(fake.calls.patches, [{ default_top_k: 9000 }]);
	assert.deepEqual(outcome, { settings: effective });
});

test("a 400 surfaces the daemon's typed invalid_setting message", async () => {
	const fake = fakeTransport({
		update: { supported: true, status: 400, body: invalidSettingBody }
	});
	const outcome = await commitSettings(fake.transport, { default_top_k: -1 });
	assert.deepEqual(outcome, { rejection: "top_k must be an integer between 0 and 8192" });
});

test("a rejection without a typed envelope falls back to the HTTP status", () => {
	assert.equal(
		rejectionMessage({ supported: true, status: 502, body: null }),
		"The daemon rejected the update (HTTP 502)."
	);
});

test("a PUT against an older daemon reports the unsupported notice", async () => {
	const fake = fakeTransport({ update: { supported: false, status: 404, body: null } });
	const outcome = await commitSettings(fake.transport, { queue_capacity: 2 });
	assert.match(outcome.rejection, /does not support runtime settings yet/);
});

test("the calibrated preset is one atomic patch and preserves the output-token limit", () => {
	assert.deepEqual(CALIBRATED_LOCAL_CODING_SETTINGS, {
		default_temperature: 1,
		default_top_p: 1,
		default_top_k: 20,
		default_reasoning_effort: "high",
		idle_unload_after_seconds: 900,
		prompt_cache_slots: 4,
		queue_capacity: 9
	});
	assert.equal("default_max_output_tokens" in CALIBRATED_LOCAL_CODING_SETTINGS, false);
});

test("output token choices are 1024 times powers of two through 32K", () => {
	assert.deepEqual(OUTPUT_TOKEN_OPTIONS, [1024, 2048, 4096, 8192, 16384, 32768]);
});

/* ------------------------------------------------------------ rendering */

test("the ready form renders every settings group with the daemon's values", () => {
	const html = render({ controller: controller() });
	assert.match(html, /data-testid="inference-settings"/);
	assert.match(html, /data-testid="inference-apply-calibrated-preset"/);
	assert.match(html, /Use calibrated defaults/);
	assert.match(html, /unload after 15 minutes · 4 cache slots · queue 9/);
	assert.match(html, /Sampling defaults/);
	assert.match(html, /Used only when a request does not specify its own value\./);
	assert.match(html, /temperature 1\.0 · top_k 20 · top_p 1\.0/);
	assert.match(html, /data-testid="inference-default-temperature"[^>]*value="1"/);
	assert.match(html, /data-testid="inference-default-top-k"[^>]*value="20"/);
	assert.match(html, /<select[^>]*data-testid="inference-default-max-output-tokens"/);
	for (const tokens of OUTPUT_TOKEN_OPTIONS) {
		assert.match(html, new RegExp(`<option value="${tokens}"`));
	}
	assert.match(html, /1,024 × 2\^k/);
	// idle_unload_after_seconds is surfaced in minutes.
	assert.match(html, /data-testid="inference-idle-unload-minutes"[^>]*value="5"/);
	assert.match(html, /0 = never unload/);
	assert.match(html, /data-testid="inference-prompt-cache-slots"[^>]*value="4"/);
	assert.match(html, /data-testid="inference-queue-capacity"[^>]*value="8"/);
	// Reasoning maps none/high onto an Off/On segmented control.
	assert.match(html, /aria-checked="true"[^>]*data-testid="inference-reasoning-none"/);
	assert.match(html, /aria-checked="false"[^>]*data-testid="inference-reasoning-high"/);
	// The footer points at the panel instead of duplicating live stats.
	assert.match(html, /MLX sidecar inference panel/);
	assert.doesNotMatch(html, /tok\/s/);
});

test("an unsupported daemon renders the quiet notice, never a form", () => {
	const html = render({ controller: controller({ view: { state: "unsupported" } }) });
	assert.match(html, /data-testid="inference-settings-unsupported"/);
	assert.match(html, /This daemon does not support runtime settings yet\./);
	assert.doesNotMatch(html, /<input/);
});

test("a daemon rejection renders inline on the offending field", () => {
	const html = render({
		controller: controller({
			rejection: { field: "default_top_k", message: "top_k must be an integer between 0 and 8192" }
		})
	});
	assert.match(html, /data-testid="inference-default-top-k-error"[^>]*>top_k must be an integer/);
	assert.match(html, /role="alert"/);
	// The message binds to top_k only.
	assert.doesNotMatch(html, /data-testid="inference-default-temperature-error"/);
});

test("a transport failure renders an error with a retry", () => {
	const html = render({
		controller: controller({ view: { state: "error", message: "Laguna settings are unreachable at http://127.0.0.1:7333" } })
	});
	assert.match(html, /data-testid="inference-settings-error"/);
	assert.match(html, /unreachable/);
	assert.match(html, /Try again/);
});

/* ------------------------------------------------------------ deep link */

const panelMonitor = {
	state: "loading",
	snapshot: null,
	error: null,
	throughput: [],
	queue: [],
	recent: [],
	unloadState: "idle",
	unloadDetail: null,
	unload: () => undefined,
	retry: () => undefined
};

test("the panel header renders a labelled settings button only when a target exists", () => {
	const withButton = renderToStaticMarkup(
		createElement(InferencePanel, { monitor: panelMonitor, onOpenSettings: () => undefined })
	);
	assert.match(withButton, /data-testid="inference-open-settings"/);
	assert.match(withButton, />Inference settings<\/span>/);
	const without = renderToStaticMarkup(createElement(InferencePanel, { monitor: panelMonitor }));
	assert.doesNotMatch(without, /data-testid="inference-open-settings"/);
});

test("the settings button deep-links to the Settings view and the rail explains the sidecar", () => {
	const app = readFileSync(join(appRoot, "src/renderer/src/App.tsx"), "utf8");
	assert.match(app, /onOpenSettings=\{\(\) => setView\(\{ kind: "settings", section: "inference" \}\)\}/);
	assert.match(app, /Owns local model memory, prompt caches, and the single-GPU queue\./);
});

test("Settings hosts an Inference section after Models and follows deep links", () => {
	const settings = readFileSync(
		join(appRoot, "src/renderer/src/components/SettingsPage.tsx"),
		"utf8"
	);
	const models = settings.indexOf('{ id: "models", label: "Models" }');
	const inference = settings.indexOf('{ id: "inference", label: "Inference" }');
	const voice = settings.indexOf('{ id: "voice", label: "Voice" }');
	assert.ok(models !== -1 && inference !== -1 && voice !== -1);
	assert.ok(models < inference && inference < voice, "Inference sits between Models and Voice");
	// An already-open Settings view must retarget when the prop changes.
	assert.match(settings, /useEffect\(\(\) => \{\n\t\tif \(SECTIONS\.some\(\(entry\) => entry\.id === initialSection\)\) setSection\(initialSection\);\n\t\}, \[initialSection\]\);/);
	assert.match(settings, /data-testid="settings-inference"/);
});
