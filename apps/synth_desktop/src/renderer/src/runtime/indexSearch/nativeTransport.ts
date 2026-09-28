import { commands } from "../../generated/protocol.ts";
import { IndexSearchError, type IndexSearchTransport, type TransportResponse } from "./types.ts";

/** Native relay edge; anonymous-only and no credentials in renderer state. */
export const nativeIndexTransport: IndexSearchTransport = async (request) => {
	if (request.identity !== "anonymous") {
		throw new IndexSearchError({ code: "index_identity_unavailable", detail: "Select anonymous public Search for the free Index route." });
	}
	if (request.signal?.aborted) {
		throw new IndexSearchError({ code: "index_search_cancelled", detail: "Search cancelled." });
	}
	try {
		// The signal stays local. Waiting for an accepted response lets the client
		// cancel its Search by ID even if the operator aborts during creation.
		const result = await commands.indexPublicSearchRequest(JSON.stringify({
			method: request.method, path: request.path, headers: request.headers,
			identity: request.identity, body: request.body
		}));
		if (result.status !== "ok") throw new Error("Relay unavailable");
		const value: unknown = JSON.parse(result.data);
		if (!value || typeof value !== "object") throw new Error("Invalid relay response");
		const row = value as Record<string, unknown>;
		if (!Number.isInteger(row.status) || Number(row.status) < 100 || Number(row.status) > 599 || !row.headers || typeof row.headers !== "object" || Array.isArray(row.headers) || Object.values(row.headers).some(value => typeof value !== "string")) throw new Error("Invalid relay response");
		return row as unknown as TransportResponse;
	} catch {
		throw new IndexSearchError({ code: "index_transport_failed", detail: "The native Index connection is unavailable." });
	}
};
