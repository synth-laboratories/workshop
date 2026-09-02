/**
 * Behavioural tests for the local-inference monitor.
 *
 * The renderer has no DOM test runtime, so the component is compiled with the
 * esbuild that ships with Vite and rendered through `react-dom/server`. That
 * exercises the real component tree for every visual state. Effects do not run
 * under static rendering, so the subscription lifecycle is proven directly
 * against `attachInferenceFeed` — the exact function the hook's effect returns.
 */
import assert from "node:assert/strict";
import { mkdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { buildSync } from "esbuild";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const source = join(appRoot, "src/renderer/src/components/InferencePanel.tsx");
const sourceText = readFileSync(source, "utf8");
const appCss = readFileSync(join(appRoot, "src/renderer/src/styles/app.css"), "utf8");

// Bundle into node_modules cache so relative ../bridge resolves, while bare
// react / @tauri-apps stay external. Nothing generated lands in the tracked tree.
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });
const compiled = join(compiledDir, "InferencePanel.mjs");
buildSync({
	entryPoints: [source],
	bundle: true,
	format: "esm",
	target: "es2022",
	platform: "neutral",
	jsx: "automatic",
	outfile: compiled,
	external: ["react", "react/jsx-runtime", "react-dom", "react-dom/server", "@tauri-apps/*"]
});

const {
	InferencePanel,
	attachInferenceFeed,
	compactModelName,
	emptyFeed,
	formatBytes,
	formatMs,
	formatQueue,
	formatTps,
	inferenceAuthorityLabel,
	inferenceObservedAt,
	INFERENCE_AUTHORITY_LOCAL,
	INFERENCE_AUTHORITY_SHOAL,
	reduceFeed,
	sparklinePath,
	HISTORY_LIMIT
} = await import(pathToFileURL(compiled).href);

/* ------------------------------------------------------------- fixtures */

const idleRolling = {
	requestsCompleted: 31,
	requestsFailed: 1,
	requestsCancelled: 2,
	inputTokens: 41000,
	outputTokens: 9100,
	cachedTokens: 22000,
	ttftP50Ms: 1840,
	ttftP95Ms: 3100,
	decodeTpsP50: 12.4,
	decodeTpsP95: 13.1,
	latencyP50Ms: 8200,
	latencyP95Ms: 19400
};

const blankRolling = {
	requestsCompleted: null,
	requestsFailed: null,
	requestsCancelled: null,
	inputTokens: null,
	outputTokens: null,
	cachedTokens: null,
	ttftP50Ms: null,
	ttftP95Ms: null,
	decodeTpsP50: null,
	decodeTpsP95: null,
	latencyP50Ms: null,
	latencyP95Ms: null
};

function generation(overrides = {}) {
	return {
		generationId: "sha256:abc",
		phase: "decode",
		queuedAt: 1000,
		startedAt: 1200,
		firstTokenAt: 3040,
		lastTokenAt: 19400,
		promptTokens: 12198,
		cachedTokens: 8420,
		outputTokens: 226,
		cacheHitRatio: 0.69,
		prefillTokensPerSecond: 980.2,
		decodeTokensPerSecond: 12.4,
		elapsedMs: 18200,
		...overrides
	};
}

function snapshot(overrides = {}) {
	return {
		model: "poolside/Laguna-XS-2.1-NVFP4-mlx",
		resident: true,
		residentBytes: 21568899389,
		queueDepth: 2,
		queueCapacity: 8,
		active: null,
		rolling: idleRolling,
		...overrides
	};
}

function monitor(overrides = {}) {
	return {
		...emptyFeed,
		state: "ready",
		unloadState: "idle",
		unloadDetail: null,
		unload: () => undefined,
		retry: () => undefined,
		...overrides
	};
}

function render(props) {
	return renderToStaticMarkup(createElement(InferencePanel, props));
}

/* --------------------------------------------------------------- states */

test("loading state announces telemetry is being read", () => {
	const html = render({ monitor: monitor({ state: "loading" }) });
	assert.match(html, /data-state="loading"/);
	assert.match(html, /data-testid="inference-loading"/);
	assert.match(html, /role="status"/);
	assert.match(html, /Reading local inference telemetry/);
});

