/**
 * Laguna XS sidecar lifecycle for Synth Desktop.
 *
 * On app start we ensure an OpenAI-compatible sidecar is listening (default
 * :7333), preferring an already-running instance, then Poolside's Metal
 * binary as upstream (model_type=laguna), then vanilla mlx_lm as a fallback.
 */
import { spawn, execFile, type ChildProcess } from "node:child_process";
import { randomBytes } from "node:crypto";
import {
	chmodSync,
	closeSync,
	existsSync,
	mkdirSync,
	openSync,
	readFileSync,
	writeFileSync
} from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

export type LagunaPhase =
	| "unknown"
	| "starting"
	| "loading"
	| "ready"
	| "error"
	| "unavailable";

export type LagunaStatus = {
	phase: LagunaPhase;
	baseUrl: string | null;
	backend: string | null;
	loadedModel: string | null;
	detail: string | null;
	memoryBytes: number | null;
	updatedAt: number;
};

const DEFAULT_PORT = 7333;
const DEFAULT_MODEL = "poolside/Laguna-XS-2.1-NVFP4-mlx";
const POOLSIDE_PORTS = [63300, 49600];

let sidecarChild: ChildProcess | null = null;
let currentStatus: LagunaStatus = {
	phase: "unknown",
	baseUrl: null,
	backend: null,
	loadedModel: null,
	detail: null,
	memoryBytes: null,
	updatedAt: Date.now()
};
const listeners = new Set<(status: LagunaStatus) => void>();

function lagunaHome(): string {
	return (
		process.env.SYNTH_LAGUNA_HOME || join(homedir(), ".synth-desktop", "laguna")
	);
}

function modelsDir(): string {
	const configured = process.env.SYNTH_LAGUNA_MODELS_DIR;
	if (configured) return configured;
	const poolside = join(homedir(), ".config", "poolside", "models");
	if (existsSync(join(poolside, "poolside", "Laguna-XS-2.1-NVFP4-mlx"))) {
		return poolside;
	}
	return join(homedir(), ".synth-desktop", "models");
}

function setStatus(patch: Partial<LagunaStatus>): LagunaStatus {
	currentStatus = {
		...currentStatus,
		...patch,
		updatedAt: Date.now()
	};
	for (const listener of listeners) listener(currentStatus);
	return currentStatus;
}

export function getLagunaStatus(): LagunaStatus {
	return currentStatus;
}

export function onLagunaStatus(listener: (status: LagunaStatus) => void): () => void {
	listeners.add(listener);
	listener(currentStatus);
	return () => listeners.delete(listener);
}

function ensureApiKey(): string {
	if (process.env.SYNTH_LAGUNA_API_KEY?.trim()) {
		return process.env.SYNTH_LAGUNA_API_KEY.trim();
	}
	const home = lagunaHome();
	mkdirSync(home, { recursive: true });
	const keyPath = join(home, "api_key");
	try {
		const existing = readFileSync(keyPath, "utf8").trim();
		if (existing) {
			process.env.SYNTH_LAGUNA_API_KEY = existing;
			return existing;
		}
	} catch {
		/* create */
	}
	const key = `synth-local-${randomBytes(24).toString("hex")}`;
	writeFileSync(keyPath, `${key}\n`, { encoding: "utf8", mode: 0o600 });
	try {
		chmodSync(keyPath, 0o600);
	} catch {
		/* best effort */
	}
	process.env.SYNTH_LAGUNA_API_KEY = key;
	return key;
}

