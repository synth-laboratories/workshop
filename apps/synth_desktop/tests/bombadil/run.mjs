import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { existsSync } from "node:fs";
import { mkdtemp, readFile, rm, stat } from "node:fs/promises";
import { homedir, tmpdir } from "node:os";
import { dirname, extname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const appRoot = resolve(here, "../..");
const workshopRoot = resolve(appRoot, "../..");
const runtimeHome = await mkdtemp(resolve(tmpdir(), "synth-bombadil-"));
const connectionPath = resolve(runtimeHome, "connection.json");
const rendererRootCandidates = [
	resolve(appRoot, "dist"),
	resolve(appRoot, "out/renderer")
];
const rendererRoot = (
	await Promise.all(
		rendererRootCandidates.map(async (candidate) => {
			try {
				await stat(resolve(candidate, "index.html"));
				return candidate;
			} catch {
				return null;
			}
		})
	)
).find(Boolean);
if (!rendererRoot) {
	throw new Error(
		"No renderer build found. Run: npm run frontend:build --workspace @synth/synth-desktop"
	);
}
const bombadil = resolve(workshopRoot, "node_modules/.bin/bombadil");
const specificationPath = process.env.BOMBADIL_SPEC
	? resolve(workshopRoot, process.env.BOMBADIL_SPEC)
	: resolve(here, "layout.spec.ts");
// Unique output roots so parallel gate jobs do not clobber each other.
const outputPath = process.env.BOMBADIL_OUTPUT_PATH
	? resolve(process.env.BOMBADIL_OUTPUT_PATH)
	: resolve(
		appRoot,
		"test-results/bombadil",
		specificationPath.replace(/.*\//, "").replace(/\.spec\.ts$/, "")
	);
const includeCuaAnalysisVisual = specificationPath.endsWith("launch-debt.spec.ts")
	|| specificationPath.endsWith("visual-library-layout.spec.ts");
const includeBlankWorkedTurn = specificationPath.endsWith("empty-completed-turn.spec.ts")
	|| specificationPath.endsWith("empty-outputs.spec.ts")
	|| specificationPath.endsWith("inference-state-honesty.spec.ts")
	|| specificationPath.endsWith("run-summary-sanity.spec.ts")
	|| specificationPath.endsWith("model-menu-polish.spec.ts");
const includeComposerToolbar = specificationPath.endsWith("composer-toolbar.spec.ts");
const includeTerminalPolish = specificationPath.endsWith("terminal-polish.spec.ts");
const includeTraceCatalogLayout = specificationPath.endsWith("trace-catalog-layout.spec.ts");
const includeShellContainment = specificationPath.endsWith("shell-containment.spec.ts");
const includeInferenceHonesty = specificationPath.endsWith("inference-state-honesty.spec.ts");
const includeRunSummarySanity = specificationPath.endsWith("run-summary-sanity.spec.ts");
const includeMuseResidencyHonesty = specificationPath.endsWith("muse-residency-honesty.spec.ts");
// Five seconds covers every directed/eventual horizon in layout.spec.ts. Longer
// runs intermittently wedge the current Chromiumoxide transport after the
// properties have already been exercised, turning a clean trace into a harness
// watchdog failure. Nightly exploration can still opt into a longer duration.
const timeLimit = process.env.BOMBADIL_TIME_LIMIT
	|| (includeBlankWorkedTurn || includeComposerToolbar || includeTerminalPolish || includeMuseResidencyHonesty
		? "10s"
		: "5s");
const timeLimitMatch = /^(\d+(?:\.\d+)?)(ms|s|m)$/.exec(timeLimit);
if (!timeLimitMatch) throw new Error(`Unsupported BOMBADIL_TIME_LIMIT: ${timeLimit}`);
const timeLimitMs = Number(timeLimitMatch[1]) * ({ ms: 1, s: 1_000, m: 60_000 })[timeLimitMatch[2]];
const watchdogMs = timeLimitMs + 35_000;
const contentTypes = {
	".css": "text/css",
	".html": "text/html",
	".js": "text/javascript",
	".json": "application/json",
	".png": "image/png",
	".svg": "image/svg+xml"
};

let runtimeProcess;
let bombadilProcess;
let connection;

async function waitForConnection() {
	const deadline = Date.now() + 15_000;
	while (Date.now() < deadline) {
		try {
			const value = JSON.parse(await readFile(connectionPath, "utf8"));
			const response = await fetch(`${value.url}/v1/health`, {
				headers: value.token ? { Authorization: `Bearer ${value.token}` } : undefined
			});
			if (response.ok) return value;
		} catch {
			// Runtime is still starting.
		}
		await new Promise((resolvePromise) => setTimeout(resolvePromise, 150));
	}
	throw new Error("Isolated Synth runtime did not become healthy");
}

async function seedVisualAlignmentFixture() {
	const headers = {
		"Content-Type": "application/json",
		...(connection.token ? { Authorization: `Bearer ${connection.token}` } : {})
	};
	const sessionResponse = await fetch(`${connection.url}/v1/sessions`, {
		method: "POST",
		headers,
		body: JSON.stringify({
			target: { kind: "local", model: "laguna-xs-2.1", adapter: null },
			title: "Bombadil visual alignment"
		})
	});
	if (!sessionResponse.ok) throw new Error(`Could not seed Bombadil alignment session (${sessionResponse.status})`);
	const session = await sessionResponse.json();
	const visualResponse = await fetch(`${connection.url}/v1/visuals`, {
		method: "POST",
		headers,
		body: JSON.stringify({
			id: "bombadil-visual-alignment",
			templateId: "model.compare.v1",
			title: "Alignment comparison",
			bindings: {},
			sessionId: session.id,
			metadata: { fixture: true, purpose: "layout-alignment" }
		})
	});
	if (!visualResponse.ok) throw new Error(`Could not seed Bombadil alignment visual (${visualResponse.status})`);
}

async function seedTraceCatalogFixture() {
	if (!includeTraceCatalogLayout) return;
	const response = await fetch(`${connection.url}/v1/traces`, {
		method: "POST",
		headers: {
			"Content-Type": "application/json",
			...(connection.token ? { Authorization: `Bearer ${connection.token}` } : {})
		},
		body: JSON.stringify({
			title: "Bombadil Trace V5 viewport containment fixture",
			source: "import",
			payload: { schemaVersion: "synth.trace.v5", events: [{ kind: "tool.completed" }] },
			metadata: { schemaVersion: "synth.trace.v5", model: "poolside/laguna-xs-2.1", events: 83, tools: 7, status: "completed", hasEvidence: true }
		})
	});
	if (!response.ok) throw new Error(`Could not seed Bombadil trace fixture (${response.status})`);
}

/**
 * Seeds the exact dishonest completed-turn UI from the 2026-08-10 CUA shot:
 * Worked + empty assistant + Reasoned, plus an Unavailable tok/s chip.
 * Intentionally red until the product refuses to render that state.
 */
function blankWorkedTurnBridgeScript() {
	return `
const blankSessionId = "blank-worked-turn";
const blankStartedAt = "2026-08-10T15:44:50.000Z";
const blankCompletedAt = "2026-08-10T15:45:01.000Z";
function blankAppEvent(sequence, kind, payload, createdAt) {
  return {
    schemaVersion: "synth.desktop-app-event.v1",
    sequence,
    eventId: "blank-" + sequence,
    sessionId: blankSessionId,
    sessionSequence: sequence,
    runId: "run-blank-worked",
    source: "local",
    kind,
    payload,
    createdAt
  };
}
const blankWorkedEvents = [
  blankAppEvent(1, "run.started", {}, blankStartedAt),
  blankAppEvent(2, "message.created", { role: "user", content: "hello", messageId: "user-hello" }, blankStartedAt),
  // Empty delta still opens an assistant draft and marks the turn "produced".
  blankAppEvent(3, "message.delta", { delta: "", messageId: "asst-blank" }, "2026-08-10T15:44:51.000Z"),
  blankAppEvent(4, "agent.reasoning", { content: "Considering a greeting response." }, "2026-08-10T15:44:55.000Z"),
  blankAppEvent(5, "run.completed", { turn: { id: "turn-blank", status: "completed" } }, blankCompletedAt)
];
window.synthCodex = {
  defaultWorkspace: async () => "/workspaces/default",
  list: async () => [{
    sessionId: blankSessionId,
    threadId: "blank-thread",
    workspace: "/workspaces/default",
    model: "openrouter/poolside/laguna-s-2.1",
    providerName: "synth-cloud",
    providerTitle: "Synth Cloud Responses",
    baseUrl: "http://127.0.0.1:41109/api/v1",
    status: "ready",
    title: "Hello",
    approvalPolicy: "untrusted",
    sandbox: "workspace-write"
  }],
  start: async () => ({ sessionId: blankSessionId, threadId: "blank-thread" }),
  startTurn: async () => ({ sessionId: blankSessionId, threadId: "blank-thread", turnId: "turn-blank" }),
  interrupt: async () => undefined,
  close: async () => undefined,
  onEvent: () => () => undefined
};
window.synthCore = {
  diagnostics: async () => ({
    databasePath: "bombadil-memory://core",
    schemaVersion: 0,
    integrityOk: true,
    contentStorePath: "bombadil-memory://content",
    journalHead: blankWorkedEvents.length,
    sessionCount: 1,
    runCount: 1,
    visualCount: 0,
    migrationComplete: true
  }),
  eventsAfter: async () => blankWorkedEvents,
  sessionEventsAfter: async (sessionId) => sessionId === blankSessionId ? blankWorkedEvents : [],
  onEvent: () => () => undefined
};
// Implausible p50 still passes the null gate, so formatTps collapses to
// "Unavailable" and the composer chip advertises nonsense throughput.
window.synthModelPerformance = {
  summaries: async () => [{
    provider: "synth-cloud",
    modelId: "openrouter/poolside/laguna-s-2.1",
    measurementKind: "observed_stream",
    sampleCount: 1,
    tpsP50: 99999,
    tpsP95: 99999,
    ttftP50Ms: null,
    lastObservedAt: blankCompletedAt
  }]
};
`;
}

/**
 * Seeds the CUA composer-toolbar collision: Never ask · Full system access
 * plus Unavailable tok/s observed p50 next to Thinking Max.
 */
function composerToolbarBridgeScript() {
	return `
try {
  localStorage.setItem("synth.preferences.v1", JSON.stringify({
    schemaVersion: 3,
    appearance: {
      theme: "system",
      chatFontSize: 14,
      codeFontFamily: "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
      codeFontSize: 12,
      terminalFontFamily: "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
      terminalFontSize: 12
    },
    submission: { activeEnterAction: "enqueue" },
    toolActivity: { mode: "grouped" },
    agentContext: { autoCompactTokenLimits: { lagunaXs: 150000, lagunaS: 250000, luna: 250000 } },
    layout: {
      last: { sidebarVisible: true, sidebarWidth: 260, outputPaneVisible: false, outputPaneWidth: 420, bottomPanelVisible: false, bottomPanelHeight: 220, selectedConversationId: null, selectedOutputTab: null },
      default: { sidebarVisible: true, sidebarWidth: 260, outputPaneVisible: false, outputPaneWidth: 420, bottomPanelVisible: false, bottomPanelHeight: 220, selectedConversationId: null, selectedOutputTab: null }
    },
    conversations: {},
    promptQueue: [],
    unreadCompletedChats: [],
    approvalMode: "allow-all",
    approvalPolicy: "never",
    sandboxMode: "danger-full-access"
  }));
} catch (error) {
  console.warn("bombadil composer-toolbar prefs seed failed", error);
}

window.synthModelPerformance = {
  summaries: async () => [{
    provider: "openrouter",
    modelId: "poolside/laguna-s-2.1",
    measurementKind: "observed_stream",
    sampleCount: 1,
    tpsP50: 99999,
    tpsP95: 99999,
    ttftP50Ms: null,
    lastObservedAt: new Date().toISOString()
  }]
};
`;
}

/** Supplies enough native-terminal behavior to exercise the real xterm chrome
 * in a browser fixture. The renderer still owns tab selection, sizing, and
 * control behavior; this only replaces the Tauri command transport. */
function terminalPolishBridgeScript() {
	return `
window.__terminalPolishFixture = [
  { id: "terminal-main", workspaceId: "fixture", cwd: "/workspaces/default", shell: "/bin/zsh", title: "workspace", status: "running", createdAt: 1 },
  { id: "terminal-server", workspaceId: "fixture", cwd: "/workspaces/default", shell: "/bin/zsh", title: "dev server", status: "running", createdAt: 2 }
];
window.__terminalPolishSequence = 2;
window.synthTerminal = {
  available: true,
  create: async () => {
    const next = { id: "terminal-" + (++window.__terminalPolishSequence), workspaceId: "fixture", cwd: "/workspaces/default", shell: "/bin/zsh", title: "shell " + window.__terminalPolishSequence, status: "running", createdAt: window.__terminalPolishSequence };
    window.__terminalPolishFixture.push(next);
    return next;
  },
  list: async () => window.__terminalPolishFixture,
  snapshot: async () => [],
  write: async () => undefined,
  resize: async () => undefined,
  close: async (id) => {
    const index = window.__terminalPolishFixture.findIndex((item) => item.id === id);
    if (index >= 0) window.__terminalPolishFixture.splice(index, 1);
  },
  onEvent: () => () => undefined
};`;
}

function browserBridgeScript() {
	return `<script>
window.synthDesktop = {
  platform: "test",
  chooseWorkspaceDirectory: async () => null,
  getInstanceDiagnostics: async () => ({
    mode: "development", name: "bombadil", displayName: "Synth Desktop · bombadil",
    appVersion: "0.1.0", sourceRevision: "bombadil", buildRevision: "bombadil",
    buildTimestamp: "0", processId: 0, executable: "bombadil",
    dataRoot: "bombadil-memory://", viteUrl: window.location.origin, manifest: null
  })
};
window.synthLaguna = {
  getStatus: async () => ({ phase: "unavailable", baseUrl: null, backend: "stub", loadedModel: null, detail: "Bombadil fixture", memoryBytes: null, updatedAt: Date.now() }),
  reload: async () => ({ phase: "unavailable", baseUrl: null, backend: "stub", loadedModel: null, detail: "Bombadil fixture", memoryBytes: null, updatedAt: Date.now() }),
  listModels: async () => [],
  downloadModel: async () => { throw new Error("Model downloads require Synth Desktop"); },
  deleteModel: async () => { throw new Error("Model deletion requires Synth Desktop"); },
  chooseModelDirectory: async () => null,
  setModelDirectory: async () => { throw new Error("Model folders require the desktop app"); },
  clearModelDirectory: async () => undefined,
  onStatus: () => () => {}
};
${includeShellContainment ? `window.synthLaguna.getStatus = async () => ({
  phase: "ready", baseUrl: "http://127.0.0.1:7333", backend: "llama_cpp",
  loadedModel: "/models/Muse-Glimmer-30B-GGUF", detail: "Ready", memoryBytes: null,
  updatedAt: Date.now(), lastUsedAt: Date.now() - 60_000,
  idleSeconds: 60, idleUnloadAfterSeconds: 900, freeAt: Date.now() + 840_000
});` : ""}
${includeMuseResidencyHonesty ? `// CUA 2026-08-10 1:20 PM: Muse card with green ready chrome, Memory unavailable,
// and "Free scheduled … · awaiting unload" after freeAt elapsed — while Laguna-XS
// still paints ready underneath.
window.synthLaguna.getStatus = async () => ({
  phase: "ready", baseUrl: "http://127.0.0.1:7333", backend: "llama_cpp",
  loadedModel: "/models/Muse-Glimmer-30B-GGUF", detail: "Ready", memoryBytes: null,
  updatedAt: Date.now() - 25 * 60_000, lastUsedAt: Date.now() - 25 * 60_000,
  idleSeconds: 25 * 60, idleUnloadAfterSeconds: 900, freeAt: Date.now() - 60_000
});
window.synthLaguna.onStatus = (handler) => {
  handler({
    phase: "ready", baseUrl: "http://127.0.0.1:7333", backend: "llama_cpp",
    loadedModel: "/models/Muse-Glimmer-30B-GGUF", detail: "Ready", memoryBytes: null,
    updatedAt: Date.now() - 25 * 60_000, lastUsedAt: Date.now() - 25 * 60_000,
    idleSeconds: 25 * 60, idleUnloadAfterSeconds: 900, freeAt: Date.now() - 60_000
  });
  return () => {};
};` : ""}
${includeInferenceHonesty ? `globalThis.__SYNTH_TEST_INFERENCE_TRANSPORT__ = {
  snapshot: async () => ({
    model: "/models/Muse-Glimmer-30B-GGUF", resident: true, residentBytes: null,
    queueDepth: 0, queueCapacity: 9, active: null,
    rolling: {
      requestsCompleted: 1, requestsFailed: 0, requestsCancelled: 0,
      inputTokens: null, outputTokens: null, cachedTokens: null,
      ttftP50Ms: 1070, ttftP95Ms: 1070,
      decodeTpsP50: null, decodeTpsP95: null,
      latencyP50Ms: null, latencyP95Ms: null
    }
  }),
  subscribe: () => () => {},
  unload: async () => ({ released: true, conflict: false, detail: null })
};` : ""}
${includeBlankWorkedTurn ? blankWorkedTurnBridgeScript() : ""}
${includeRunSummarySanity ? `blankWorkedEvents[0].createdAt = "2026-08-09T15:44:50.000Z";` : ""}
${includeInferenceHonesty ? `window.synthCodex.list = async () => [{
  sessionId: "blank-worked-turn", threadId: "blank-thread", workspace: "/workspaces/default",
  model: "laguna-xs-2.1", providerName: "local-laguna", providerTitle: "Local Laguna",
  baseUrl: "http://127.0.0.1:7333/v1", status: "ready", title: "Hello",
  approvalPolicy: "untrusted", sandbox: "workspace-write"
}];` : ""}
${includeComposerToolbar ? composerToolbarBridgeScript() : ""}
${includeTerminalPolish ? terminalPolishBridgeScript() : ""}
${includeTraceCatalogLayout ? `window.synthInventory = {
  listContainers: async () => [],
  getContainer: async () => { throw new Error("fixture has no containers"); },
  registerContainer: async () => { throw new Error("not used"); },
  probeContainer: async () => { throw new Error("not used"); },
  listTraces: async () => [{
    id: "trace-bombadil-containment", digest: "sha256:bombadil-containment-fixture",
    title: "Bombadil Trace V5 viewport containment fixture", source: "import",
    containerId: null, sessionId: null, runId: null, reward: null, metrics: [],
    createdAt: "2026-08-10T16:00:00.000Z", path: "/tmp/bombadil-trace.json",
    metadata: { schemaVersion: "synth.trace.v5", model: "poolside/laguna-xs-2.1", events: 83, tools: 7, status: "completed", hasEvidence: true }
  }],
  getTrace: async () => { throw new Error("not used"); },
  chooseTraceInput: async () => null,
  ingestTraceBundle: async () => { throw new Error("not used"); },
  resolveTraceProjection: async () => { throw new Error("not used"); },
  listUsage: async () => [],
  counts: async () => ({ containers: 0, traces: 1, usage: 0 })
};` : ""}
// This is the exact payload shape produced by the CUA-observed Laguna prompt
// trim visual. Only the dedicated launch-debt spec receives it: the normal
// navigation spec must be able to visit Visuals without knowingly rendering
// an intentionally invalid debt fixture.
const cuaAnalysisVisual = {
  schemaVersion: "synth.desktop-visual.v1", id: "laguna-prompt-trim-preinstall", currentRevision: 1,
  title: "Laguna Prompt Trim Preinstall", templateId: "analysis.visual.v1", status: "draft", rendererKind: "template",
  bindings: { spec: { title: "Laguna Prompt Trim Preinstall", blocks: [
    { type: "metrics", items: [{ label: "Visual schemas before", value: "13" }, { label: "Advertised tools after", value: "1" }] },
    { type: "note", text: "Compact visual operations load only when needed." }
  ] } },
  sessionId: null, messageId: null, runId: null, traceId: null, parentVisualId: null,
  sourceAgentId: "laguna", sourceModel: "laguna-xs-2.1", contentDigest: null, previewDigest: null,
  metadata: {}, createdAt: "2026-08-09T13:24:48.000Z", updatedAt: "2026-08-09T13:24:48.000Z"
};
const fixtureVisuals = ${includeCuaAnalysisVisual ? "[cuaAnalysisVisual]" : "[]"};
window.synthVisuals = {
  listTemplates: async () => [{ id: "analysis.visual.v1", title: "Agent-authored analysis", genre: "analysis" }],
  getTemplate: async () => ({ id: "analysis.visual.v1", title: "Agent-authored analysis" }),
  list: async () => fixtureVisuals, get: async () => cuaAnalysisVisual, revisions: async () => [],
  create: async () => cuaAnalysisVisual, update: async () => cuaAnalysisVisual, save: async () => cuaAnalysisVisual,
  fork: async () => cuaAnalysisVisual, archive: async () => cuaAnalysisVisual, show: async () => cuaAnalysisVisual,
  onEvent: () => () => {}, onShow: () => () => {}
};
window.synthRuntime = {
  async request(path, options = {}) {
    const response = await fetch("/__runtime" + path, {
      method: options.method || "GET",
      headers: options.body === undefined ? {} : { "Content-Type": "application/json" },
      body: options.body === undefined ? undefined : JSON.stringify(options.body)
    });
    if (!response.ok) throw new Error("Runtime request failed (" + response.status + ")");
    return response.json();
  },
  async subscribe(sessionId, afterSequence, onEvent, onStatus) {
    let closed = false;
    let cursor = afterSequence || 0;
    onStatus?.({ state: "connected" });
    const poll = async () => {
      if (closed) return;
      try {
        const response = await window.synthRuntime.request("/v1/sessions/" + encodeURIComponent(sessionId) + "/events?after_sequence=" + cursor + "&limit=500");
        for (const event of response.events || []) {
          cursor = Math.max(cursor, Number(event.sequence) || cursor);
          onEvent(event);
        }
      } catch (error) {
        onStatus?.({ state: "reconnecting", detail: String(error) });
      }
      if (!closed) setTimeout(poll, 100);
    };
    void poll();
    return { close() { closed = true; } };
  }
};
</script>`;
}

async function proxyRuntime(request, response) {
	const path = request.url.slice("/__runtime".length) || "/";
	const chunks = [];
	for await (const chunk of request) chunks.push(chunk);
	const body = chunks.length ? Buffer.concat(chunks) : undefined;
	const upstream = await fetch(`${connection.url}${path}`, {
		method: request.method,
		headers: {
			...(connection.token ? { Authorization: `Bearer ${connection.token}` } : {}),
			...(request.headers["content-type"] ? { "Content-Type": request.headers["content-type"] } : {})
		},
		body: request.method === "GET" || request.method === "HEAD" ? undefined : body
	});
	response.writeHead(upstream.status, {
		"Content-Type": upstream.headers.get("content-type") || "application/json",
		"Cache-Control": "no-store"
	});
	response.end(Buffer.from(await upstream.arrayBuffer()));
}

const rendererServer = createServer(async (request, response) => {
	try {
		if ((request.url || "").startsWith("/__runtime")) {
			await proxyRuntime(request, response);
			return;
		}
		const pathname = new URL(request.url || "/", "http://127.0.0.1").pathname;
		let filePath = resolve(rendererRoot, `.${pathname === "/" ? "/index.html" : pathname}`);
		if (!filePath.startsWith(rendererRoot)) throw new Error("Path outside renderer root");
		if ((await stat(filePath)).isDirectory()) filePath = resolve(filePath, "index.html");
		let body = await readFile(filePath);
		if (filePath.endsWith("index.html")) {
			body = Buffer.from(body.toString("utf8").replace("<head>", `<head>${browserBridgeScript()}`));
		}
		response.writeHead(200, {
			"Content-Type": contentTypes[extname(filePath)] || "application/octet-stream",
			"Cache-Control": "no-store"
		});
		response.end(body);
	} catch (error) {
		response.writeHead(500, { "Content-Type": "text/plain" });
		response.end(error instanceof Error ? error.message : String(error));
	}
});

try {
	const pythonCandidates = [
		process.env.SYNTH_PYTHON,
		process.env.PYTHON,
		resolve(homedir(), ".synth-desktop/laguna/.venv/bin/python"),
		"/opt/homebrew/bin/python3.12",
		"python3"
	].filter(Boolean);
	const python = pythonCandidates.find((candidate) => candidate === "python3" || existsSync(candidate))
		|| "python3";
	runtimeProcess = spawn(python, [
		"-m", "synth_local_runtime",
		"--host", "127.0.0.1",
		"--port", "0",
		"--data-dir", resolve(runtimeHome, "data"),
		"--connection-file", connectionPath
	], {
		cwd: workshopRoot,
		env: {
			...process.env,
			PYTHONPATH: resolve(workshopRoot, "services/local-runtime/src"),
			SYNTH_INTERN_DEMO: "1",
			SYNTH_WORKSHOP_ROOT: workshopRoot,
			SYNTH_VISUALS_ROOT: resolve(workshopRoot, "visuals")
		},
		stdio: ["ignore", "pipe", "pipe"],
		detached: true
	});
	connection = await waitForConnection();
	await seedVisualAlignmentFixture();
	await seedTraceCatalogFixture();

	await new Promise((resolvePromise, reject) => {
		rendererServer.once("error", reject);
		rendererServer.listen(0, "127.0.0.1", resolvePromise);
	});
	const address = rendererServer.address();
	if (!address || typeof address === "string") throw new Error("Renderer server did not bind");
	const origin = `http://127.0.0.1:${address.port}`;

	bombadilProcess = spawn(bombadil, [
		"browser", "test",
		origin,
		specificationPath,
		"--headless",
		"--width", "1280",
		"--height", "840",
		"--chrome-grant-permissions", "",
		"--instrument-javascript", "",
		"--time-limit", timeLimit,
		// Muse residency honesty needs a few action ticks to expand the card
		// before details-row locks can observe the CUA copy.
		...(includeMuseResidencyHonesty ? [] : ["--exit-on-violation"]),
		"--output-path", outputPath,
		"--output-path-overwrite"
	], { cwd: workshopRoot, stdio: "inherit", detached: true });
	const code = await new Promise((resolvePromise, reject) => {
		const timeout = setTimeout(() => {
			if (bombadilProcess?.pid) process.kill(-bombadilProcess.pid, "SIGTERM");
			reject(new Error("Bombadil exceeded its test limit plus startup grace"));
		}, watchdogMs);
		bombadilProcess.once("error", reject);
		bombadilProcess.once("exit", (value) => {
			clearTimeout(timeout);
			resolvePromise(value ?? 1);
		});
	});
	if (code !== 0) process.exitCode = code;
} finally {
	if (bombadilProcess?.pid && !bombadilProcess.killed) {
		try { process.kill(-bombadilProcess.pid, "SIGTERM"); } catch { /* exited */ }
	}
	if (runtimeProcess?.pid && !runtimeProcess.killed) {
		// This process and its data directory are created solely for this test.
		// SIGKILL avoids the runtime's intentional daemon persistence on teardown.
		try { process.kill(-runtimeProcess.pid, "SIGKILL"); } catch { /* exited */ }
	}
	if (rendererServer.listening) {
		rendererServer.closeAllConnections();
		await new Promise((resolvePromise) => rendererServer.close(resolvePromise));
	}
	await rm(runtimeHome, { recursive: true, force: true });
}
