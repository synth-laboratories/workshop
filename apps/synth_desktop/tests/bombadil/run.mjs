import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { existsSync, mkdirSync, readdirSync, symlinkSync } from "node:fs";
import { mkdtemp, readFile, rm, stat } from "node:fs/promises";
import { homedir, tmpdir } from "node:os";
import { dirname, extname, join, resolve } from "node:path";
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
function playwrightChromeBinary() {
	const cache = join(homedir(), "Library/Caches/ms-playwright");
	if (!existsSync(cache)) return null;
	const suffixes = [
		"chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
		"chrome-mac/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"
	];
	for (const name of readdirSync(cache)) {
		for (const suffix of suffixes) {
			const bin = join(cache, name, suffix);
			if (existsSync(bin)) return bin;
		}
	}
	return null;
}
function ensureChromeOnPath() {
	const systemChrome = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
	const bin = existsSync(systemChrome) ? systemChrome : playwrightChromeBinary();
	if (!bin) return;
	process.env.CHROME = bin;
	const shimDir = join(runtimeHome, "chrome-bin");
	mkdirSync(shimDir, { recursive: true });
	for (const name of ["google-chrome", "chromium", "chrome"]) {
		const shim = join(shimDir, name);
		try { symlinkSync(bin, shim); } catch { /* already linked */ }
	}
	process.env.PATH = `${shimDir}:${process.env.PATH ?? ""}`;
}
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
	|| specificationPath.endsWith("visual-library-layout.spec.ts")
	|| specificationPath.endsWith("visual-pane-boundaries.spec.ts");
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
const includeVisualContracts = specificationPath.endsWith("v0.1-visual-contracts.spec.ts");
const includeChatgptBranding = specificationPath.endsWith("chatgpt-branding.spec.ts");
const includeApprovalCard = specificationPath.endsWith("approval-card.spec.ts");
const includeGroupedVisualEvidence = specificationPath.endsWith("grouped-visual-evidence.spec.ts");
// Five seconds covers every directed/eventual horizon in layout.spec.ts. Longer
// runs intermittently wedge the current Chromiumoxide transport after the
// properties have already been exercised, turning a clean trace into a harness
// watchdog failure. Nightly exploration can still opt into a longer duration.
const timeLimit = process.env.BOMBADIL_TIME_LIMIT
	|| (includeComposerToolbar
		? "45s"
		: includeBlankWorkedTurn || includeTerminalPolish || includeVisualContracts || includeChatgptBranding || includeApprovalCard || includeGroupedVisualEvidence
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
    measurementKind: "observed_stream_segment",
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
 * Seeds a waiting shell-approval turn: pending approval card above Working…,
 * with Reject / Approve once / Always allow. Used by approval-card.spec.ts.
 */
function approvalCardBridgeScript() {
	return `
const approvalSessionId = "v02-approval-session";
window.__approvalResolverCalls = [];
function approvalAppEvent(sequence, kind, payload) {
  return {
    schemaVersion: "synth.desktop-app-event.v1",
    sequence,
    eventId: "approval-" + sequence,
    sessionId: approvalSessionId,
    sessionSequence: sequence,
    runId: "turn-approval",
    source: "codex",
    kind,
    payload,
    createdAt: "2026-08-13T13:00:0" + sequence + "Z"
  };
}
const approvalEvents = [
  approvalAppEvent(1, "message.created", { messageId: "user-1", role: "user", content: "list the workspace" }),
  approvalAppEvent(2, "run.started", { runId: "turn-approval" }),
  approvalAppEvent(3, "approval.requested", {
    approvalId: "appr_shell_1",
    kind: "shell_command",
    command: "ls /Users/joshuapurtell",
    detail: "ls /Users/joshuapurtell",
    alwaysSupported: true
  })
];
const approvalListeners = [];
window.__bombadilApprovalDecisions = [];
window.synthLaguna.getStatus = async () => ({
  phase: "ready", baseUrl: "http://127.0.0.1:7333", backend: "mlx_lm",
  loadedModel: "/models/Laguna-XS-2.1-NVFP4-mlx", detail: "Ready", memoryBytes: null,
  updatedAt: Date.now(), lastUsedAt: Date.now(), idleSeconds: 0,
  idleUnloadAfterSeconds: 900, freeAt: Date.now() + 900000
});
window.synthCodex = {
  defaultWorkspace: async () => "/Users/joshuapurtell",
  list: async () => [{
    sessionId: approvalSessionId,
    threadId: "v02-approval-thread",
    workspace: "/Users/joshuapurtell",
    model: "poolside/Laguna-XS-2.1-NVFP4-mlx",
    providerName: "local-laguna",
    providerTitle: "Laguna XS Responses",
    baseUrl: "http://127.0.0.1:7333/v1",
    status: "running",
    title: "Waiting approval",
    approvalPolicy: "untrusted",
    sandbox: "workspace-write"
  }],
  start: async () => ({ sessionId: approvalSessionId, threadId: "v02-approval-thread" }),
  startTurn: async () => ({ sessionId: approvalSessionId, threadId: "v02-approval-thread", turnId: "turn-approval" }),
  interrupt: async () => undefined,
  resolveApproval: async (sessionId, approvalId, decision) => {
    window.__bombadilApprovalDecisions.push({ sessionId, approvalId, decision });
    window.__approvalResolverCalls.push([sessionId, approvalId, decision]);
    const event = {
      sessionId,
      method: decision === "reject" ? "approval.rejected" : "approval.granted",
      params: { approvalId, decision }
    };
    approvalListeners.forEach((listener) => listener(event));
  },
  close: async () => undefined,
  onEvent: (listener) => {
    approvalListeners.push(listener);
    return () => {
      const index = approvalListeners.indexOf(listener);
      if (index >= 0) approvalListeners.splice(index, 1);
    };
  }
};
window.synthCore = {
  diagnostics: async () => ({
    databasePath: "bombadil-memory://core",
    schemaVersion: 0,
    integrityOk: true,
    contentStorePath: "bombadil-memory://content",
    journalHead: approvalEvents.length,
    sessionCount: 1,
    runCount: 1,
    visualCount: 0,
    migrationComplete: true
  }),
  eventsAfter: async () => approvalEvents,
  sessionEventsAfter: async (sessionId) => sessionId === approvalSessionId ? approvalEvents : [],
  onEvent: () => () => undefined
};
window.synthCodexOauth = {
  begin: async () => { throw new Error("not used"); },
  completeManual: async () => ({ state: "connected", action: "none", canUseModels: true, configured: true }),
  status: async () => ({ state: "connected", action: "none", canUseModels: true, configured: true }),
  ensureReady: async () => ({ state: "connected", action: "none", canUseModels: true, configured: true }),
  disconnect: async () => ({ state: "disconnected", action: "connect", canUseModels: false, configured: false }),
  cancel: async () => undefined
};
window.synthConfig = {
  get: async () => ({
    configPath: "/tmp/config.toml", envFile: "/tmp/.env", profile: "test",
    backendUrl: "https://api.usesynth.ai", apiKeyEnv: "SYNTH_API_KEY",
    apiKeyConfigured: true, workerKeyConfigured: false, openrouterApiKeyConfigured: true
  }),
  update: async () => { throw new Error("not used"); },
  listModelMultiAgent: async () => [], updateModelMultiAgent: async () => [],
  getWorkspaceAccess: async () => ({ allowedRoots: [] }),
  updateWorkspaceAccess: async () => ({ allowedRoots: [] }),
  getDesktopPermissions: async () => ({ approvalPolicy: "untrusted", sandboxMode: "workspace-write" }),
  updateDesktopPermissions: async (request) => request
};
`;
}

/**
 * Seeds the 2026-08-13 CUA W1 turn: assistant preamble plus four mixed
 * container / shell / visual MCP calls that grouped mode used to collapse
 * into "Ran commands, used tools 4 calls".
 */
function groupedVisualEvidenceBridgeScript() {
	return `
const groupedSessionId = "v02-grouped-visual-session";
try {
  localStorage.setItem("synth.accountChoiceMade", "1");
  const prefs = JSON.parse(localStorage.getItem("synth.preferences.v1") || "{}");
  const layout = prefs.layout && typeof prefs.layout === "object" ? prefs.layout : {};
  const last = layout.last && typeof layout.last === "object" ? layout.last : {};
  localStorage.setItem("synth.preferences.v1", JSON.stringify({
    schemaVersion: 4,
    ...prefs,
    toolActivity: { mode: "grouped" },
    layout: {
      ...layout,
      last: { ...last, selectedConversationId: groupedSessionId }
    }
  }));
} catch (error) {
  console.warn("bombadil grouped-visual prefs seed failed", error);
}
function groupedAppEvent(sequence, kind, payload) {
  return {
    schemaVersion: "synth.desktop-app-event.v1",
    sequence,
    eventId: "grouped-" + sequence,
    sessionId: groupedSessionId,
    sessionSequence: sequence,
    runId: "turn-w1-craftax",
    source: "codex",
    kind,
    payload,
    createdAt: "2026-08-13T13:58:" + String(sequence).padStart(2, "0") + "Z"
  };
}
const craftaxVisual = {
  id: "vis_w1_craftax",
  templateId: "live.craftax.v1",
  title: "Craftax live",
  messageId: "asst-w1",
  bindings: {
    schemaVersion: "synth.visual-bindings.v1",
    slots: [{
      slot: "stream", kind: "inline", schema: "synth.trace-stream-event.v1",
      data: {
        events: [
          { ts: "2026-08-13T13:58:00Z", kind: "stream.subscribed", payload: { "stream.id": "stream_craftax_w1", next_sequence: 1 } },
          { ts: "2026-08-13T13:58:01Z", kind: "observation", payload: { text: "You see a tree.", step: 0 } },
          { ts: "2026-08-13T13:58:02Z", kind: "action", payload: { name: "collect_wood", step: 1 } }
        ]
      }
    }]
  }
};
const groupedEvents = [
  groupedAppEvent(1, "message.created", {
    messageId: "user-w1", role: "user",
    content: "find craftax rust gambeench containers, register it, run 10 steps, create a visual with the trace data/rewards you get"
  }),
  groupedAppEvent(2, "run.started", { runId: "turn-w1-craftax" }),
  groupedAppEvent(3, "message.created", {
    messageId: "asst-w1", role: "assistant",
    content: "I'm using the Synth container skill to discover/register the Craftax Rust GameBench container, then the Synth visuals skill to build a visual from the resulting trace and reward evidence."
  }),
  groupedAppEvent(4, "item/completed", {
    item: {
      type: "mcpToolCall", id: "call-list", server: "synth_containers",
      tool: "container_list", status: "completed", arguments: {}
    }
  }),
  groupedAppEvent(5, "item/completed", {
    item: {
      type: "mcpToolCall", id: "call-register", server: "synth_containers",
      tool: "container_register", status: "completed",
      arguments: { name: "craftax-rust", base_url: "http://127.0.0.1:8097" },
      result: { structuredContent: { container: { id: "ctr_craftax" } } }
    }
  }),
  groupedAppEvent(6, "item/completed", {
    item: { type: "commandExecution", id: "call-shell-1", command: "pwd", status: "completed" }
  }),
  groupedAppEvent(7, "item/completed", {
    item: { type: "commandExecution", id: "call-shell-2", command: "ls", status: "completed" }
  }),
  groupedAppEvent(8, "item/completed", {
    item: {
      type: "mcpToolCall", id: "call-visual", server: "synth_visuals",
      tool: "visual_create_from_template", status: "completed",
      arguments: { template_id: "live.craftax.v1", title: "Craftax live" },
      result: { structuredContent: { visual: craftaxVisual } }
    }
  }),
  groupedAppEvent(9, "run.completed", { runId: "turn-w1-craftax" })
];
window.synthLaguna.getStatus = async () => ({
  phase: "ready", baseUrl: "http://127.0.0.1:7333", backend: "mlx_lm",
  loadedModel: "/models/Laguna-XS-2.1-NVFP4-mlx", detail: "Ready", memoryBytes: null,
  updatedAt: Date.now(), lastUsedAt: Date.now(), idleSeconds: 0,
  idleUnloadAfterSeconds: 900, freeAt: Date.now() + 900000
});
window.synthCodex = {
  defaultWorkspace: async () => "/Users/joshuapurtell",
  list: async () => [{
    sessionId: groupedSessionId,
    threadId: "v02-grouped-thread",
    workspace: "/Users/joshuapurtell",
    model: "poolside/Laguna-XS-2.1-NVFP4-mlx",
    providerName: "local-laguna",
    providerTitle: "Laguna XS Responses",
    baseUrl: "http://127.0.0.1:7333/v1",
    status: "ready",
    title: "Find craftax rust gambeench",
    approvalPolicy: "untrusted",
    sandbox: "workspace-write"
  }],
  start: async () => ({ sessionId: groupedSessionId, threadId: "v02-grouped-thread" }),
  startTurn: async () => ({ sessionId: groupedSessionId, threadId: "v02-grouped-thread", turnId: "turn-w1-craftax" }),
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
    journalHead: groupedEvents.length,
    sessionCount: 1,
    runCount: 1,
    visualCount: 1,
    migrationComplete: true
  }),
  eventsAfter: async () => groupedEvents,
  sessionEventsAfter: async (sessionId) => sessionId === groupedSessionId ? groupedEvents : [],
  onEvent: () => () => undefined
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

window.synthConfig = {
  get: async () => ({
    configPath: "/tmp/config.toml", envFile: "/tmp/.env", profile: "test",
    backendUrl: "https://api.usesynth.ai", apiKeyEnv: "SYNTH_API_KEY",
    apiKeyConfigured: true, workerKeyConfigured: false, openrouterApiKeyConfigured: true
  }),
  update: async () => { throw new Error("not used"); },
  listModelMultiAgent: async () => [], updateModelMultiAgent: async () => [],
  getWorkspaceAccess: async () => ({ allowedRoots: [] }),
  updateWorkspaceAccess: async () => ({ allowedRoots: [] })
};
window.synthCodexOauth = {
  begin: async () => { throw new Error("not used"); },
  completeManual: async () => ({ configured: true, accountHint: "bombadil@example.com" }),
  status: async () => ({ configured: true, accountHint: "bombadil@example.com" }),
  disconnect: async () => ({ configured: false }),
  cancel: async () => undefined
};
window.synthLaguna.getStatus = async () => ({
  phase: "ready", baseUrl: "http://127.0.0.1:7333", backend: "mlx_lm",
  loadedModel: "/models/Laguna-XS-2.1-NVFP4-mlx", detail: "Ready", memoryBytes: null,
  updatedAt: Date.now(), lastUsedAt: Date.now(), idleSeconds: 0,
  idleUnloadAfterSeconds: 900, freeAt: Date.now() + 900000
});

window.synthModelPerformance = {
  summaries: async () => [{
    provider: "openrouter",
    modelId: "poolside/laguna-s-2.1",
    measurementKind: "observed_stream_segment",
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
  phase: "ready", baseUrl: "http://127.0.0.1:7333", backend: "mlx_lm",
  loadedModel: "/models/Laguna-XS-2.1-NVFP4-mlx", detail: "Ready", memoryBytes: null,
  updatedAt: Date.now(), lastUsedAt: Date.now() - 60_000,
  idleSeconds: 60, idleUnloadAfterSeconds: 900, freeAt: Date.now() + 840_000
});` : ""}
${includeInferenceHonesty ? `globalThis.__SYNTH_TEST_INFERENCE_TRANSPORT__ = {
  snapshot: async () => ({
    model: "/models/Laguna-XS-2.1-NVFP4-mlx", resident: true, residentBytes: null,
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
${includeApprovalCard ? approvalCardBridgeScript() : ""}
${includeGroupedVisualEvidence ? groupedVisualEvidenceBridgeScript() : ""}
${includeRunSummarySanity ? `blankWorkedEvents[0].createdAt = "2026-08-09T15:44:50.000Z";` : ""}
${includeInferenceHonesty ? `window.synthCodex.list = async () => [{
  sessionId: "blank-worked-turn", threadId: "blank-thread", workspace: "/workspaces/default",
  model: "laguna-xs-2.1", providerName: "local-laguna", providerTitle: "Local Laguna",
  baseUrl: "http://127.0.0.1:7333/v1", status: "ready", title: "Hello",
  approvalPolicy: "untrusted", sandbox: "workspace-write"
}];` : ""}
${includeComposerToolbar ? composerToolbarBridgeScript() : ""}
${includeTerminalPolish ? terminalPolishBridgeScript() : ""}
${includeChatgptBranding ? `window.synthCodexOauth = {
  begin: async () => { throw new Error("not used"); },
  completeManual: async () => ({ configured: true, accountHint: "test@openai.com" }),
  status: async () => ({ configured: true, accountHint: "test@openai.com" }),
  disconnect: async () => ({ configured: false }),
  cancel: async () => undefined
};` : ""}
${includeVisualContracts ? `window.synthConfig = {
  get: async () => ({
    configPath: "/tmp/config.toml", envFile: "/tmp/.env", profile: "prod",
    backendUrl: "https://api.usesynth.ai", apiKeyEnv: "SYNTH_API_KEY",
    apiKeyConfigured: true, workerKeyConfigured: false, openrouterApiKeyConfigured: true
  }),
  update: async () => { throw new Error("not used"); },
  listModelMultiAgent: async () => [], updateModelMultiAgent: async () => [],
  getWorkspaceAccess: async () => ({ allowedRoots: [] }),
  updateWorkspaceAccess: async () => ({ allowedRoots: [] })
};` : ""}
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
const cuaLongTitleVisual = {
  ...cuaAnalysisVisual,
  id: "laguna-prompt-trim-long-title",
  title: "SFT · Banking77 intent SFT · hosted Tinker · banking77_classify checkpoint campaigns with a deliberately taller wrapped title",
  updatedAt: "2026-08-09T13:25:48.000Z"
};
const groupedCraftaxVisual = {
  schemaVersion: "synth.desktop-visual.v1", id: "vis_w1_craftax", currentRevision: 1,
  title: "Craftax live", templateId: "live.craftax.v1", status: "saved", rendererKind: "template",
  bindings: {
    schemaVersion: "synth.visual-bindings.v1",
    slots: [{
      slot: "stream", kind: "inline", schema: "synth.trace-stream-event.v1",
      data: { events: [] }
    }]
  },
  sessionId: "v02-grouped-visual-session", messageId: "asst-w1", runId: "turn-w1-craftax",
  metadata: {}, createdAt: "2026-08-13T13:58:08Z", updatedAt: "2026-08-13T13:58:08Z"
};
const fixtureVisuals = ${includeCuaAnalysisVisual ? "[cuaAnalysisVisual, cuaLongTitleVisual]" : includeGroupedVisualEvidence ? "[groupedCraftaxVisual]" : "[]"};
window.synthVisuals = {
  listTemplates: async () => [{ id: ${includeGroupedVisualEvidence ? `"live.craftax.v1"` : `"analysis.visual.v1"`}, title: ${includeGroupedVisualEvidence ? `"Craftax live eval"` : `"Agent-authored analysis"`}, genre: ${includeGroupedVisualEvidence ? `"live"` : `"analysis"`} }],
  getTemplate: async () => ({ id: ${includeGroupedVisualEvidence ? `"live.craftax.v1"` : `"analysis.visual.v1"`}, title: ${includeGroupedVisualEvidence ? `"Craftax live eval"` : `"Agent-authored analysis"`} }),
  list: async () => fixtureVisuals, get: async (visualId) => fixtureVisuals.find((visual) => visual.id === visualId) || ${includeGroupedVisualEvidence ? "groupedCraftaxVisual" : "cuaAnalysisVisual"}, revisions: async () => [],
  create: async () => ${includeGroupedVisualEvidence ? "groupedCraftaxVisual" : "cuaAnalysisVisual"}, update: async () => ${includeGroupedVisualEvidence ? "groupedCraftaxVisual" : "cuaAnalysisVisual"}, save: async () => ${includeGroupedVisualEvidence ? "groupedCraftaxVisual" : "cuaAnalysisVisual"},
  fork: async () => ${includeGroupedVisualEvidence ? "groupedCraftaxVisual" : "cuaAnalysisVisual"}, archive: async () => ${includeGroupedVisualEvidence ? "groupedCraftaxVisual" : "cuaAnalysisVisual"}, show: async () => ${includeGroupedVisualEvidence ? "groupedCraftaxVisual" : "cuaAnalysisVisual"},
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

	ensureChromeOnPath();
	const remoteDebugger = process.env.BOMBADIL_REMOTE_DEBUGGER;
	bombadilProcess = spawn(bombadil, [
		"browser", remoteDebugger ? "test-external" : "test",
		...(remoteDebugger ? ["--remote-debugger", remoteDebugger, "--create-target"] : []),
		origin,
		specificationPath,
		...(remoteDebugger ? [] : ["--headless"]),
		"--width", "1280",
		"--height", "840",
		"--chrome-grant-permissions", "",
		"--instrument-javascript", "",
		"--time-limit", timeLimit,
		"--exit-on-violation",
		"--output-path", outputPath,
		"--output-path-overwrite"
	], { cwd: workshopRoot, stdio: "inherit", detached: true, env: { ...process.env } });
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