function writeEnvSh(apiKey: string, baseUrl: string): void {
	const home = lagunaHome();
	mkdirSync(home, { recursive: true });
	const body = [
		`export SYNTH_LAGUNA_HOST="127.0.0.1"`,
		`export SYNTH_LAGUNA_BASE_URL="${baseUrl}"`,
		`export SYNTH_LAGUNA_API_KEY="${apiKey}"`,
		`export SYNTH_LAGUNA_BACKEND="${process.env.SYNTH_LAGUNA_BACKEND || "auto"}"`,
		`export SYNTH_LAGUNA_DEFAULT_MODEL="${DEFAULT_MODEL}"`,
		`export SYNTH_LAGUNA_MODELS_DIR="${modelsDir()}"`,
		`export SYNTH_LAGUNA_AUTO_LOAD="1"`,
		process.env.SYNTH_LAGUNA_EXTERNAL_URL
			? `export SYNTH_LAGUNA_EXTERNAL_URL="${process.env.SYNTH_LAGUNA_EXTERNAL_URL}"`
			: "",
		process.env.SYNTH_LAGUNA_UPSTREAM_API_KEY
			? `export SYNTH_LAGUNA_UPSTREAM_API_KEY="${process.env.SYNTH_LAGUNA_UPSTREAM_API_KEY}"`
			: "",
		`export PATH="$HOME/.synth-desktop/laguna/.venv/bin:$PATH"`
	]
		.filter(Boolean)
		.join("\n");
	writeFileSync(join(home, "env.sh"), `${body}\n`, "utf8");
}

async function fetchJson(
	url: string,
	options: { apiKey?: string | null; timeoutMs?: number } = {}
): Promise<{ ok: boolean; status: number; body: Record<string, unknown> | null }> {
	try {
		const response = await fetch(url, {
			headers: options.apiKey
				? { Authorization: `Bearer ${options.apiKey}` }
				: undefined,
			signal: AbortSignal.timeout(options.timeoutMs ?? 1_200)
		});
		const text = await response.text();
		let body: Record<string, unknown> | null = null;
		if (text) {
			try {
				body = JSON.parse(text) as Record<string, unknown>;
			} catch {
				body = { raw: text };
			}
		}
		return { ok: response.ok, status: response.status, body };
	} catch {
		return { ok: false, status: 0, body: null };
	}
}

function healthToPhase(body: Record<string, unknown> | null): LagunaPhase {
	if (!body) return "unavailable";
	const status = String(body.status || "");
	if (status === "ok" || status === "ready") return "ready";
	if (status === "loading") return "loading";
	if (status === "error" || status === "unloaded") return "error";
	return "starting";
}

async function probeSynthSidecar(
	baseUrl: string,
	apiKey: string
): Promise<LagunaStatus | null> {
	const result = await fetchJson(`${baseUrl}/health`, { apiKey });
	if (!result.ok || !result.body) return null;
	const phase = healthToPhase(result.body);
	return {
		phase,
		baseUrl,
		backend: typeof result.body.backend === "string" ? result.body.backend : null,
		loadedModel:
			typeof result.body.loadedModel === "string" ? result.body.loadedModel : null,
		detail: phase === "ready" ? "Laguna XS ready" : `sidecar ${phase}`,
		memoryBytes:
			typeof result.body.memoryBytes === "number" ? result.body.memoryBytes : null,
		updatedAt: Date.now()
	};
}

function readSavedUpstreamKey(): string | null {
	try {
		const raw = readFileSync(
			join(lagunaHome(), "poolside_sidecar_api_key"),
			"utf8"
		).trim();
		return raw || null;
	} catch {
		return null;
	}
}

function saveUpstreamKey(key: string): void {
	const home = lagunaHome();
	mkdirSync(home, { recursive: true });
	writeFileSync(join(home, "poolside_sidecar_api_key"), `${key}\n`, {
		encoding: "utf8",
		mode: 0o600
	});
}

function discoverPoolsideKeyFromProcess(): Promise<string | null> {
	return new Promise((resolve) => {
		execFile(
			"ps",
			["-axo", "command="],
			{ maxBuffer: 8 * 1024 * 1024 },
			(error, stdout) => {
				if (error) {
					resolve(null);
					return;
				}
				for (const line of stdout.split("\n")) {
					if (!line.includes("poolside-mlx-sidecar") && !line.includes("poolside-mlx")) {
						continue;
					}
					const parts = line.trim().split(/\s+/);
					for (let i = 0; i < parts.length; i += 1) {
						if (parts[i] === "--api-key" && parts[i + 1]) {
							resolve(parts[i + 1]);
							return;
						}
						if (parts[i].startsWith("--api-key=")) {
							resolve(parts[i].slice("--api-key=".length));
							return;
						}
					}
				}
				resolve(null);
			}
		);
	});
}