test("live generation renders phase, rate, queue and token chips", () => {
	const html = render({
		monitor: monitor({
			snapshot: snapshot({ active: generation() }),
			throughput: [11.8, 12.1, 12.4],
			queue: [1, 2, 2]
		})
	});
	assert.match(html, /data-phase="decode"/);
	assert.match(html, /GENERATING/);
	assert.match(html, />decode</);
	assert.match(html, /12\.4 tok\/s/);
	assert.match(html, /18\.2 s/);
	assert.match(html, /RESIDENT/);
	assert.match(html, /20\.1 GB/);
	assert.match(html, /2\/8/); // queue depth / capacity
	assert.match(html, /12,198/); // prompt tokens
	assert.match(html, /8,420/); // cached tokens
	assert.match(html, /69%/); // cache hit ratio
	assert.match(html, /226/); // output tokens
	assert.match(html, /1\.84 s/); // ttft p50
	assert.match(html, /3\.10 s/); // ttft p95
	// Freeing the model is refused while the slot is held.
	assert.match(html, /data-testid="inference-free"[^>]*disabled/);
	assert.match(html, /A generation is running/);
});

test("every daemon phase renders as a distinct labelled state", () => {
	const phases = {
		queued: "queued",
		loading: "loading weights",
		compiling: "compiling",
		prefill: "prefill",
		decode: "decode",
		complete: "complete"
	};
	for (const [phase, label] of Object.entries(phases)) {
		const html = render({
			monitor: monitor({ snapshot: snapshot({ active: generation({ phase }) }) })
		});
		assert.match(html, new RegExp(`data-phase="${phase}"`), phase);
		assert.match(html, new RegExp(`>${label}<`), label);
	}
});

test("idle state says so and offers a working Free now control", () => {
	const html = render({ monitor: monitor({ snapshot: snapshot() }) });
	assert.match(html, /data-phase="idle"/);
	assert.match(html, /IDLE/);
	assert.match(html, /no generation in flight/);
	assert.doesNotMatch(html, /data-testid="inference-free"[^>]*disabled/);
	assert.match(html, /Release the model weights now/);
	assert.match(html, /data-testid="inference-authority">Local</);
});

test("idle telemetry is compact by default with diagnostics behind Advanced", () => {
	const html = render({
		monitor: monitor({
			snapshot: snapshot(),
			recent: [{
				id: "recent-1",
				status: "ok",
				phase: "complete",
				model: "poolside/Laguna-XS-2.1-NVFP4-mlx",
				promptTokens: 19734,
				outputTokens: 17,
				cachedTokens: 0,
				cacheHitRatio: 0,
				ttftMs: 22430,
				decodeTps: 28.8
			}]
		})
	});
	assert.match(html, /data-testid="inference-recent"/);
	assert.match(html, /Recent requests/);
	assert.match(html, /28\.8 tok\/s/);
	assert.match(html, /ttft 22\.43 s/);
	assert.match(html, /<details class="inference-advanced">/);
	assert.match(html, /<summary>Advanced<\/summary>/);
	assert.doesNotMatch(html, /<details class="inference-advanced" open/);
});

test("a pinned finetune is identified as a LoRA model while idle", () => {
	const html = render({
		monitor: monitor({ snapshot: snapshot() }),
		selectedModel: "synth/Laguna-XS-2.1-ft"
	});
	assert.match(html, /Laguna XS 2\.1 ft/);
	assert.match(html, /data-testid="inference-policy-kind"/);
	assert.match(html, /data-finetuned="yes"/);
	assert.match(html, /Fine-tuned model · LoRA attached/);
});

test("an active local turn is not labelled idle between inference calls", () => {
	const html = render({ monitor: monitor({ snapshot: snapshot() }), turnRunning: true });
	assert.match(html, /data-phase="turn-active"/);
	assert.match(html, /TURN ACTIVE/);
	assert.match(html, /waiting for next inference call/);
	assert.doesNotMatch(html, />IDLE</);
	assert.doesNotMatch(html, /no generation in flight/);
	assert.match(html, /data-testid="inference-free"[^>]*disabled/);
	assert.match(html, /another inference call may follow/);
});

test("a cold local turn reports model warmup before generation telemetry begins", () => {
	const html = render({
		monitor: monitor({ snapshot: snapshot({ resident: false, residentBytes: null }) }),
		turnRunning: true,
		warmingUp: true
	});
	assert.match(html, /data-phase="loading"/);
	assert.match(html, /LOADING/);
	assert.match(html, /LOADING on Local/);
	assert.match(html, /loading model weights/);
	assert.doesNotMatch(html, /waiting for next inference call/);
	assert.doesNotMatch(html, />IDLE</);
});

