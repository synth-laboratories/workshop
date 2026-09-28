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
	type IndexSearchCitation,
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
/**
 * Backend #1660: the delivered body is exactly the Monitor-reviewed public
 * contract, so the Search id, token, expiry and customer charge travel in
 * these (lower-cased) response headers. Body fields remain a fallback for
 * older backends.
 */
export const SEARCH_ID_HEADER = "x-index-search-id";
export const SEARCH_TOKEN_RESPONSE_HEADER = "x-search-token";
export const SEARCH_TOKEN_EXPIRES_HEADER = "x-search-token-expires-at";
export const CUSTOMER_CHARGE_HEADER = "x-index-customer-charge-cents";

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

/**
 * Map a non-success transport response onto a typed error. The backend
 * answers FastAPI-style: `{"detail": {"code", "scope"?}}`, with `Retry-After`
 * as a header on 429. A 422 carries `{"detail": [...]}` (validation errors).
 */
export function errorFromResponse(response: TransportResponse): IndexSearchError {
	const body = response.body && typeof response.body === "object" && !Array.isArray(response.body) ? (response.body as Record<string, unknown>) : {};
	const detailRow = body.detail && typeof body.detail === "object" && !Array.isArray(body.detail) ? (body.detail as Record<string, unknown>) : {};
	const rawCode = optionalString(detailRow.code);
	const detailText = optionalString(body.detail);
	const code: IndexSearchErrorCode = rawCode && KNOWN_ERROR_CODES.has(rawCode) ? (rawCode as IndexSearchErrorCode) : "index_unexpected_status";
	const detail = detailText ?? (rawCode ? `Index search failed with HTTP ${response.status} (${rawCode}).` : `Index search failed with HTTP ${response.status}.`);
	const retryHeader = response.headers["retry-after"];
	const retryAfterS = retryHeader !== undefined && retryHeader.trim() !== "" ? finiteNumber(Number(retryHeader)) : null;
	return new IndexSearchError({
		code,
		detail,
		status: response.status,
		retryAfterS: code === "index_public_rate_limited" ? retryAfterS : null,
		scope: optionalString(detailRow.scope)
	});
}

export function parseCitation(value: unknown): IndexSearchCitation {
	const row = record(value, "citation");
	return {
		contributionId: str(row, "contribution_id", "citation"),
		revisionId: str(row, "revision_id", "citation")
	};
}

/** The inline marker the response text uses for a cited contribution. */
export function citationMarker(citation: IndexSearchCitation): string {
	return `[${citation.contributionId}]`;
}

function releaseIdFrom(headers: Record<string, string>, monitor: Record<string, unknown>): string | null {
	const fromHeader = headers[MONITOR_RELEASE_HEADER]?.trim();
	return fromHeader ? fromHeader : optionalString(monitor.release_id);
}

function parseMode(value: unknown, fallback: IndexSearchMode): IndexSearchMode {
	return value === "fast" || value === "deep" ? value : fallback;
}

/**
 * Parse a Fast 200 / completed Deep body (`PublicSearchDelivery`). Returns the
 * envelope and the token separately. `headers` are the response headers
 * (lower-cased): the Monitor release id is read from `X-Index-Monitor-Release`
 * first, then the body.
 */
export function parseCompleted(
	value: unknown,
	mode: IndexSearchMode,
	headers: Record<string, string> = {}
): { envelope: IndexSearchEnvelope; token: string | null } {
	const row = record(value, "search response");
	const status = row.status;
	if (status !== "completed" && status !== "partial") {
		throw new IndexSearchError({ code: "index_malformed_response", detail: "search response status is not completed or partial." });
	}
	const response = str(row, "response", "search response");
	const citations = row.citations;
	if (!Array.isArray(citations)) {
		throw new IndexSearchError({ code: "index_malformed_response", detail: "search response omitted citations." });
	}
	const monitor = row.monitor && typeof row.monitor === "object" ? (row.monitor as Record<string, unknown>) : {};
	const usage = row.usage && typeof row.usage === "object" ? (row.usage as Record<string, unknown>) : {};
	const chargeHeader = headers[CUSTOMER_CHARGE_HEADER]?.trim();
	const charge = chargeHeader
		? /^\d+$/.test(chargeHeader)
			? Number(chargeHeader)
			: null
		: (finiteNumber(usage.customer_charge_cents) ?? finiteNumber(row.amount_cents));
	if (charge === null) {
		throw new IndexSearchError({ code: "index_malformed_response", detail: "search response omitted the customer charge." });
	}
	const searchIdHeader = headers[SEARCH_ID_HEADER]?.trim();
	return {
		envelope: {
			searchId: searchIdHeader ? searchIdHeader : str(row, "search_id", "search response"),
			mode: parseMode(row.mode, mode),
			status,
			response,
			citations: citations.map(parseCitation),
			monitor: { releaseId: releaseIdFrom(headers, monitor) },
			usage: { customerChargeCents: charge },
			tokenExpiresAt: headers[SEARCH_TOKEN_EXPIRES_HEADER]?.trim() || optionalString(row.search_token_expires_at)
		},
		token: headers[SEARCH_TOKEN_RESPONSE_HEADER]?.trim() || optionalString(row.search_token)
	};
}

