import { app, BrowserWindow, dialog, ipcMain, nativeImage, shell } from "electron";
import { spawn, type ChildProcess } from "node:child_process";
import { randomUUID } from "node:crypto";
import {
	closeSync,
	existsSync,
	mkdirSync,
	openSync,
	readFileSync,
	unlinkSync
} from "node:fs";
import { delimiter, join } from "node:path";
import {
	ensureLagunaSidecar,
	getLagunaApiKey,
	getLagunaStatus,
	onLagunaStatus,
	type LagunaStatus
} from "./laguna";

const isDev = !app.isPackaged;
const APP_NAME = "Synth Desktop";

type RuntimeConnection = {
	url: string;
	token: string | null;
};

type RuntimeRequest = {
	path: string;
	method?: "GET" | "POST" | "DELETE";
	body?: unknown;
};

type RuntimeSubscribeRequest = {
	subscriptionId: string;
	sessionId: string;
	afterSequence?: number;
};

type Subscription = {
	id: string;
	sessionId: string;
	afterSequence: number;
	controller: AbortController;
	ownerId: number;
};

let mainWindow: BrowserWindow | null = null;
let runtimeConnection: RuntimeConnection | null = null;
const subscriptions = new Map<string, Subscription>();

function sleep(milliseconds: number): Promise<void> {
	return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

/** Prefer PNG for dock — icns often fails to paint in electron-vite dev. */
function resolveAppIcon(): string | undefined {
	const roots = [join(__dirname, "../../resources"), join(app.getAppPath(), "resources")];
	for (const root of roots) {
		for (const name of ["icon.png", "icon.icns"]) {
			const candidate = join(root, name);
			if (existsSync(candidate)) return candidate;
		}
	}
	return undefined;
}

function applyDockIdentity(): void {
	app.setName(APP_NAME);
	if (process.platform !== "darwin" || !app.dock) return;
	const iconPath = resolveAppIcon();
	if (!iconPath) return;
	const img = nativeImage.createFromPath(iconPath);
	if (!img.isEmpty()) {
		app.dock.setIcon(img);
	}
}

function resolvePreloadPath(): string {
	const candidates = [
		join(__dirname, "../preload/index.js"),
		join(__dirname, "../preload/index.mjs"),
		join(__dirname, "../preload/index.cjs")
	];
	const found = candidates.find((p) => existsSync(p));
	if (!found) {
		throw new Error(`Preload script not found near ${candidates[0]}`);
	}
	return found;
}

/** Workshop monorepo root (`workshop/`). */
function resolveWorkshopRoot(): string {
	if (process.env.SYNTH_WORKSHOP_ROOT) {
		return process.env.SYNTH_WORKSHOP_ROOT;
	}
	// electron-vite: app.getAppPath() → apps/synth_desktop (dev) or asar root.
	// From apps/synth_desktop → ../.. is workshop/.
	const fromAppPath = join(app.getAppPath(), "../..");
	if (existsSync(join(fromAppPath, "services/local-runtime"))) {
		return fromAppPath;
	}
	// Compiled out/main → ../../.. is apps/, ../../../.. is workshop — prefer app path.
	const fromDirname = join(__dirname, "../../../..");
	if (existsSync(join(fromDirname, "services/local-runtime"))) {
		return fromDirname;
	}
	return fromAppPath;
}

function runtimeDirectory(): string {
	return (
		process.env.SYNTH_RUNTIME_HOME ||
		join(app.getPath("home"), ".synth-desktop", "runtime")
	);
}

function connectionFilePath(): string {
	return join(runtimeDirectory(), "connection.json");
}

function runtimeDataDirectory(): string {
	return join(runtimeDirectory(), "data");
}

function readConnectionFile(): RuntimeConnection | null {
	try {
		const value = JSON.parse(readFileSync(connectionFilePath(), "utf8")) as {
			url?: unknown;
			token?: unknown;
		};
		if (typeof value.url !== "string") return null;
		return {
			url: value.url.replace(/\/$/, ""),
			token: typeof value.token === "string" ? value.token : null
		};
	} catch {
		return null;
	}
}

async function checkHealth(connection: RuntimeConnection): Promise<boolean> {
	try {
		const response = await fetch(`${connection.url}/v1/health`, {
			headers: connection.token
				? { Authorization: `Bearer ${connection.token}` }
				: undefined,
			signal: AbortSignal.timeout(1_500)
		});
		if (!response.ok) return false;
		const health = (await response.json()) as { protocolVersion?: string };
		return health.protocolVersion === "synth.desktop-runtime.v1";
	} catch {
		return false;
	}
}

async function probeLocalInference(): Promise<string | null> {
	const status = getLagunaStatus();
	if (status.phase === "ready" && status.baseUrl) {
		return status.baseUrl;
	}
	const apiKey =
		getLagunaApiKey() ||
		process.env.SYNTH_LAGUNA_API_KEY ||
		process.env.SYNTH_COMPOSE_LAGUNA_API_KEY ||
		undefined;
	const candidates = [
		process.env.SYNTH_LAGUNA_BASE_URL,
		status.baseUrl,
		process.env.SYNTH_INFERENCE_URL,
		"http://127.0.0.1:7333",
		"http://127.0.0.1:7332"
	].filter((value): value is string => Boolean(value));
	const seen = new Set<string>();
	for (const raw of candidates) {
		const base = raw.replace(/\/$/, "");
		if (seen.has(base)) continue;
		seen.add(base);
		try {
			const response = await fetch(`${base}/health`, {
				headers: apiKey ? { Authorization: `Bearer ${apiKey}` } : undefined,
				signal: AbortSignal.timeout(800)
			});
			if (response.ok) {
				if (apiKey) process.env.SYNTH_LAGUNA_API_KEY = apiKey;
				return base;
			}
		} catch {
			/* try next */
		}
	}
	return null;
}

async function runtimeLocalMode(
	connection: RuntimeConnection
): Promise<"stub" | "mlx" | null> {
	try {
		const response = await fetch(`${connection.url}/v1/health`, {
			headers: connection.token
				? { Authorization: `Bearer ${connection.token}` }
				: undefined,
			signal: AbortSignal.timeout(1_500)
		});
		if (!response.ok) return null;
		const health = (await response.json()) as {
			local?: { mode?: string };
		};
		if (health.local?.mode === "mlx" || health.local?.mode === "stub") {
			return health.local.mode;
		}
		return null;
	} catch {
		return null;
	}
}

function broadcastLagunaStatus(status: LagunaStatus): void {
	for (const window of BrowserWindow.getAllWindows()) {
		if (!window.isDestroyed()) {
			window.webContents.send("laguna:status", status);
		}
	}
}

function spawnRuntime(options: {
	lagunaBaseUrl?: string | null;
}): { token: string; childPid: number | undefined } {
	const workshopRoot = resolveWorkshopRoot();
	const runtimeSource = join(workshopRoot, "services/local-runtime/src");
	const visualsRoot =
		process.env.SYNTH_VISUALS_ROOT || join(workshopRoot, "visuals");
	const connectionFile = connectionFilePath();
	const dataDirectory = runtimeDataDirectory();
	const logDirectory = runtimeDirectory();
	mkdirSync(logDirectory, { recursive: true });
	mkdirSync(dataDirectory, { recursive: true });
	try {
		unlinkSync(connectionFile);
	} catch {
		/* no prior connection file */
	}

	const token = randomUUID();
	const python = process.env.SYNTH_PYTHON || "python3";
	const logPath = join(logDirectory, "runtime.log");
	const logFd = openSync(logPath, "a");
	const existingPythonPath = process.env.PYTHONPATH;
	const childEnv: NodeJS.ProcessEnv = {
		...process.env,
		PYTHONPATH: existingPythonPath
			? `${runtimeSource}${delimiter}${existingPythonPath}`
			: runtimeSource,
		SYNTH_RUNTIME_TOKEN: token,
		SYNTH_WORKSHOP_ROOT: workshopRoot,
		SYNTH_VISUALS_ROOT: visualsRoot
	};
	if (options.lagunaBaseUrl) {
		childEnv.SYNTH_LAGUNA_BASE_URL = options.lagunaBaseUrl;
	}
	const apiKey = getLagunaApiKey() || process.env.SYNTH_LAGUNA_API_KEY;
	if (apiKey) {
		childEnv.SYNTH_LAGUNA_API_KEY = apiKey;
	}
	const child: ChildProcess = spawn(
		python,
		[
			"-m",
			"synth_local_runtime",
			"--host",
			"127.0.0.1",
			"--port",
			"0",
			"--data-dir",
			dataDirectory,
			"--connection-file",
			connectionFile
		],
		{
			cwd: workshopRoot,
			detached: true,
			stdio: ["ignore", logFd, logFd],
			env: childEnv
		}
	);
	child.unref();
	closeSync(logFd);
	return { token, childPid: child.pid };
}

async function ensureRuntime(): Promise<RuntimeConnection> {
	const workshopRoot = resolveWorkshopRoot();
	const lagunaBaseUrl =
		(await ensureLagunaSidecar(workshopRoot)) ||
		process.env.SYNTH_LAGUNA_BASE_URL ||
		(await probeLocalInference());
	if (lagunaBaseUrl) {
		process.env.SYNTH_LAGUNA_BASE_URL = lagunaBaseUrl;
	}

	if (process.env.SYNTH_RUNTIME_URL) {
		const manual: RuntimeConnection = {
			url: process.env.SYNTH_RUNTIME_URL.replace(/\/$/, ""),
			token: process.env.SYNTH_RUNTIME_TOKEN || null
		};
		if (!(await checkHealth(manual))) {
			throw new Error(`SYNTH_RUNTIME_URL is not healthy: ${manual.url}`);
		}
		runtimeConnection = manual;
		return runtimeConnection;
	}

	const existing = readConnectionFile();
	if (existing && (await checkHealth(existing))) {
		const mode = await runtimeLocalMode(existing);
		// Reuse only when already wired to Laguna, or when we have no sidecar.
		if (mode === "mlx" || !lagunaBaseUrl) {
			runtimeConnection = existing;
			return runtimeConnection;
		}
		// Stale stub connection while Laguna is now available — respawn.
		try {
			unlinkSync(connectionFilePath());
		} catch {
			/* ignore */
		}
	}

	spawnRuntime({ lagunaBaseUrl });
	const deadline = Date.now() + 15_000;
	while (Date.now() < deadline) {
		const candidate = readConnectionFile();
		if (candidate && (await checkHealth(candidate))) {
			runtimeConnection = candidate;
			return runtimeConnection;
		}
		await sleep(120);
	}
	throw new Error(
		`The local runtime did not start. See ${join(runtimeDirectory(), "runtime.log")}`
	);
}

function validatedRuntimePath(value: string): string {
	if (typeof value !== "string" || !value.startsWith("/v1/")) {
		throw new Error("Only versioned local runtime paths are allowed");
	}
	if (value.includes("\\") || value.startsWith("//")) {
		throw new Error("Invalid local runtime path");
	}
	return value;
}

async function runtimeFetch(
	runtimePath: string,
	options: { method?: string; body?: unknown } = {}
): Promise<unknown> {
	const connection = runtimeConnection || (await ensureRuntime());
	const safePath = validatedRuntimePath(runtimePath);
	const method = options.method || "GET";
	if (!["GET", "POST", "DELETE"].includes(method)) {
		throw new Error(`Unsupported runtime method: ${method}`);
	}
	const headers: Record<string, string> = {
		Accept: "application/json",
		...(connection.token ? { Authorization: `Bearer ${connection.token}` } : {})
	};
	let body: string | undefined;
	if (options.body !== undefined) {
		headers["Content-Type"] = "application/json";
		body = JSON.stringify(options.body);
	}
	const response = await fetch(`${connection.url}${safePath}`, {
		method,
		headers,
		body,
		signal: AbortSignal.timeout(60_000)
	});
	const text = await response.text();
	let payload: unknown = null;
	if (text) {
		try {
			payload = JSON.parse(text);
		} catch {
			payload = { raw: text };
		}
	}
	if (!response.ok) {
		const record = payload as { error?: { message?: string }; detail?: string } | null;
		const message =
			record?.error?.message ||
			record?.detail ||
			`Runtime request failed (${response.status})`;
		const error = new Error(message) as Error & { status?: number; payload?: unknown };
		error.status = response.status;
		error.payload = payload;
		throw error;
	}
	return payload;
}

function parseSseFrame(frame: string): {
	eventName: string;
	id: string | null;
	data: string;
} | null {
	let eventName = "message";
	let id: string | null = null;
	const dataLines: string[] = [];
	for (const line of frame.split(/\r?\n/)) {
		if (!line || line.startsWith(":")) continue;
		const separator = line.indexOf(":");
		const field = separator === -1 ? line : line.slice(0, separator);
		let value = separator === -1 ? "" : line.slice(separator + 1);
		if (value.startsWith(" ")) value = value.slice(1);
		if (field === "event") eventName = value;
		if (field === "id") id = value;
		if (field === "data") dataLines.push(value);
	}
	if (!dataLines.length) return null;
	return { eventName, id, data: dataLines.join("\n") };
}

async function streamSubscription(
	webContents: Electron.WebContents,
	subscription: Subscription
): Promise<void> {
	const connection = runtimeConnection || (await ensureRuntime());
	let cursor = Number(subscription.afterSequence) || 0;
	let retryMilliseconds = 500;
	while (!subscription.controller.signal.aborted && !webContents.isDestroyed()) {
		try {
			webContents.send("runtime:subscription", {
				subscriptionId: subscription.id,
				type: "status",
				status: { state: "connecting" }
			});
			const response = await fetch(
				`${connection.url}/v1/sessions/${encodeURIComponent(subscription.sessionId)}/events/stream?after_sequence=${cursor}`,
				{
					headers: {
						Accept: "text/event-stream",
						...(connection.token
							? { Authorization: `Bearer ${connection.token}` }
							: {})
					},
					signal: subscription.controller.signal
				}
			);
			if (!response.ok || !response.body) {
				throw new Error(`Event stream failed (${response.status})`);
			}
			webContents.send("runtime:subscription", {
				subscriptionId: subscription.id,
				type: "status",
				status: { state: "connected" }
			});
			retryMilliseconds = 500;
			const reader = response.body.getReader();
			const decoder = new TextDecoder();
			let buffer = "";
			while (!subscription.controller.signal.aborted) {
				const { done, value } = await reader.read();
				if (done) break;
				buffer += decoder.decode(value, { stream: true }).replace(/\r\n/g, "\n");
				let boundary: number;
				while ((boundary = buffer.indexOf("\n\n")) !== -1) {
					const rawFrame = buffer.slice(0, boundary);
					buffer = buffer.slice(boundary + 2);
					const frame = parseSseFrame(rawFrame);
					if (!frame) continue;
					const event = JSON.parse(frame.data) as { sequence?: number };
					cursor = Math.max(
						cursor,
						Number(event.sequence) || Number(frame.id) || cursor
					);
					webContents.send("runtime:subscription", {
						subscriptionId: subscription.id,
						type: "event",
						event
					});
				}
			}
		} catch (error) {
			if (subscription.controller.signal.aborted || webContents.isDestroyed()) break;
			webContents.send("runtime:subscription", {
				subscriptionId: subscription.id,
				type: "status",
				status: {
					state: "reconnecting",
					detail: error instanceof Error ? error.message : String(error)
				}
			});
			await sleep(retryMilliseconds);
			retryMilliseconds = Math.min(retryMilliseconds * 2, 5_000);
		}
	}
}

function registerIpc(): void {
	ipcMain.handle("runtime:request", async (_event, request: RuntimeRequest) => {
		return runtimeFetch(request.path, {
			method: request.method,
			body: request.body
		});
	});

	ipcMain.handle("laguna:getStatus", async () => getLagunaStatus());

	ipcMain.handle("project:chooseDirectory", async () => {
		const result = mainWindow
			? await dialog.showOpenDialog(mainWindow, {
				title: "Choose a project folder",
				properties: ["openDirectory", "createDirectory"]
			})
			: await dialog.showOpenDialog({
			title: "Choose a project folder",
			properties: ["openDirectory", "createDirectory"]
			});
		return result.canceled ? null : (result.filePaths[0] ?? null);
	});

	ipcMain.handle(
		"runtime:subscribe",
		async (event, request: RuntimeSubscribeRequest) => {
			const id = String(request.subscriptionId);
			const existing = subscriptions.get(id);
			existing?.controller.abort();
			const subscription: Subscription = {
				id,
				sessionId: String(request.sessionId),
				afterSequence: Number(request.afterSequence) || 0,
				controller: new AbortController(),
				ownerId: event.sender.id
			};
			subscriptions.set(id, subscription);
			void streamSubscription(event.sender, subscription).finally(() => {
				if (subscriptions.get(id) === subscription) subscriptions.delete(id);
			});
			return { subscriptionId: id };
		}
	);

	ipcMain.on("runtime:unsubscribe", (_event, subscriptionId: string) => {
		const subscription = subscriptions.get(String(subscriptionId));
		if (subscription) {
			subscription.controller.abort();
			subscriptions.delete(String(subscriptionId));
		}
	});
}

async function createWindow(): Promise<void> {
	const iconPath = resolveAppIcon();
	const icon = iconPath ? nativeImage.createFromPath(iconPath) : undefined;

	mainWindow = new BrowserWindow({
		width: 1280,
		height: 840,
		minWidth: 960,
		minHeight: 640,
		show: false,
		title: "Synth Desktop",
		backgroundColor: "#f3f5f8",
		...(icon && !icon.isEmpty() ? { icon } : {}),
		titleBarStyle: process.platform === "darwin" ? "hiddenInset" : "default",
		trafficLightPosition: { x: 16, y: 13 },
		movable: true,
		webPreferences: {
			preload: resolvePreloadPath(),
			sandbox: false,
			contextIsolation: true,
			nodeIntegration: false
		}
	});

	mainWindow.on("ready-to-show", () => {
		applyDockIdentity();
		mainWindow?.setTitle("Synth Desktop");
		mainWindow?.show();
	});

	mainWindow.webContents.setWindowOpenHandler((details) => {
		if (details.url.startsWith("https://") || details.url.startsWith("http://")) {
			void shell.openExternal(details.url);
		}
		return { action: "deny" };
	});

	mainWindow.webContents.on("will-navigate", (event, url) => {
		const current = mainWindow?.webContents.getURL();
		if (current && url !== current) event.preventDefault();
	});

	mainWindow.webContents.on("destroyed", () => {
		const ownerId = mainWindow?.webContents.id;
		for (const [id, subscription] of subscriptions) {
			if (ownerId != null && subscription.ownerId === ownerId) {
				subscription.controller.abort();
				subscriptions.delete(id);
			}
		}
	});

	// Show UI immediately; Laguna + runtime come up in the background so the
	// sidebar can animate starting → loading → ready.
	broadcastLagunaStatus({
		...getLagunaStatus(),
		phase: "starting",
		detail: "Starting Laguna XS…"
	});

	if (isDev && process.env.ELECTRON_RENDERER_URL) {
		await mainWindow.loadURL(process.env.ELECTRON_RENDERER_URL);
	} else {
		await mainWindow.loadFile(join(__dirname, "../renderer/index.html"));
	}

	try {
		await ensureRuntime();
		broadcastLagunaStatus(getLagunaStatus());
	} catch (error) {
		console.error(error);
		broadcastLagunaStatus({
			...getLagunaStatus(),
			phase: "error",
			detail: error instanceof Error ? error.message : String(error)
		});
	}
}

app.setName(APP_NAME);

app.whenReady().then(async () => {
	applyDockIdentity();

	registerIpc();
	onLagunaStatus(broadcastLagunaStatus);
	try {
		await createWindow();
	} catch (error) {
		console.error(error);
		app.quit();
		return;
	}

	app.on("activate", () => {
		if (BrowserWindow.getAllWindows().length === 0) {
			void createWindow();
		}
	});
});

app.on("window-all-closed", () => {
	if (process.platform !== "darwin") app.quit();
});

app.on("before-quit", () => {
	for (const subscription of subscriptions.values()) {
		subscription.controller.abort();
	}
	subscriptions.clear();
	// Deliberately do not stop the runtime daemon. Async Intern work and the
	// local replay cache remain available when Electron is reopened.
});
