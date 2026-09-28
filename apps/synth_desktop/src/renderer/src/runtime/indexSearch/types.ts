/**
 * Synth Index public search: the one typed contract Workshop speaks.
 *
 * Wire shapes mirror the backend spec (`POST /api/v1/index/public/search`,
 * `GET /api/v1/index/public/searches/{search_id}`,
 * `GET /api/v1/index/public/capabilities` anonymously or
 * `GET /api/v1/index/capabilities` with an account credential).
 * Every other module in this folder consumes these types; nothing outside the
 * folder parses Index JSON.
 */

export type IndexSearchMode = "fast" | "deep";

export type IndexSearchRequest = {
	query: string;
	mode: IndexSearchMode;
	maxResults?: number;
	idempotencyKey?: string;
};

export type IndexSearchResult = {
	contributionId: string;
	revisionId: string;
	title: string;
	excerpt: string;
	citation: string;
};

/** A completed search. Deliberately excludes the search token. */
export type IndexSearchEnvelope = {
	searchId: string;
	mode: IndexSearchMode;
	results: IndexSearchResult[];
	/** Release id from the `X-Index-Monitor-Release` response header, else the body's `monitor.release_id`. */
	monitor: { releaseId: string | null };
	usage: { customerChargeCents: number };
};

export type IndexSearchModeLimits = {
	peerMinute: number;
	peerDay: number;
	globalMinute: number;
	globalDay: number;
};

export type IndexSearchCapabilities = {
	publicSearch: {
		enabled: boolean;
		modes: IndexSearchMode[];
		limits: Partial<Record<IndexSearchMode, IndexSearchModeLimits>>;
		priceCents: Partial<Record<IndexSearchMode, number>>;
		retention: { publicQueryDays: number | null; privateProcessingMinutes: number | null };
		privacyCopy: string | null;
	};
};

/** Backend error codes from the contract plus the local codes the adapter adds. */
export type IndexSearchErrorCode =
	| "index_public_rate_limited"
	| "index_public_budget_exhausted"
	| "index_rate_store_unavailable"
	| "monitor_unavailable"
	| "index_request_too_large"
	| "index_public_search_disabled"
	| "index_search_not_found"
	| "index_identity_unavailable"
	| "index_transport_failed"
	| "index_malformed_response"
	| "index_search_cancelled"
	| "index_unexpected_status";

export class IndexSearchError extends Error {
	readonly code: IndexSearchErrorCode;
	readonly detail: string;
	readonly status: number | null;
	readonly retryAfterS: number | null;
	readonly scope: string | null;

	constructor(input: {
		code: IndexSearchErrorCode;
		detail: string;
		status?: number | null;
		retryAfterS?: number | null;
		scope?: string | null;
	}) {
		super(`${input.code}: ${input.detail}`);
		this.name = "IndexSearchError";
		this.code = input.code;
		this.detail = input.detail;
		this.status = input.status ?? null;
		this.retryAfterS = input.retryAfterS ?? null;
		this.scope = input.scope ?? null;
	}
}

/**
 * Who the search runs as. The renderer never holds an API key: `account`
 * means "let the native host attach the operator's key". A transport that
 * cannot honor that must fail closed with `index_identity_unavailable`.
 */
export type IndexSearchIdentity = "anonymous" | "account";

export type TransportRequest = {
	method: "GET" | "POST";
	path: string;
	headers: Record<string, string>;
	body?: unknown;
	identity: IndexSearchIdentity;
	signal?: AbortSignal;
};

export type TransportResponse = {
	status: number;
	/** Header names lower-cased. */
	headers: Record<string, string>;
	/** Parsed JSON body, or null when the body was empty or not JSON. */
	body: unknown;
};

export type IndexSearchTransport = (request: TransportRequest) => Promise<TransportResponse>;
