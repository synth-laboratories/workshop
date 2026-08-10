import { useCallback, useEffect, useId, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/**
 * Local inference monitor for the Laguna daemon on 127.0.0.1:7333.
 *
 * The daemon is the only source of truth for residency and activity. Every
 * metric it reports is nullable, and `null` means *genuinely unavailable* —
 * this surface renders "Unavailable" for those and never substitutes a zero or
 * an interpolated guess. `/health` remains the authoritative low-frequency
 * residency probe; this panel consumes the high-frequency activity stream.
 */

export type InferencePhase =
	| "queued"
	| "loading"
	| "compiling"
	| "prefill"
	| "decode"
	| "complete";

export type InferenceGeneration = {
	generationId: string | null;
	phase: InferencePhase | null;
	queuedAt: number | null;
	startedAt: number | null;
	firstTokenAt: number | null;
	lastTokenAt: number | null;
	promptTokens: number | null;
	cachedTokens: number | null;
	outputTokens: number | null;
	cacheHitRatio: number | null;
	prefillTokensPerSecond: number | null;
	decodeTokensPerSecond: number | null;
	elapsedMs: number | null;
};

export type InferenceRolling = {
	requestsCompleted: number | null;
	requestsFailed: number | null;
	requestsCancelled: number | null;
	inputTokens: number | null;
	outputTokens: number | null;
	cachedTokens: number | null;
	ttftP50Ms: number | null;
	ttftP95Ms: number | null;
	decodeTpsP50: number | null;
	decodeTpsP95: number | null;
	latencyP50Ms: number | null;
	latencyP95Ms: number | null;
};

export type InferenceSnapshot = {
	model: string | null;
	resident: boolean;
	residentBytes: number | null;
	queueDepth: number | null;
	queueCapacity: number | null;
	/** `null` while the daemon is idle. */
	active: InferenceGeneration | null;
	rolling: InferenceRolling;
};

export type InferenceUnloadOutcome = {
	released: boolean;
	conflict: boolean;
	detail: string | null;
};

/** Everything the panel needs from the host process, injectable for tests. */
export type InferenceTransport = {
	snapshot(): Promise<InferenceSnapshot>;
	/** Starts the stream and returns its teardown. Must be safe to call twice. */
	subscribe(
		onSnapshot: (snapshot: InferenceSnapshot) => void,
		onError: (message: string) => void
	): () => void;
	unload(): Promise<InferenceUnloadOutcome>;
};

export type RecentRequestStatus = "ok" | "failed" | "cancelled";

export type RecentRequest = {
	id: string;
	status: RecentRequestStatus;
	phase: InferencePhase | null;
	model: string | null;
	promptTokens: number | null;
	outputTokens: number | null;
	cachedTokens: number | null;
	cacheHitRatio: number | null;
	ttftMs: number | null;
	decodeTps: number | null;
};

export type InferenceFeedState = "loading" | "ready" | "error" | "off";

export type InferenceFeed = {
	state: InferenceFeedState;
	snapshot: InferenceSnapshot | null;
	error: string | null;
	/** Bounded decode-throughput samples; `null` where the metric was absent. */
	throughput: (number | null)[];
	/** Bounded queue-depth samples. */
	queue: (number | null)[];
	recent: RecentRequest[];
};

export type UnloadState = "idle" | "pending" | "released" | "blocked" | "error";

export type InferenceMonitor = InferenceFeed & {
	unloadState: UnloadState;
	unloadDetail: string | null;
	unload: () => void;
	retry: () => void;
};

export const HISTORY_LIMIT = 60;
export const RECENT_LIMIT = 6;

export const emptyFeed: InferenceFeed = {
	state: "loading",
	snapshot: null,
	error: null,
	throughput: [],
	queue: [],
	recent: []
};

/* ------------------------------------------------------------------ format */

const UNAVAILABLE = "Unavailable";

function isNumber(value: number | null | undefined): value is number {
	return typeof value === "number" && Number.isFinite(value);
}

export function compactModelName(model: string | null): string {
	if (!model) return "Local model";
	const leaf = model.split("/").at(-1) ?? model;
	return leaf
		.replace(/-mlx$/i, "")
		.replace(/-(nvfp4|fp8|int4|q4|4bit|8bit)$/i, "")
		.replace(/-/g, " ")
		.trim();
}

export function formatBytes(bytes: number | null): string {
	if (!isNumber(bytes) || bytes <= 0) return UNAVAILABLE;
	const gigabytes = bytes / 1024 ** 3;
	if (gigabytes >= 1) return `${gigabytes.toFixed(1)} GB`;
	return `${(bytes / 1024 ** 2).toFixed(0)} MB`;
}

export function formatCount(value: number | null): string {
	if (!isNumber(value)) return UNAVAILABLE;
	return Math.round(value).toLocaleString("en-US");
}

export function formatRatio(value: number | null): string {
	if (!isNumber(value)) return UNAVAILABLE;
	return `${Math.round(value * 100)}%`;
}

export function formatTps(value: number | null): string {
	// A tiny client-side timing window once produced a nine-digit "tok/s" value.
	// Local XS telemetry is useful only when it is plausible; absent is clearer
	// and more honest than an eye-catching but false number.
	if (!isNumber(value) || value > 10_000) return UNAVAILABLE;
	return value >= 100 ? value.toFixed(0) : value.toFixed(1);
}

export function formatMs(value: number | null): string {
	if (!isNumber(value)) return UNAVAILABLE;
	if (value < 1000) return `${Math.round(value)} ms`;
	return `${(value / 1000).toFixed(2)} s`;
}

export function formatElapsed(milliseconds: number | null): string {
	if (!isNumber(milliseconds)) return UNAVAILABLE;
	const seconds = milliseconds / 1000;
	if (seconds < 60) return `${seconds.toFixed(1)} s`;
	const minutes = Math.floor(seconds / 60);
	return `${minutes}m ${Math.floor(seconds % 60)}s`;
}

export function formatQueue(depth: number | null, capacity: number | null): string {
	if (!isNumber(depth)) return UNAVAILABLE;
	return isNumber(capacity) ? `${depth}/${capacity}` : `${depth}`;
}

/**
 * Polyline through `values`, restarting the pen at every gap so an unavailable
 * sample reads as a break rather than a fabricated interpolation.
 */
export function sparklinePath(
	values: (number | null)[],
	width = 100,
	height = 24
): string | null {
	const numeric = values.filter(isNumber);
	if (numeric.length < 2) return null;
	const max = Math.max(...numeric);
	const min = Math.min(...numeric, 0);
	const span = max - min || 1;
	const step = values.length > 1 ? width / (values.length - 1) : width;
	let path = "";
	let pen = "M";
	values.forEach((value, index) => {
		if (!isNumber(value)) {
			pen = "M";
			return;
		}
		const x = Number((index * step).toFixed(2));
		const y = Number((height - ((value - min) / span) * height).toFixed(2));
		path += `${pen}${x},${y} `;
		pen = "L";
	});
	return path.trim() || null;
}

/* ------------------------------------------------------------------- feed */

function delta(before: number | null, after: number | null): number {
	if (!isNumber(before) || !isNumber(after)) return 0;
	return after - before;
}

function bounded<T>(history: T[], next: T, limit: number): T[] {
	const appended = [...history, next];
	return appended.length > limit ? appended.slice(appended.length - limit) : appended;
}

function finishRequest(
	generation: InferenceGeneration,
	before: InferenceSnapshot,
	after: InferenceSnapshot
): RecentRequest {
	// The contract exposes no per-request history, so terminal status is derived
	// from the rolling counters that moved while this generation left the slot.
	const failed = delta(before.rolling.requestsFailed, after.rolling.requestsFailed);
	const cancelled = delta(before.rolling.requestsCancelled, after.rolling.requestsCancelled);
	const status: RecentRequestStatus =
		failed > 0 ? "failed" : cancelled > 0 ? "cancelled" : "ok";
	const ttftMs =
		isNumber(generation.firstTokenAt) && isNumber(generation.startedAt)
			? generation.firstTokenAt - generation.startedAt
			: null;
	return {
		id: generation.generationId ?? `generation-${before.rolling.requestsCompleted ?? 0}`,
		status,
		phase: generation.phase,
		model: before.model,
		promptTokens: generation.promptTokens,
		outputTokens: generation.outputTokens,
		cachedTokens: generation.cachedTokens,
		cacheHitRatio: generation.cacheHitRatio,
		ttftMs,
		decodeTps: generation.decodeTokensPerSecond
	};
}

/** Pure feed transition. Exported so the accumulation rules are testable. */
export function reduceFeed(
	feed: InferenceFeed,
	snapshot: InferenceSnapshot,
	historyLimit = HISTORY_LIMIT
): InferenceFeed {
	const previous = feed.snapshot;
	const leaving = previous?.active ?? null;
	const arriving = snapshot.active ?? null;
	const rotated =
		leaving !== null &&
		(arriving === null || arriving.generationId !== leaving.generationId);
	const recent =
		rotated && previous
			? [finishRequest(leaving, previous, snapshot), ...feed.recent].slice(0, RECENT_LIMIT)
			: feed.recent;
	return {
		state: "ready",
		snapshot,
		error: null,
		throughput: bounded(feed.throughput, arriving?.decodeTokensPerSecond ?? null, historyLimit),
		queue: bounded(feed.queue, snapshot.queueDepth ?? null, historyLimit),
		recent
	};
}

export function describeFailure(reason: unknown): string {
	if (typeof reason === "string") return reason;
	if (reason instanceof Error) return reason.message;
	return "Laguna inference telemetry is unavailable.";
}

/**
 * Opens the feed and returns its teardown. Nothing is published after teardown,
 * including an in-flight snapshot that resolves late.
 */
export function attachInferenceFeed(
	transport: InferenceTransport,
	publish: (feed: InferenceFeed) => void,
	historyLimit = HISTORY_LIMIT
): () => void {
	let disposed = false;
	let feed = emptyFeed;
	const emit = (next: InferenceFeed) => {
		if (disposed) return;
		feed = next;
		publish(next);
	};
	const accept = (snapshot: InferenceSnapshot) => emit(reduceFeed(feed, snapshot, historyLimit));
	const fail = (message: string) =>
		emit({ ...feed, state: feed.snapshot ? "ready" : "error", error: message });
	transport
		.snapshot()
		.then(accept)
		.catch((reason: unknown) => fail(describeFailure(reason)));
	const stop = transport.subscribe(accept, fail);
	return () => {
		disposed = true;
		stop();
	};
}

/* -------------------------------------------------------------- transport */

const unavailableTransport: InferenceTransport = {
	snapshot: () => Promise.reject(new Error("Local inference requires the desktop app")),
	subscribe: () => () => undefined,
	unload: () => Promise.reject(new Error("Local inference requires the desktop app"))
};

let tauriTransport: InferenceTransport | null = null;

function isTauri(): boolean {
	return (
		typeof window !== "undefined" &&
		(window.location.protocol === "tauri:" || "__TAURI_INTERNALS__" in window)
	);
}

export function defaultInferenceTransport(): InferenceTransport {
	if (!isTauri()) return unavailableTransport;
	tauriTransport ??= {
		snapshot: () => invoke<InferenceSnapshot>("laguna_inference_snapshot"),
		subscribe(onSnapshot, onError) {
			let disposed = false;
			let unlisten: (() => void) | undefined;
			void listen<InferenceSnapshot>("laguna:inference", ({ payload }) => {
				if (!disposed) onSnapshot(payload);
			}).then((next) => {
				if (disposed) next();
				else unlisten = next;
			});
			void invoke("laguna_inference_stream_start").catch((reason: unknown) => {
				if (!disposed) onError(describeFailure(reason));
			});
			return () => {
				disposed = true;
				unlisten?.();
				void invoke("laguna_inference_stream_stop").catch(() => undefined);
			};
		},
		unload: () => invoke<InferenceUnloadOutcome>("laguna_model_unload")
	};
	return tauriTransport;
}

/* ------------------------------------------------------------------- hook */

export type UseInferenceMonitorOptions = {
	/** The stream is only open while this is true — a hidden pane never polls. */
	visible?: boolean;
	transport?: InferenceTransport;
	historyLimit?: number;
};

export function useInferenceMonitor(options: UseInferenceMonitorOptions = {}): InferenceMonitor {
	const { visible = true, historyLimit = HISTORY_LIMIT } = options;
	const supplied = options.transport;
	const transport = useMemo(() => supplied ?? defaultInferenceTransport(), [supplied]);
	const [attempt, setAttempt] = useState(0);
	const [feed, setFeed] = useState<InferenceFeed>(emptyFeed);
	const [unloadState, setUnloadState] = useState<UnloadState>("idle");
	const [unloadDetail, setUnloadDetail] = useState<string | null>(null);

	useEffect(() => {
		if (!visible) {
			setFeed({ ...emptyFeed, state: "off" });
			return;
		}
		setFeed(emptyFeed);
		return attachInferenceFeed(transport, setFeed, historyLimit);
	}, [attempt, historyLimit, transport, visible]);

	const unload = useCallback(() => {
		setUnloadState("pending");
		setUnloadDetail(null);
		transport
			.unload()
			.then((outcome) => {
				setUnloadState(outcome.released ? "released" : outcome.conflict ? "blocked" : "error");
				setUnloadDetail(outcome.detail);
			})
			.catch((reason: unknown) => {
				setUnloadState("error");
				setUnloadDetail(describeFailure(reason));
			});
	}, [transport]);

	const retry = useCallback(() => setAttempt((value) => value + 1), []);

	return { ...feed, unloadState, unloadDetail, unload, retry };
}

/* --------------------------------------------------------------- rendering */

const PHASE_LABELS: Record<InferencePhase, string> = {
	queued: "queued",
	loading: "loading weights",
	compiling: "compiling",
	prefill: "prefill",
	decode: "decode",
	complete: "complete"
};

const STATUS_LABELS: Record<RecentRequestStatus, string> = {
	ok: "ok",
	failed: "failed",
	cancelled: "cancelled"
};

/** Explicit deep link to Settings → Inference. Absent without a target. */
function InferenceSettingsButton({ onOpen }: { onOpen?: () => void }) {
	if (!onOpen) return null;
	return (
		<button
			type="button"
			className="inference-settings-link"
			data-testid="inference-open-settings"
			onClick={onOpen}
		>
			<svg viewBox="0 0 16 16" width="13" height="13" aria-hidden fill="none" stroke="currentColor" strokeWidth="1.4">
				<circle cx="8" cy="8" r="2.4" />
				<path d="M8 1.6v2M8 12.4v2M1.6 8h2M12.4 8h2M3.5 3.5l1.4 1.4M11.1 11.1l1.4 1.4M12.5 3.5l-1.4 1.4M4.9 11.1l-1.4 1.4" />
			</svg>
			<span>Inference settings</span>
		</button>
	);
}

function Unavailable({ label }: { label: string }) {
	return (
		<span className="inference-unavailable" title={`${label} is not reported by the daemon`}>
			{UNAVAILABLE}
		</span>
	);
}

function Metric({ label, value }: { label: string; value: string }) {
	return value === UNAVAILABLE ? <Unavailable label={label} /> : <>{value}</>;
}

function Sparkline({
	values,
	label,
	caption
}: {
	values: (number | null)[];
	label: string;
	caption: string;
}) {
	const path = sparklinePath(values);
	const latest = [...values].reverse().find(isNumber) ?? null;
	return (
		<figure className="inference-spark" data-testid={`inference-spark-${label}`}>
			<figcaption>
				<span>{caption}</span>
				<strong>{isNumber(latest) ? formatTps(latest) : UNAVAILABLE}</strong>
			</figcaption>
			{path ? (
				<svg
					viewBox="0 0 100 24"
					preserveAspectRatio="none"
					role="img"
					aria-label={`${caption} over the last ${values.length} samples`}
				>
					<path d={path} fill="none" vectorEffect="non-scaling-stroke" />
				</svg>
			) : (
				<p className="inference-spark-empty">Not enough samples yet</p>
			)}
		</figure>
	);
}

export type InferencePanelProps = {
	/** Mount-controlled visibility; the subscription follows it. */
	visible?: boolean;
	/** The surrounding local agent turn is running, even between GPU generations. */
	turnRunning?: boolean;
	/** The local turn is currently waiting for model weights to become resident. */
	warmingUp?: boolean;
	/** Supply to hoist the subscription lifecycle into the parent. */
	monitor?: InferenceMonitor;
	transport?: InferenceTransport;
	historyLimit?: number;
	className?: string;
	/** When set, a labelled button deep-links to Settings → Inference. */
	onOpenSettings?: () => void;
};

export function InferencePanel({
	visible = true,
	turnRunning = false,
	warmingUp = false,
	monitor,
	transport,
	historyLimit,
	className,
	onOpenSettings
}: InferencePanelProps) {
	// The panel is mounted in both the rail and the page, so ids must be local.
	const reasonId = `${useId()}-free-reason`;
	// The hook is always called; it stays inert when the parent owns the feed.
	const internal = useInferenceMonitor({
		visible: visible && !monitor,
		transport,
		historyLimit
	});
	const view = monitor ?? internal;
	// A hidden pane reports "off" from its first paint, before any effect runs.
	const state = !monitor && !visible ? "off" : view.state;
	const snapshot = view.snapshot;
	const active = snapshot?.active ?? null;
	const phase = active?.phase ?? null;
	const rolling = snapshot?.rolling;

	const shell = ["inference-panel", className].filter(Boolean).join(" ");

	if (state === "loading" || state === "off") {
		return (
			<section className={shell} data-testid="inference-panel" data-state={state}>
				<header className="inference-head">
					<h2>Inference</h2>
					<InferenceSettingsButton onOpen={onOpenSettings} />
				</header>
				<p
					className="inference-note"
					role="status"
					data-testid={state === "off" ? "inference-paused" : "inference-loading"}
				>
					{state === "off" ? "Monitor paused" : "Reading local inference telemetry…"}
				</p>
			</section>
		);
	}

	if (state === "error" || !snapshot || !rolling) {
		return (
			<section className={shell} data-testid="inference-panel" data-state="error">
				<header className="inference-head">
					<h2>Inference</h2>
					<InferenceSettingsButton onOpen={onOpenSettings} />
				</header>
				<p className="inference-error" role="alert" data-testid="inference-error">
					{view.error ?? "Laguna inference telemetry is unavailable."}
				</p>
				<button type="button" className="inference-retry" onClick={view.retry}>
					Try again
				</button>
			</section>
		);
	}

	const freeBlocked = Boolean(active) || turnRunning || !snapshot.resident || view.unloadState === "pending";
	const freeReason = active
		? "A generation is running; the model stays resident."
		: turnRunning
			? "The local turn is active; another inference call may follow."
			: !snapshot.resident
				? "No weights are resident."
				: "Release the model weights now.";

	return (
		<section
			className={shell}
			data-testid="inference-panel"
			data-state="ready"
			data-phase={phase ?? (warmingUp ? "loading" : turnRunning ? "turn-active" : snapshot.resident ? "idle" : "unloaded")}
		>
			<header className="inference-head">
				<h2>
					Inference <span aria-hidden>·</span> {compactModelName(snapshot.model)}
				</h2>
				<span
					className="inference-residency"
					data-resident={snapshot.resident ? "yes" : "no"}
					data-testid="inference-residency"
				>
					{snapshot.resident ? (
						<>
							RESIDENT <span aria-hidden>·</span>{" "}
							<Metric label="Resident memory" value={formatBytes(snapshot.residentBytes)} />
						</>
					) : (
						"UNLOADED"
					)}
				</span>
				<InferenceSettingsButton onOpen={onOpenSettings} />
			</header>

			<div className="inference-activity" data-testid="inference-activity" aria-live="polite">
				{active ? (
					<>
						<span className="inference-activity-state">
							{phase === "decode"
								? "GENERATING"
								: phase === "prefill"
									? "PREFILLING"
									: phase === "compiling"
										? "PREPARING"
										: phase === "loading"
											? "WARMING"
											: phase === "complete"
												? "FINISHING"
												: "QUEUED"}
						</span>
						<span className="inference-phase" data-phase={phase ?? "queued"}>
							{phase ? PHASE_LABELS[phase] : "unknown phase"}
						</span>
						<span className="inference-activity-rate">
							<Metric
								label="Decode throughput"
								value={
									isNumber(active.decodeTokensPerSecond)
										? `${formatTps(active.decodeTokensPerSecond)} tok/s`
										: UNAVAILABLE
								}
							/>
						</span>
						<span className="inference-activity-elapsed">
							<Metric label="Elapsed" value={formatElapsed(active.elapsedMs)} />
						</span>
					</>
				) : warmingUp ? (
					<>
						<span className="inference-activity-state">WARMING</span>
						<span className="inference-phase" data-phase="loading">
							loading model weights
						</span>
					</>
				) : turnRunning ? (
					<>
						<span className="inference-activity-state">TURN ACTIVE</span>
						<span className="inference-phase" data-phase="turn-active">
							waiting for next inference call
						</span>
					</>
				) : (
					<>
						<span className="inference-activity-state" data-idle="yes">
							IDLE
						</span>
						<span className="inference-phase" data-phase="idle">
							no generation in flight
						</span>
					</>
				)}
			</div>

			<ul className="inference-chips" data-testid="inference-chips">
				<li>
					<span>in flight</span>
					<strong>
						<Metric
							label="Queue depth"
							value={formatQueue(snapshot.queueDepth, snapshot.queueCapacity)}
						/>
					</strong>
				</li>
				<li>
					<span>prompt</span>
					<strong>
						<Metric label="Prompt tokens" value={formatCount(active?.promptTokens ?? null)} />
					</strong>
				</li>
				<li>
					<span>cached</span>
					<strong>
						<Metric label="Cached tokens" value={formatCount(active?.cachedTokens ?? null)} />
						{isNumber(active?.cacheHitRatio) ? (
							<em> · {formatRatio(active.cacheHitRatio)}</em>
						) : null}
					</strong>
				</li>
				<li>
					<span>output</span>
					<strong>
						<Metric label="Output tokens" value={formatCount(active?.outputTokens ?? null)} />
					</strong>
				</li>
			</ul>

			<dl className="inference-stats" data-testid="inference-stats">
				<div className="inference-stat-row">
					<dt>TTFT</dt>
					<dd>
						<Metric label="TTFT p50" value={formatMs(rolling.ttftP50Ms)} /> <i>p50</i>
					</dd>
					<dd>
						<Metric label="TTFT p95" value={formatMs(rolling.ttftP95Ms)} /> <i>p95</i>
					</dd>
				</div>
				<div className="inference-stat-row">
					<dt>Decode</dt>
					<dd>
						<Metric label="Decode p50" value={formatTps(rolling.decodeTpsP50)} /> <i>p50</i>
					</dd>
					<dd>
						<Metric label="Decode p95" value={formatTps(rolling.decodeTpsP95)} /> <i>p95 tok/s</i>
					</dd>
				</div>
				<div className="inference-stat-row inference-stat-row-requests">
					<dt>Requests</dt>
					<dd>
						<Metric label="Completed requests" value={formatCount(rolling.requestsCompleted)} />{" "}
						<i>ok</i>
					</dd>
					<dd>
						<Metric label="Failed requests" value={formatCount(rolling.requestsFailed)} />{" "}
						<i>failed</i>
					</dd>
					<dd>
						<Metric label="Cancelled requests" value={formatCount(rolling.requestsCancelled)} />{" "}
						<i>cancelled</i>
					</dd>
				</div>
			</dl>

			<div className="inference-sparks">
				<Sparkline values={view.throughput} label="throughput" caption="decode tok/s" />
			<Sparkline values={view.queue} label="queue" caption="in-flight requests" />
			</div>

			<section className="inference-recent" data-testid="inference-recent">
				<h3>Recent requests</h3>
				{view.recent.length === 0 ? (
					<p className="inference-note">No completed generations observed yet</p>
				) : (
					<ul>
						{view.recent.map((request) => (
							<li key={request.id} data-status={request.status}>
								<span className="inference-recent-status" data-status={request.status}>
									{STATUS_LABELS[request.status]}
								</span>
								<span className="inference-recent-model">{compactModelName(request.model)}</span>
								<span>
									<Metric label="Prompt tokens" value={formatCount(request.promptTokens)} />
									<span aria-hidden> → </span>
									<Metric label="Output tokens" value={formatCount(request.outputTokens)} />
								</span>
								<span>
									cache <Metric label="Cache hit ratio" value={formatRatio(request.cacheHitRatio)} />
								</span>
								<span>
									ttft <Metric label="TTFT" value={formatMs(request.ttftMs)} />
								</span>
								<span>
									<Metric label="Decode throughput" value={formatTps(request.decodeTps)} /> tok/s
								</span>
							</li>
						))}
					</ul>
				)}
			</section>

			<footer className="inference-foot">
				<button
					type="button"
					className="inference-free"
					data-testid="inference-free"
					onClick={view.unload}
					disabled={freeBlocked}
					title={freeReason}
					aria-describedby={reasonId}
				>
					{view.unloadState === "pending" ? "Freeing…" : "Free now"}
				</button>
				<span id={reasonId} className="inference-foot-note" aria-live="polite">
					{view.unloadDetail ??
						(view.unloadState === "released" ? "Weights released." : freeReason)}
				</span>
			</footer>
		</section>
	);
}