async function discoverPoolsideUpstream(): Promise<{
	url: string;
	apiKey: string;
} | null> {
	const candidates = [
		process.env.SYNTH_LAGUNA_EXTERNAL_URL,
		...POOLSIDE_PORTS.map((port) => `http://127.0.0.1:${port}`)
	].filter((value): value is string => Boolean(value));

	let apiKey =
		process.env.SYNTH_LAGUNA_UPSTREAM_API_KEY ||
		process.env.SYNTH_LAGUNA_EXTERNAL_API_KEY ||
		readSavedUpstreamKey() ||
		(await discoverPoolsideKeyFromProcess());

	if (apiKey) {
		saveUpstreamKey(apiKey);
		process.env.SYNTH_LAGUNA_UPSTREAM_API_KEY = apiKey;
	}

	for (const raw of candidates) {
		const url = raw.replace(/\/$/, "");
		if (!apiKey) {
			const unauthorized = await fetchJson(`${url}/health`);
			if (unauthorized.status === 401) {
				apiKey = await discoverPoolsideKeyFromProcess();
				if (apiKey) {
					saveUpstreamKey(apiKey);
					process.env.SYNTH_LAGUNA_UPSTREAM_API_KEY = apiKey;
				}
			}
		}
		if (!apiKey) continue;
		const result = await fetchJson(`${url}/health`, { apiKey });
		if (result.ok && result.body) {
			return { url, apiKey };
		}
	}
	return null;
}

function resolvePython(): string {
	const venv = join(lagunaHome(), ".venv", "bin", "python");
	if (existsSync(venv)) return venv;
	return process.env.SYNTH_PYTHON || "python3";
}

function spawnSidecar(options: {
	workshopRoot: string;
	apiKey: string;
	baseUrl: string;
	backend: "external" | "mlx_lm" | "mock" | "auto";
	externalUrl?: string | null;
	upstreamApiKey?: string | null;
}): void {
	const python = resolvePython();
	const daemonRoot = join(options.workshopRoot, "services", "laguna-daemon");
	const logPath = join(lagunaHome(), "desktop-sidecar.log");
	mkdirSync(lagunaHome(), { recursive: true });
	const logFd = openSync(logPath, "a");

	const env: NodeJS.ProcessEnv = {
		...process.env,
		PYTHONPATH: process.env.PYTHONPATH
			? `${daemonRoot}:${process.env.PYTHONPATH}`
			: daemonRoot,
		SYNTH_LAGUNA_HOST: "127.0.0.1",
		SYNTH_LAGUNA_PORT: String(DEFAULT_PORT),
		SYNTH_LAGUNA_API_KEY: options.apiKey,
		SYNTH_LAGUNA_BACKEND: options.backend,
		SYNTH_LAGUNA_MODELS_DIR: modelsDir(),
		SYNTH_LAGUNA_DEFAULT_MODEL: DEFAULT_MODEL,
		SYNTH_LAGUNA_AUTO_LOAD: "1",
		SYNTH_LAGUNA_REQUIRE_AUTH: "1",
		SYNTH_LAGUNA_DATA_DIR: lagunaHome()
	};
	if (options.externalUrl) {
		env.SYNTH_LAGUNA_EXTERNAL_URL = options.externalUrl;
	}
	if (options.upstreamApiKey) {
		env.SYNTH_LAGUNA_UPSTREAM_API_KEY = options.upstreamApiKey;
	}

	const child = spawn(python, ["-m", "laguna_daemon"], {
		cwd: options.workshopRoot,
		detached: true,
		stdio: ["ignore", logFd, logFd],
		env
	});
	child.unref();
	closeSync(logFd);
	sidecarChild = child;
	child.on("exit", (code, signal) => {
		if (sidecarChild === child) {
			sidecarChild = null;
			if (currentStatus.phase !== "ready") {
				setStatus({
					phase: "error",
					detail: `sidecar exited (code=${code ?? "null"} signal=${signal ?? "null"}); see ${logPath}`
				});
			}
		}
	});
}