test("unloaded state reports no residency and omits the inapplicable memory action", () => {
	const html = render({
		monitor: monitor({ snapshot: snapshot({ resident: false, residentBytes: null }) })
	});
	assert.match(html, /data-phase="unloaded"/);
	assert.match(html, /data-resident="no"/);
	assert.match(html, /UNLOADED on Local/);
	assert.match(html, /NOT LOADED/);
	assert.match(html, /model weights are not resident/);
	assert.doesNotMatch(html, /data-testid="inference-free"/);
});

test("error state is announced and offers a retry", () => {
	const html = render({
		monitor: monitor({ state: "error", error: "Laguna is unreachable at http://127.0.0.1:7333" })
	});
	assert.match(html, /role="alert"/);
	assert.match(html, /data-testid="inference-error"/);
	assert.match(html, /unreachable/);
	assert.match(html, /Try again/);
});

test("unavailable metrics render as Unavailable and never as a fabricated zero", () => {
	const html = render({
		monitor: monitor({
			snapshot: snapshot({
				residentBytes: null,
				queueDepth: null,
				queueCapacity: null,
				rolling: blankRolling,
				active: generation({
					promptTokens: null,
					cachedTokens: null,
					outputTokens: null,
					cacheHitRatio: null,
					decodeTokensPerSecond: null,
					elapsedMs: null
				})
			})
		})
	});
	const unavailable = html.match(/Unavailable/g) ?? [];
	// residency, queue, prompt, cached, output, rate, elapsed, 6 rolling stats.
	assert.ok(unavailable.length >= 13, `expected many Unavailable cells, saw ${unavailable.length}`);
	assert.doesNotMatch(html, /0 tok\/s/);
	assert.doesNotMatch(html, />0</);
	assert.match(html, /inference-unavailable/);
	assert.match(html, /is not reported by the daemon/);
});

test("implausible decode throughput is withheld instead of dominating the monitor", () => {
	assert.equal(formatTps(96_380_753), "Unavailable");
	assert.equal(formatTps(46.6), "46.6");
	const html = render({
		monitor: monitor({ snapshot: snapshot({ rolling: { ...idleRolling, decodeTpsP95: 96_380_753 } }) })
	});
	assert.match(html, /Decode p95[^>]*>Unavailable/);
	assert.doesNotMatch(html, /96,380,753|96380753/);
});

test("recent requests distinguish ok, failed and cancelled outcomes", () => {
	const html = render({
		monitor: monitor({
			snapshot: snapshot(),
			recent: [
				{
					id: "a",
					status: "failed",
					phase: "decode",
					model: "poolside/Laguna-XS-2.1-NVFP4-mlx",
					promptTokens: 900,
					outputTokens: 12,
					cachedTokens: 0,
					cacheHitRatio: 0,
					ttftMs: 2100,
					decodeTps: 9.2
				},
				{
					id: "b",
					status: "cancelled",
					phase: "prefill",
					model: null,
					promptTokens: null,
					outputTokens: null,
					cachedTokens: null,
					cacheHitRatio: null,
					ttftMs: null,
					decodeTps: null
				}
			]
		})
	});
	assert.match(html, /data-status="failed"/);
	assert.match(html, /data-status="cancelled"/);
	assert.match(html, /Laguna XS 2\.1/);
	assert.match(html, /2\.10 s/);
});

test("sparklines carry accessible labels and degrade to a stated empty state", () => {
	const sparse = render({ monitor: monitor({ snapshot: snapshot(), throughput: [1] }) });
	assert.match(sparse, /Not enough samples yet/);
	const drawn = render({
		monitor: monitor({ snapshot: snapshot(), throughput: [1, 4, 9], queue: [0, 1, 2] })
	});
	assert.match(drawn, /role="img"/);
	assert.match(drawn, /decode tok\/s over the last 3 samples/);
	assert.match(drawn, /data-testid="inference-spark-queue"/);
	assert.match(drawn, /in-flight requests over the last 3 samples/);
});

/* --------------------------------------------------- subscription lifecycle */

