import { invoke } from "@tauri-apps/api/core";
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
		const result = await invoke<string>("index_public_search_request", {
			requestJson: JSON.stringify({ method: request.method, path: request.path,
				headers: request.headers, identity: request.identity, body: request.body })
		});
		const value: unknown = JSON.parse(result);
		if (!value || typeof value !== "object") throw new Error("Invalid relay response");
		const row = value as Record<string, unknown>;
		if (!Number.isInteger(row.status) || !row.headers || typeof row.headers !== "object") throw new Error("Invalid relay response");
		return row as unknown as TransportResponse;
	} catch {
		throw new IndexSearchError({ code: "index_transport_failed", detail: "The native Index connection is unavailable." });
	}
};
