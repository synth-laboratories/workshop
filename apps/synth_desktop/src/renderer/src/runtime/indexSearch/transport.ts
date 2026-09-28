import { IndexSearchError, type IndexSearchTransport, type TransportRequest, type TransportResponse } from "./types.ts";

type FetchLike = (input: string, init: { method: string; headers: Record<string, string>; body?: string; signal?: AbortSignal }) => Promise<{
	status: number;
	headers: { forEach(callback: (value: string, key: string) => void): void };
	text(): Promise<string>;
}>;

/**
 * Direct HTTP transport. The packaged renderer's CSP only allows loopback
 * `connect-src`, so this transport is for local backends and tests; hosted
 * profiles go through the native relay (see INDEX_SEARCH_BRIDGE.md). It never
 * holds a credential, so `account` identity fails closed here.
 */
export function fetchTransport(baseUrl: string, fetchImpl: FetchLike): IndexSearchTransport {
	const base = baseUrl.replace(/\/+$/, "");
	return async (request: TransportRequest): Promise<TransportResponse> => {
		if (request.identity !== "anonymous") {
			throw new IndexSearchError({
				code: "index_identity_unavailable",
				detail: "The renderer transport cannot attach an account credential."
			});
		}
		const url = resolveSameOrigin(base, request.path);
		let response: Awaited<ReturnType<FetchLike>>;
		try {
			response = await fetchImpl(url, {
				method: request.method,
				headers: request.body === undefined ? request.headers : { "content-type": "application/json", ...request.headers },
				body: request.body === undefined ? undefined : JSON.stringify(request.body),
				signal: request.signal
			});
		} catch (error) {
			if (request.signal?.aborted) {
				throw new IndexSearchError({ code: "index_search_cancelled", detail: "Search cancelled." });
			}
			throw new IndexSearchError({ code: "index_transport_failed", detail: error instanceof Error ? error.message : String(error) });
		}
		const headers: Record<string, string> = {};
		response.headers.forEach((value, key) => {
			headers[key.toLowerCase()] = value;
		});
		const text = await response.text();
		let body: unknown = null;
		if (text.trim()) {
			try {
				body = JSON.parse(text);
			} catch {
				body = null;
			}
		}
		return { status: response.status, headers, body };
	};
}

/**
 * Resolve a request path against the backend. Relative paths join the base;
 * an absolute URL is only followed when it has the backend's exact origin, so
 * a `poll_url` can never carry the search token to another host.
 */
export function resolveSameOrigin(base: string, path: string): string {
	if (path.startsWith("/") && !path.startsWith("//")) return `${base}${path}`;
	let target: URL;
	let origin: string;
	try {
		target = new URL(path);
		origin = new URL(base).origin;
	} catch {
		throw new IndexSearchError({ code: "index_transport_failed", detail: "Refusing a request path that is neither relative nor a URL." });
	}
	if (target.origin !== origin) {
		throw new IndexSearchError({ code: "index_transport_failed", detail: "Refusing to send an Index request to a different origin." });
	}
	return target.toString();
}

const LOOPBACK_HOSTS: ReadonlySet<string> = new Set(["127.0.0.1", "localhost", "[::1]", "::1"]);

/** True when a backend URL is reachable from the renderer under the packaged CSP. */
export function isLoopbackBackend(backendUrl: string): boolean {
	try {
		const url = new URL(backendUrl);
		return LOOPBACK_HOSTS.has(url.hostname);
	} catch {
		return false;
	}
}