function fakeTransport() {
	const calls = { subscribe: 0, teardown: 0, snapshot: 0, unload: 0 };
	let deliver = null;
	let resolveSnapshot = () => undefined;
	return {
		calls,
		push: (value) => deliver?.(value),
		resolvePending: (value) => resolveSnapshot(value),
		transport: {
			snapshot() {
				calls.snapshot += 1;
				return new Promise((resolve) => {
					resolveSnapshot = resolve;
				});
			},
			subscribe(onSnapshot) {
				calls.subscribe += 1;
				deliver = onSnapshot;
				return () => {
					calls.teardown += 1;
					deliver = null;
				};
			},
			unload() {
				calls.unload += 1;
				return Promise.resolve({ released: true, conflict: false, detail: null });
			}
		}
	};
}

test("closing the pane tears the subscription down and silences late results", async () => {
	const fake = fakeTransport();
	const published = [];
	const detach = attachInferenceFeed(fake.transport, (feed) => published.push(feed));

	assert.equal(fake.calls.subscribe, 1);
	assert.equal(fake.calls.teardown, 0);
	fake.push(snapshot({ active: generation() }));
	assert.equal(published.length, 1);
	assert.equal(published[0].state, "ready");

	// The pane closes.
	detach();
	assert.equal(fake.calls.teardown, 1, "transport teardown must run exactly once");

	// Nothing the transport still holds may reach the closed pane.
	fake.push(snapshot());
	fake.resolvePending(snapshot());
	await Promise.resolve();
	await Promise.resolve();
	assert.equal(published.length, 1, "no publish may follow teardown");
});

test("the hook opens the feed only while visible and returns its teardown", () => {
	// Effects cannot run without a DOM, so the wiring itself is asserted.
	assert.match(sourceText, /if \(!visible\) \{[\s\S]*?return;\n\t\t\}/);
	assert.match(sourceText, /return attachInferenceFeed\(transport, setFeed, historyLimit\);/);
	assert.match(sourceText, /\}, \[attempt, historyLimit, transport, visible\]\);/);
});

test("a hidden pane renders paused and never touches the transport", () => {
	const fake = fakeTransport();
	const html = render({ visible: false, transport: fake.transport });
	assert.match(html, /data-state="off"/);
	assert.match(html, /data-testid="inference-paused"/);
	assert.match(html, /Monitor paused/);
	assert.equal(fake.calls.subscribe, 0);
	assert.equal(fake.calls.snapshot, 0);
});

test("a failing snapshot surfaces an error only while no data has arrived", async () => {
	const failing = {
		snapshot: () => Promise.reject(new Error("connection refused")),
		subscribe: () => () => undefined,
		unload: () => Promise.reject(new Error("nope"))
	};
	const published = [];
	const detach = attachInferenceFeed(failing, (feed) => published.push(feed));
	await Promise.resolve();
	await Promise.resolve();
	detach();
	assert.equal(published.at(-1).state, "error");
	assert.equal(published.at(-1).error, "connection refused");
});

/* ------------------------------------------------------------ accumulation */

test("feed history is bounded and preserves gaps as gaps", () => {
	let feed = emptyFeed;
	for (let index = 0; index < HISTORY_LIMIT + 10; index += 1) {
		const active = index % 2 === 0 ? generation({ decodeTokensPerSecond: index }) : null;
		feed = reduceFeed(feed, snapshot({ active, queueDepth: index }));
	}
	assert.equal(feed.throughput.length, HISTORY_LIMIT);
	assert.equal(feed.queue.length, HISTORY_LIMIT);
	assert.ok(feed.throughput.includes(null), "idle samples stay null rather than becoming zero");
	assert.equal(feed.queue.at(-1), HISTORY_LIMIT + 9);
});

test("a departing generation is classified from the rolling counter that moved", () => {
	const first = reduceFeed(emptyFeed, snapshot({ active: generation() }));
	const cancelled = reduceFeed(
		first,
		snapshot({
			active: null,
			rolling: { ...idleRolling, requestsCancelled: idleRolling.requestsCancelled + 1 }
		})
	);
	assert.equal(cancelled.recent.length, 1);
	assert.equal(cancelled.recent[0].status, "cancelled");
	assert.equal(cancelled.recent[0].ttftMs, 1840);

	const failed = reduceFeed(
		first,
		snapshot({
			active: null,
			rolling: { ...idleRolling, requestsFailed: idleRolling.requestsFailed + 1 }
		})
	);
	assert.equal(failed.recent[0].status, "failed");

	const completed = reduceFeed(first, snapshot({ active: null }));
	assert.equal(completed.recent[0].status, "ok");

	// A generation still in the slot is not archived.
	const held = reduceFeed(first, snapshot({ active: generation() }));
	assert.equal(held.recent.length, 0);
});