type LifecycleState = "queued" | "running" | "completed" | "failed" | "cancelled";

/**
 * A `PublicSearchAccepted` body: the 202 of a Deep start / unfinished poll,
 * and the HTTP 200 of a Deep poll that ended `failed` or `cancelled`.
 */
function lifecycleState(row: Record<string, unknown>): LifecycleState | null {
	const value = row.state ?? row.status;
	return value === "queued" || value === "running" || value === "completed" || value === "failed" || value === "cancelled" ? value : null;
}

function terminalLifecycleError(row: Record<string, unknown>, state: "failed" | "cancelled"): IndexSearchError {
	if (state === "cancelled") return new IndexSearchError({ code: "index_search_cancelled", detail: "Search was cancelled.", status: 200 });
	const failure = row.failure && typeof row.failure === "object" ? (row.failure as Record<string, unknown>) : {};
	const failureCode = optionalString(failure.code);
	return new IndexSearchError({
		code: "index_search_failed",
		detail: failureCode ? `Deep search failed (${failureCode}).` : "Deep search failed.",
		status: 200
	});
}

/** Only a same-backend relative path may carry the search token. */
export function safePollPath(pollUrl: string | null, searchId: string): string {
	const fallback = `${INDEX_PUBLIC_SEARCHES_PATH}/${encodeURIComponent(searchId)}`;
	if (!pollUrl) return fallback;
	if (!pollUrl.startsWith("/") || pollUrl.startsWith("//") || pollUrl.includes("\\")) return fallback;
	return pollUrl;
}

function parseLimits(value: unknown): IndexSearchModeLimits | undefined {
	if (!value || typeof value !== "object") return undefined;
	const row = value as Record<string, unknown>;
	const peerMinute = finiteNumber(row.peer_per_minute);
	const peerDay = finiteNumber(row.peer_per_day);
	const globalMinute = finiteNumber(row.global_per_minute);
	const globalDay = finiteNumber(row.global_per_day);
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
		const pollPath = safePollPath(optionalString(accepted.poll_url), searchId);
		onProgress?.({ kind: "accepted", searchId });
		try {
			for (let attempt = 1; attempt <= DEEP_POLL_MAX_ATTEMPTS; attempt += 1) {
				await this.sleep(DEEP_POLL_DELAYS_MS[Math.min(attempt, DEEP_POLL_DELAYS_MS.length) - 1], signal);
				throwIfAborted(signal);
				onProgress?.({ kind: "polling", searchId, attempt });
				const poll = await this.transport({ method: "GET", path: pollPath, headers: { [SEARCH_TOKEN_HEADER]: token }, identity: this.identity, signal });
				if (poll.status === 202) continue;
				if (poll.status !== 200) throw errorFromResponse(poll);
				const row = record(poll.body, "search poll");
				const state = lifecycleState(row);
				if (state === "failed" || state === "cancelled") throw terminalLifecycleError(row, state);
				if (state === "queued" || state === "running") continue;
				const completed = parseCompleted(row, request.mode, poll.headers);
				return this.handle(completed.envelope, completed.token ?? token);
			}
		} catch (error) {
			if (signal?.aborted || (error instanceof IndexSearchError && error.code === "index_search_cancelled" && error.status === null)) {
				await this.cancelOnBackend(searchId, token);
			}
			throw error;
		}
		throw new IndexSearchError({ code: "index_transport_failed", detail: "Deep search did not finish within the polling budget.", status: 202 });
	}

	/**
	 * Best-effort `POST /searches/{id}/cancel` after the operator cancels a Deep
	 * search. Runs without the aborted signal; a failure here never masks the
	 * cancellation the operator asked for.
	 */
	private async cancelOnBackend(searchId: string, token: string): Promise<void> {
		try {
			await this.transport({
				method: "POST",
				path: `${INDEX_PUBLIC_SEARCHES_PATH}/${encodeURIComponent(searchId)}/cancel`,
				headers: { [SEARCH_TOKEN_HEADER]: token },
				identity: this.identity
			});
		} catch {
			// The search expires on its own; the local cancellation already stands.
		}
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
