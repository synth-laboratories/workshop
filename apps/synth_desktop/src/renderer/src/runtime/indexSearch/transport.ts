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
		const url = request.path.startsWith("http://") || request.path.startsWith("https://") ? request.path : `${base}${request.path}`;
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

/** True when a backend URL is reachable from the renderer under the packaged CSP. */
export function isLoopbackBackend(backendUrl: string): boolean {
	try {
		const url = new URL(backendUrl);
		return url.hostname === "127.0.0.1" || url.hostname === "localhost";
	} catch {
		return false;
	}
}
