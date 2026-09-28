/**
 * Index search adapter: request/response parsing, error mapping, Deep polling
 * and cancellation. The search token lives only inside a closure on the
 * returned handle so it can never reach persisted state, logs or JSON.
 */
import {
	IndexSearchError,
	type IndexSearchCapabilities,
	type IndexSearchEnvelope,
	type IndexSearchErrorCode,
	type IndexSearchIdentity,
	type IndexSearchMode,
	type IndexSearchModeLimits,
	type IndexSearchRequest,
	type IndexSearchResult,
	type IndexSearchTransport,
	type TransportResponse
} from "./types.ts";

export const INDEX_PUBLIC_SEARCH_PATH = "/api/v1/index/public/search";
export const INDEX_PUBLIC_SEARCHES_PATH = "/api/v1/index/public/searches";
/**
 * Capabilities: the anonymous-capable route is `/public/capabilities`; the
 * bare `/capabilities` route answers 401 without a credential. Both carry the
 * same `public_search` block, so `identity` alone picks the path.
 */
export const INDEX_PUBLIC_CAPABILITIES_PATH = "/api/v1/index/public/capabilities";
export const INDEX_ACCOUNT_CAPABILITIES_PATH = "/api/v1/index/capabilities";

export function capabilitiesPathFor(identity: IndexSearchIdentity): string {
	return identity === "account" ? INDEX_ACCOUNT_CAPABILITIES_PATH : INDEX_PUBLIC_CAPABILITIES_PATH;
}
const SEARCH_TOKEN_HEADER = "X-Search-Token";
/**
 * The Monitor's release decision travels as a response header, never in the
 * body (reviewed bytes must equal delivered bytes); the body's
 * `monitor.release_id` is null and `monitor.release_header` names this header.
 * Transport header names are lower-cased.
 */
export const MONITOR_RELEASE_HEADER = "x-index-monitor-release";
export const MONITOR_DELIVERY_HEADER = "x-index-monitor-delivery";

/** Poll cadence for Deep searches; bounded so a stuck search cannot spin. */
export const DEEP_POLL_DELAYS_MS = [500, 1000, 2000, 3000, 5000];
export const DEEP_POLL_MAX_ATTEMPTS = 120;

export type SleepFn = (ms: number, signal?: AbortSignal) => Promise<void>;

export type IndexSearchClientOptions = {
	transport: IndexSearchTransport;
	identity?: IndexSearchIdentity;
	sleep?: SleepFn;
	now?: () => number;
};

/** Completed search plus the in-memory token. `toJSON` omits the token on purpose. */
export type IndexSearchHandle = {
	readonly envelope: IndexSearchEnvelope;
	/** Re-read the finished search with the per-search token. Memory only. */
	refetch(signal?: AbortSignal): Promise<IndexSearchEnvelope>;
	toJSON(): IndexSearchEnvelope;
};

export type SearchProgress = { kind: "accepted"; searchId: string } | { kind: "polling"; searchId: string; attempt: number };

export type SearchOptions = {
	signal?: AbortSignal;
	onProgress?: (progress: SearchProgress) => void;
};

export const defaultSleep: SleepFn = (ms, signal) =>
	new Promise((resolve, reject) => {
		if (signal?.aborted) {
			reject(cancelled());
			return;
		}
		const timer = setTimeout(() => {
			signal?.removeEventListener("abort", onAbort);
			resolve();
		}, ms);
		function onAbort() {
			clearTimeout(timer);
			reject(cancelled());
		}
		signal?.addEventListener("abort", onAbort, { once: true });
	});

function cancelled(): IndexSearchError {
	return new IndexSearchError({ code: "index_search_cancelled", detail: "Search cancelled." });
}

function record(value: unknown, what: string): Record<string, unknown> {
	if (!value || typeof value !== "object" || Array.isArray(value)) {
		throw new IndexSearchError({ code: "index_malformed_response", detail: `${what} is not an object.` });
	}
	return value as Record<string, unknown>;
}

function str(row: Record<string, unknown>, key: string, what: string): string {
	const value = row[key];
	if (typeof value !== "string") {
		throw new IndexSearchError({ code: "index_malformed_response", detail: `${what} omitted ${key}.` });
	}
	return value;
}

function optionalString(value: unknown): string | null {
	return typeof value === "string" ? value : null;
}

function finiteNumber(value: unknown): number | null {
	return typeof value === "number" && Number.isFinite(value) ? value : null;
}