async function waitForReady(
	baseUrl: string,
	apiKey: string,
	timeoutMs: number
): Promise<LagunaStatus> {
	const deadline = Date.now() + timeoutMs;
	while (Date.now() < deadline) {
		const probed = await probeSynthSidecar(baseUrl, apiKey);
		if (probed) {
			setStatus(probed);
			if (probed.phase === "ready") return probed;
			if (probed.phase === "error") return probed;
		} else {
			setStatus({
				phase: "starting",
				baseUrl,
				detail: "Waiting for Laguna sidecar…"
			});
		}
		await new Promise((resolve) => setTimeout(resolve, 400));
	}
	return setStatus({
		phase: "error",
		baseUrl,
		detail: `Timed out waiting for Laguna at ${baseUrl}`
	});
}

/**
 * Ensure the Synth Laguna sidecar is up. Idempotent.
 * Returns the base URL when usable (ready), or null if unavailable.
 */
export async function ensureLagunaSidecar(workshopRoot: string): Promise<string | null> {
	if (process.env.SYNTH_LAGUNA_AUTO_START === "0") {
		setStatus({
			phase: "unavailable",
			detail: "Auto-start disabled (SYNTH_LAGUNA_AUTO_START=0)"
		});
		return process.env.SYNTH_LAGUNA_BASE_URL?.replace(/\/$/, "") || null;
	}

	const apiKey = ensureApiKey();
	const baseUrl = (
		process.env.SYNTH_LAGUNA_BASE_URL || `http://127.0.0.1:${DEFAULT_PORT}`
	).replace(/\/$/, "");

	setStatus({
		phase: "starting",
		baseUrl,
		detail: "Checking Laguna XS…",
		backend: null,
		loadedModel: null
	});

	const existing = await probeSynthSidecar(baseUrl, apiKey);
	if (existing?.phase === "ready") {
		process.env.SYNTH_LAGUNA_BASE_URL = baseUrl;
		writeEnvSh(apiKey, baseUrl);
		return setStatus({ ...existing, detail: "Laguna XS ready" }).baseUrl;
	}

	const upstream = await discoverPoolsideUpstream();
	const backend: "external" | "mlx_lm" | "auto" = upstream
		? "external"
		: process.platform === "darwin"
			? "mlx_lm"
			: "auto";

	if (upstream) {
		process.env.SYNTH_LAGUNA_EXTERNAL_URL = upstream.url;
		process.env.SYNTH_LAGUNA_UPSTREAM_API_KEY = upstream.apiKey;
		setStatus({
			phase: "loading",
			baseUrl,
			backend: "external",
			detail: "Connecting to local Laguna engine…"
		});
	} else {
		setStatus({
			phase: "loading",
			baseUrl,
			backend,
			detail: "Starting Laguna sidecar…"
		});
	}

	writeEnvSh(apiKey, baseUrl);
	spawnSidecar({
		workshopRoot,
		apiKey,
		baseUrl,
		backend,
		externalUrl: upstream?.url ?? null,
		upstreamApiKey: upstream?.apiKey ?? null
	});

	const ready = await waitForReady(baseUrl, apiKey, 90_000);
	if (ready.phase === "ready") {
		process.env.SYNTH_LAGUNA_BASE_URL = baseUrl;
		process.env.SYNTH_LAGUNA_API_KEY = apiKey;
		return baseUrl;
	}
	return null;
}

export function getLagunaApiKey(): string | null {
	return process.env.SYNTH_LAGUNA_API_KEY || null;
}
