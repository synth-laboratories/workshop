import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { mkdtemp, readFile, rm, stat } from "node:fs/promises";
import { tmpdir } from "node:os";
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
const outputPath = resolve(appRoot, "test-results/bombadil");
const specificationPath = process.env.BOMBADIL_SPEC
	? resolve(workshopRoot, process.env.BOMBADIL_SPEC)
	: resolve(here, "layout.spec.ts");
const includeCuaAnalysisVisual = specificationPath.endsWith("launch-debt.spec.ts");
// Five seconds covers every directed/eventual horizon in layout.spec.ts. Longer
// runs intermittently wedge the current Chromiumoxide transport after the
// properties have already been exercised, turning a clean trace into a harness
// watchdog failure. Nightly exploration can still opt into a longer duration.
const timeLimit = process.env.BOMBADIL_TIME_LIMIT || "5s";
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
  onStatus: () => () => {}
};
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
	runtimeProcess = spawn("python3", [
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
		"--exit-on-violation",
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