const KNOWN_ERROR_CODES: ReadonlySet<string> = new Set<IndexSearchErrorCode>([
	"index_public_rate_limited",
	"index_public_budget_exhausted",
	"index_rate_store_unavailable",
	"monitor_unavailable",
	"index_request_too_large",
	"index_public_search_disabled",
	"index_search_not_found"
]);

/** Map a non-success transport response onto a typed error. */
export function errorFromResponse(response: TransportResponse): IndexSearchError {
	const body = response.body && typeof response.body === "object" ? (response.body as Record<string, unknown>) : {};
	const rawCode = optionalString(body.code);
	const detail = optionalString(body.detail) ?? `Index search failed with HTTP ${response.status}.`;
	const retryHeader = response.headers["retry-after"];
	const retryAfterS = finiteNumber(body.retry_after_s) ?? (retryHeader ? finiteNumber(Number(retryHeader)) : null);
	const code: IndexSearchErrorCode = rawCode && KNOWN_ERROR_CODES.has(rawCode) ? (rawCode as IndexSearchErrorCode) : "index_unexpected_status";
	return new IndexSearchError({
		code,
		detail,
		status: response.status,
		retryAfterS: code === "index_public_rate_limited" ? retryAfterS : null,
		scope: optionalString(body.scope)
	});
}

export function parseResult(value: unknown): IndexSearchResult {
	const row = record(value, "result");
	return {
		contributionId: str(row, "contribution_id", "result"),
		revisionId: str(row, "revision_id", "result"),
		title: str(row, "title", "result"),
		excerpt: str(row, "excerpt", "result"),
		citation: str(row, "citation", "result")
	};
}

function releaseIdFrom(headers: Record<string, string>, monitor: Record<string, unknown>): string | null {
	const fromHeader = headers[MONITOR_RELEASE_HEADER]?.trim();
	return fromHeader ? fromHeader : optionalString(monitor.release_id);
}

/**
 * Parse a Fast 200 / completed Deep body. Returns the envelope and the token
 * separately. `headers` are the response headers (lower-cased): the Monitor
 * release id is read from `X-Index-Monitor-Release` first, then the body.
 */
export function parseCompleted(
	value: unknown,
	mode: IndexSearchMode,
	headers: Record<string, string> = {}
): { envelope: IndexSearchEnvelope; token: string | null } {
	const row = record(value, "search response");
	const results = row.results;
	if (!Array.isArray(results)) {
		throw new IndexSearchError({ code: "index_malformed_response", detail: "search response omitted results." });
	}
	const monitor = row.monitor && typeof row.monitor === "object" ? (row.monitor as Record<string, unknown>) : {};
	const usage = row.usage && typeof row.usage === "object" ? (row.usage as Record<string, unknown>) : {};
	const charge = finiteNumber(usage.customer_charge_cents);
	if (charge === null) {
		throw new IndexSearchError({ code: "index_malformed_response", detail: "search response omitted usage.customer_charge_cents." });
	}
	return {
		envelope: {
			searchId: str(row, "search_id", "search response"),
			mode,
			results: results.map(parseResult),
			monitor: { releaseId: releaseIdFrom(headers, monitor) },
			usage: { customerChargeCents: charge }
		},
		token: optionalString(row.search_token)
	};
}

function parseLimits(value: unknown): IndexSearchModeLimits | undefined {
	if (!value || typeof value !== "object") return undefined;
	const row = value as Record<string, unknown>;
	const peerMinute = finiteNumber(row.peer_minute);
	const peerDay = finiteNumber(row.peer_day);
	const globalMinute = finiteNumber(row.global_minute);
	const globalDay = finiteNumber(row.global_day);
	if (peerMinute === null || peerDay === null || globalMinute === null || globalDay === null) return undefined;
	return { peerMinute, peerDay, globalMinute, globalDay };
}

export function parseCapabilities(value: unknown): IndexSearchCapabilities {
	const row = record(value, "capabilities");
	const search = record(row.public_search, "capabilities.public_search");
	const modes = Array.isArray(search.modes) ? search.modes.filter((mode): mode is IndexSearchMode => mode === "fast" || mode === "deep") : [];
	const limitsRow = search.limits && typeof search.limits === "object" ? (search.limits as Record<string, unknown>) : {};
	const priceRow = search.price_cents && typeof search.price_cents === "object" ? (search.price_cents as Record<string, unknown>) : {};
	const retention = search.retention && typeof search.retention === "object" ? (search.retention as Record<string, unknown>) : {};
	const limits: Partial<Record<IndexSearchMode, IndexSearchModeLimits>> = {};
	const priceCents: Partial<Record<IndexSearchMode, number>> = {};
	for (const mode of ["fast", "deep"] as const) {
		const parsed = parseLimits(limitsRow[mode]);
		if (parsed) limits[mode] = parsed;
		const price = finiteNumber(priceRow[mode]);
		if (price !== null) priceCents[mode] = price;
	}
	return {
		publicSearch: {
			enabled: search.enabled === true,
			modes,
			limits,
			priceCents,
			retention: {
				publicQueryDays: finiteNumber(retention.public_query_days),
				privateProcessingMinutes: finiteNumber(retention.private_processing_minutes)
			},
			privacyCopy: optionalString(search.privacy_copy)
		}
	};
}