/* --------------------------------------------------------------- formatting */

test("formatters answer Unavailable instead of inventing values", () => {
	assert.equal(formatBytes(null), "Unavailable");
	assert.equal(formatBytes(0), "Unavailable");
	assert.equal(formatBytes(21568899389), "20.1 GB");
	assert.equal(formatMs(null), "Unavailable");
	assert.equal(formatMs(310), "310 ms");
	assert.equal(formatMs(1840), "1.84 s");
	assert.equal(formatQueue(null, 8), "Unavailable");
	assert.equal(formatQueue(2, null), "2");
	assert.equal(formatQueue(0, 8), "0/8");
	assert.equal(compactModelName("poolside/Laguna-XS-2.1-NVFP4-mlx"), "Laguna XS 2.1 · NVFP4");
	assert.equal(compactModelName(null), "Local model");
});

test("sparkline paths break at unavailable samples", () => {
	assert.equal(sparklinePath([]), null);
	assert.equal(sparklinePath([4]), null);
	assert.equal(sparklinePath([null, null]), null);
	const path = sparklinePath([1, null, 3, 4], 100, 20);
	assert.match(path, /^M/);
	// Two moves: the opening point and the restart after the gap.
	assert.equal((path.match(/M/g) ?? []).length, 2);
});

test("inference authority is Local for the on-device Laguna sidecar", () => {
	assert.equal(inferenceAuthorityLabel({}), INFERENCE_AUTHORITY_LOCAL);
	assert.equal(inferenceAuthorityLabel({ source: "local" }), "Local");
	assert.equal(inferenceAuthorityLabel({ baseUrl: "http://127.0.0.1:7333" }), "Local");
	assert.equal(inferenceAuthorityLabel({ source: "laguna", baseUrl: "http://localhost:7333" }), "Local");
	assert.equal(inferenceObservedAt({}), null);
	assert.equal(inferenceObservedAt({ updatedAt: Date.parse("2026-08-26T17:14:00.000Z") })?.iso, "2026-08-26T17:14:00.000Z");
	const html = render({
		monitor: monitor({ snapshot: snapshot() }),
		status: { baseUrl: "http://127.0.0.1:7333", updatedAt: Date.parse("2026-08-26T17:14:00.000Z") }
	});
	assert.match(html, /data-testid="inference-authority">Local</);
	assert.match(html, /data-testid="inference-observed-at"/);
	assert.match(html, /dateTime="2026-08-26T17:14:00.000Z"/);
	assert.doesNotMatch(html, /Synth Cloud · Shoal/);
});

test("inference authority is Synth Cloud · Shoal only when the observation is hosted", () => {
	assert.equal(inferenceAuthorityLabel({ source: "shoal" }), INFERENCE_AUTHORITY_SHOAL);
	assert.equal(inferenceAuthorityLabel({ source: "synth-cloud" }), "Synth Cloud · Shoal");
	assert.equal(inferenceAuthorityLabel({ baseUrl: "https://inference.shoal.synth.dev" }), "Synth Cloud · Shoal");
	const html = render({
		monitor: monitor({
			snapshot: snapshot({
				resident: false,
				residentBytes: null,
				source: "shoal",
				observedAt: "2026-08-26T17:14:00.000Z"
			})
		})
	});
	assert.match(html, /data-testid="inference-authority">Synth Cloud · Shoal</);
	assert.match(html, /UNLOADED on Synth Cloud · Shoal/);
	assert.match(html, /data-authority="shoal"/);
	assert.doesNotMatch(html, />UNLOADED</);
});

test("inference panel fills the side panel and settings workspace", () => {
	assert.match(sourceText, /className=\{shell\}/);
	assert.match(sourceText, /"inference-panel"/);
	assert.match(
		appCss,
		/\.workbench-side-panel-content \.inference-panel,[\s\S]*?\.settings-page \.inference-panel,[\s\S]*?width:\s*100%;[\s\S]*?max-width:\s*none/
	);
	assert.match(appCss, /\.inference-panel\s*\{[^}]*width:\s*100%;[^}]*max-width:\s*none/s);
	assert.doesNotMatch(appCss, /\.inference-panel\s*\{[^}]*max-width:\s*390px/s);
});