function throwIfAborted(signal?: AbortSignal) {
	if (signal?.aborted) throw cancelled();
}

export class IndexSearchClient {
	readonly identity: IndexSearchIdentity;
	private readonly transport: IndexSearchTransport;
	private readonly sleep: SleepFn;

	constructor(options: IndexSearchClientOptions) {
		this.transport = options.transport;
		this.identity = options.identity ?? "anonymous";
		this.sleep = options.sleep ?? defaultSleep;
	}

	async capabilities(signal?: AbortSignal): Promise<IndexSearchCapabilities> {
		throwIfAborted(signal);
		const response = await this.transport({ method: "GET", path: capabilitiesPathFor(this.identity), headers: {}, identity: this.identity, signal });
		if (response.status !== 200) throw errorFromResponse(response);
		return parseCapabilities(response.body);
	}

	async search(request: IndexSearchRequest, options: SearchOptions = {}): Promise<IndexSearchHandle> {
		const { signal, onProgress } = options;
		throwIfAborted(signal);
		const body: Record<string, unknown> = { mode: request.mode, query: request.query };
		if (request.maxResults !== undefined) body.max_results = request.maxResults;
		if (request.idempotencyKey !== undefined) body.idempotency_key = request.idempotencyKey;
		const response = await this.transport({ method: "POST", path: INDEX_PUBLIC_SEARCH_PATH, headers: {}, body, identity: this.identity, signal });
		if (response.status === 200) {
			const completed = parseCompleted(response.body, request.mode, response.headers);
			return this.handle(completed.envelope, completed.token);
		}
		if (response.status !== 202) throw errorFromResponse(response);
		const accepted = record(response.body, "accepted search");
		const searchId = str(accepted, "search_id", "accepted search");
		const token = str(accepted, "search_token", "accepted search");
		const pollUrl = optionalString(accepted.poll_url) ?? `${INDEX_PUBLIC_SEARCHES_PATH}/${encodeURIComponent(searchId)}`;
		onProgress?.({ kind: "accepted", searchId });
		for (let attempt = 1; attempt <= DEEP_POLL_MAX_ATTEMPTS; attempt += 1) {
			await this.sleep(DEEP_POLL_DELAYS_MS[Math.min(attempt, DEEP_POLL_DELAYS_MS.length) - 1], signal);
			throwIfAborted(signal);
			onProgress?.({ kind: "polling", searchId, attempt });
			const poll = await this.transport({ method: "GET", path: pollUrl, headers: { [SEARCH_TOKEN_HEADER]: token }, identity: this.identity, signal });
			if (poll.status === 202) continue;
			if (poll.status !== 200) throw errorFromResponse(poll);
			const completed = parseCompleted(poll.body, request.mode, poll.headers);
			return this.handle(completed.envelope, completed.token ?? token);
		}
		throw new IndexSearchError({ code: "index_transport_failed", detail: "Deep search did not finish within the polling budget.", status: 202 });
	}

	private handle(envelope: IndexSearchEnvelope, token: string | null): IndexSearchHandle {
		const transport = this.transport;
		const identity = this.identity;
		const mode = envelope.mode;
		// The token is captured here and nowhere else: not a field, not serialized.
		return {
			envelope,
			async refetch(signal?: AbortSignal) {
				throwIfAborted(signal);
				if (!token) {
					throw new IndexSearchError({ code: "index_search_not_found", detail: "This search carries no token to re-read it with.", status: 404 });
				}
				const response = await transport({
					method: "GET",
					path: `${INDEX_PUBLIC_SEARCHES_PATH}/${encodeURIComponent(envelope.searchId)}`,
					headers: { [SEARCH_TOKEN_HEADER]: token },
					identity,
					signal
				});
				if (response.status !== 200) throw errorFromResponse(response);
				return parseCompleted(response.body, mode, response.headers).envelope;
			},
			toJSON() {
				return envelope;
			}
		};
	}
}
