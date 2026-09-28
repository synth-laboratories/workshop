import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
	DEEP_POLL_MAX_ATTEMPTS,
	INDEX_CAPABILITIES_PATH,
	INDEX_PUBLIC_SEARCH_PATH,
	IndexSearchClient,
	errorFromResponse,
	parseCapabilities
} from "./client.ts";
import { indexSearchCopy } from "./copy.ts";
import { fetchTransport, isLoopbackBackend } from "./transport.ts";
import { IndexSearchError, type IndexSearchTransport, type TransportRequest, type TransportResponse } from "./types.ts";
import { canSubmit, rateLimitSecondsRemaining, viewStateFromError } from "./viewState.ts";

const TOKEN = "tok-search-secret-ABCDEFGHIJKLMNOP";

const COMPLETED_BODY = {
	search_id: "srch_1",
	search_token: TOKEN,
	results: [
		{ contribution_id: "c1", revision_id: "r1", title: "T", excerpt: "E", citation: "[synth-index:c1@r1]" }
	],
	monitor: { release_id: "rel_9" },
	usage: { customer_charge_cents: 0 }
};

const CAPABILITIES_BODY = {
	public_search: {
		enabled: true,
		modes: ["fast", "deep"],
		limits: {
			fast: { peer_minute: 12, peer_day: 340, global_minute: 5000, global_day: 90000 },
			deep: { peer_minute: 3, peer_day: 41, global_minute: 200, global_day: 4000 }
		},
		price_cents: { fast: 0, deep: 250 },
		retention: { public_query_days: 30, private_processing_minutes: 60 },
		privacy_copy: "Queries are retained for abuse review only."
	}
};

type Script = (request: TransportRequest, callIndex: number) => TransportResponse | Promise<TransportResponse>;

function scripted(script: Script): { transport: IndexSearchTransport; calls: TransportRequest[] } {
	const calls: TransportRequest[] = [];
	const transport: IndexSearchTransport = async (request) => {
		calls.push(request);
		return script(request, calls.length - 1);
	};
	return { transport, calls };
}

const noSleep = async () => {};

function json(status: number, body: unknown, headers: Record<string, string> = {}): TransportResponse {
	return { status, headers, body };
}

test("fast search returns a typed envelope and keeps the token out of the handle", async () => {
	const { transport, calls } = scripted(() => json(200, COMPLETED_BODY));
	const client = new IndexSearchClient({ transport, sleep: noSleep });
	const handle = await client.search({ query: "craftax", mode: "fast", maxResults: 5 });
	assert.equal(calls[0].method, "POST");
	assert.equal(calls[0].path, INDEX_PUBLIC_SEARCH_PATH);
	assert.deepEqual(calls[0].body, { mode: "fast", query: "craftax", max_results: 5 });
	assert.equal(calls[0].identity, "anonymous");
	assert.equal(handle.envelope.searchId, "srch_1");
	assert.equal(handle.envelope.results[0].citation, "[synth-index:c1@r1]");
	assert.equal(handle.envelope.monitor.releaseId, "rel_9");
	assert.equal(handle.envelope.usage.customerChargeCents, 0);
	const serialized = JSON.stringify(handle);
	assert.ok(!serialized.includes(TOKEN), "token must not serialize");
	assert.ok(!JSON.stringify(handle.envelope).includes(TOKEN));
	assert.ok(!Object.keys(handle).some((key) => /token/i.test(key)));
});

test("refetch sends the per-search token as a header only", async () => {
	const { transport, calls } = scripted((_request, index) => (index === 0 ? json(200, COMPLETED_BODY) : json(200, COMPLETED_BODY)));
	const client = new IndexSearchClient({ transport, sleep: noSleep });
	const handle = await client.search({ query: "q", mode: "fast" });
	await handle.refetch();
	assert.equal(calls[1].method, "GET");
	assert.equal(calls[1].path, "/api/v1/index/public/searches/srch_1");
	assert.equal(calls[1].headers["X-Search-Token"], TOKEN);
	assert.equal(calls[1].body, undefined);
});

test("deep search polls with the token until 200 and reports progress", async () => {
	const { transport, calls } = scripted((_request, index) => {
		if (index === 0) return json(202, { search_id: "srch_d", search_token: TOKEN, poll_url: "/api/v1/index/public/searches/srch_d" });
		if (index < 3) return json(202, { search_id: "srch_d", state: "running" });
		return json(200, { ...COMPLETED_BODY, search_id: "srch_d" });
	});
	const slept: number[] = [];
	const client = new IndexSearchClient({ transport, sleep: async (ms) => void slept.push(ms) });
	const progress: string[] = [];
	const handle = await client.search({ query: "q", mode: "deep" }, { onProgress: (p) => progress.push(p.kind) });
	assert.equal(handle.envelope.searchId, "srch_d");
	assert.equal(handle.envelope.mode, "deep");
	assert.equal(calls.length, 4);
	for (const poll of calls.slice(1)) {
		assert.equal(poll.method, "GET");
		assert.equal(poll.path, "/api/v1/index/public/searches/srch_d");
		assert.equal(poll.headers["X-Search-Token"], TOKEN);
	}
	assert.deepEqual(slept, [500, 1000, 2000]);
	assert.deepEqual(progress, ["accepted", "polling", "polling", "polling"]);
});

test("deep polling gives up after the bounded attempt budget", async () => {
	const { transport, calls } = scripted((_request, index) => (index === 0 ? json(202, { search_id: "s", search_token: TOKEN }) : json(202, {})));
	const client = new IndexSearchClient({ transport, sleep: noSleep });
	await assert.rejects(client.search({ query: "q", mode: "deep" }), (error: unknown) => error instanceof IndexSearchError && error.code === "index_transport_failed");
	assert.equal(calls.length, DEEP_POLL_MAX_ATTEMPTS + 1);
});

test("deep polling with a wrong token surfaces index_search_not_found", async () => {
	const { transport } = scripted((_request, index) =>
		index === 0 ? json(202, { search_id: "s", search_token: TOKEN }) : json(404, { code: "index_search_not_found", detail: "unknown search" })
	);
	const client = new IndexSearchClient({ transport, sleep: noSleep });
	await assert.rejects(client.search({ query: "q", mode: "deep" }), (error: unknown) => error instanceof IndexSearchError && error.code === "index_search_not_found" && error.status === 404);
});

test("cancellation aborts between polls and before the first request", async () => {
	const controller = new AbortController();
	const { transport, calls } = scripted((_request, index) => {
		if (index === 0) return json(202, { search_id: "s", search_token: TOKEN });
		return json(202, {});
	});
	const client = new IndexSearchClient({
		transport,
		sleep: async () => {
			controller.abort();
		}
	});
	await assert.rejects(client.search({ query: "q", mode: "deep" }, { signal: controller.signal }), (error: unknown) => error instanceof IndexSearchError && error.code === "index_search_cancelled");
	assert.equal(calls.length, 1, "no poll after abort");
	const aborted = new AbortController();
	aborted.abort();
	await assert.rejects(client.search({ query: "q", mode: "fast" }, { signal: aborted.signal }), (error: unknown) => error instanceof IndexSearchError && error.code === "index_search_cancelled");
	assert.equal(calls.length, 1, "no request after a pre-aborted signal");
});

const ERROR_CASES: Array<{ status: number; code: string; extra?: Record<string, unknown>; headers?: Record<string, string> }> = [
	{ status: 429, code: "index_public_rate_limited", extra: { scope: "peer_minute" }, headers: { "retry-after": "17" } },
	{ status: 503, code: "index_public_budget_exhausted" },
	{ status: 503, code: "index_rate_store_unavailable" },
	{ status: 503, code: "monitor_unavailable" },
	{ status: 413, code: "index_request_too_large" },
	{ status: 404, code: "index_public_search_disabled" },
	{ status: 404, code: "index_search_not_found" }
];

for (const kase of ERROR_CASES) {
	test(`error ${kase.status} ${kase.code} maps to a typed error`, async () => {
		const { transport } = scripted(() => json(kase.status, { code: kase.code, detail: `d:${kase.code}`, ...kase.extra }, kase.headers));
		const client = new IndexSearchClient({ transport, sleep: noSleep });
		await assert.rejects(client.search({ query: "q", mode: "fast" }), (error: unknown) => {
			assert.ok(error instanceof IndexSearchError);
			assert.equal(error.code, kase.code);
			assert.equal(error.status, kase.status);
			assert.equal(error.detail, `d:${kase.code}`);
			if (kase.code === "index_public_rate_limited") {
				assert.equal(error.retryAfterS, 17);
				assert.equal(error.scope, "peer_minute");
			} else {
				assert.equal(error.retryAfterS, null);
			}
			return true;
		});
	});
}

test("unknown codes and non-JSON bodies become index_unexpected_status", () => {
	assert.equal(errorFromResponse(json(500, null)).code, "index_unexpected_status");
	assert.equal(errorFromResponse(json(500, { code: "something_else", detail: "x" })).code, "index_unexpected_status");
	assert.equal(errorFromResponse(json(500, null)).detail, "Index search failed with HTTP 500.");
});

test("malformed success bodies fail closed", async () => {
	const { transport } = scripted(() => json(200, { search_id: "s", results: [{ title: "no ids" }], usage: { customer_charge_cents: 0 } }));
	const client = new IndexSearchClient({ transport, sleep: noSleep });
	await assert.rejects(client.search({ query: "q", mode: "fast" }), (error: unknown) => error instanceof IndexSearchError && error.code === "index_malformed_response");
});

test("view state maps each error code to its UI state and counts the 429 down", () => {
	const now = 1_000_000;
	const limited = viewStateFromError(new IndexSearchError({ code: "index_public_rate_limited", detail: "slow down", retryAfterS: 17, scope: "peer_minute" }), now);
	assert.equal(limited.phase, "rate_limited");
	assert.equal(rateLimitSecondsRemaining(limited, now), 17);
	assert.equal(rateLimitSecondsRemaining(limited, now + 16_500), 1);
	assert.equal(rateLimitSecondsRemaining(limited, now + 17_000), 0);
	assert.equal(canSubmit(limited, now), false);
	assert.equal(canSubmit(limited, now + 17_000), true);
	const noHeader = viewStateFromError(new IndexSearchError({ code: "index_public_rate_limited", detail: "slow down" }), now);
	assert.equal(rateLimitSecondsRemaining(noHeader, now), null);
	for (const code of ["index_public_budget_exhausted", "index_rate_store_unavailable", "monitor_unavailable"] as const) {
		const state = viewStateFromError(new IndexSearchError({ code, detail: "down" }), now);
		assert.equal(state.phase, "unavailable");
		assert.equal(canSubmit(state, now), false, `${code} fails closed`);
	}
	assert.equal(viewStateFromError(new IndexSearchError({ code: "index_public_search_disabled", detail: "off" }), now).phase, "disabled");
	assert.equal(viewStateFromError(new IndexSearchError({ code: "index_request_too_large", detail: "big" }), now).phase, "too_large");
	assert.equal(viewStateFromError(new IndexSearchError({ code: "index_search_cancelled", detail: "c" }), now).phase, "cancelled");
	assert.equal(viewStateFromError(new IndexSearchError({ code: "index_search_not_found", detail: "nf" }), now).phase, "failed");
	assert.ok(!JSON.stringify(limited).includes("token"));
});

test("capabilities parse and drive every price, limit, retention and privacy sentence", async () => {
	const { transport, calls } = scripted(() => json(200, CAPABILITIES_BODY));
	const client = new IndexSearchClient({ transport, sleep: noSleep });
	const capabilities = await client.capabilities();
	assert.equal(calls[0].path, INDEX_CAPABILITIES_PATH);
	assert.deepEqual(capabilities.publicSearch.modes, ["fast", "deep"]);
	assert.equal(capabilities.publicSearch.limits.deep?.peerDay, 41);
	const copy = indexSearchCopy(capabilities);
	assert.equal(copy.priceLabel("fast"), "0¢ per search");
	assert.equal(copy.priceLabel("deep"), "$2.50 per search");
	assert.equal(copy.limitsLabel("fast"), "12/min · 340/day per device");
	assert.equal(copy.retentionLabel(), "public queries kept 30 days · private processing 60 minutes");
	assert.equal(copy.privacyCopy(), "Queries are retained for abuse review only.");
	const changed = parseCapabilities({ ...CAPABILITIES_BODY, public_search: { ...CAPABILITIES_BODY.public_search, price_cents: { fast: 5, deep: 900 } } });
	assert.equal(indexSearchCopy(changed).priceLabel("fast"), "5¢ per search");
	assert.equal(indexSearchCopy(changed).priceLabel("deep"), "$9 per search");
	const absent = indexSearchCopy(null);
	assert.equal(absent.priceLabel("fast"), null);
	assert.equal(absent.limitsLabel("deep"), null);
	assert.equal(absent.retentionLabel(), null);
	assert.equal(absent.privacyCopy(), null);
});

test("no price, limit or retention literal is hardcoded in the bridge or its panel", () => {
	const here = dirname(fileURLToPath(import.meta.url));
	const files = readdirSync(here)
		.filter((name) => name.endsWith(".ts") && !name.endsWith(".test.ts"))
		.map((name) => join(here, name));
	files.push(join(here, "..", "..", "components", "IndexSearchPanel.tsx"));
	const forbidden = [/\bfree\b/i, /\$\d/, /\b\d+\s*¢/, /\b\d+\s*cents?\b/i, /\b\d+\s*days?\b/i, /per\s+(minute|day)\b.*\d/i];
	for (const file of files) {
		const source = readFileSync(file, "utf8").replace(/\/\*[\s\S]*?\*\/|\/\/.*$/gm, "");
		for (const pattern of forbidden) {
			assert.ok(!pattern.test(source), `${file} hardcodes ${pattern}`);
		}
	}
});

test("fetch transport fails closed for account identity and never reaches the network", async () => {
	let called = 0;
	const transport = fetchTransport("http://127.0.0.1:8000/", async () => {
		called += 1;
		return { status: 200, headers: { forEach() {} }, text: async () => "{}" };
	});
	await assert.rejects(transport({ method: "GET", path: INDEX_CAPABILITIES_PATH, headers: {}, identity: "account" }), (error: unknown) => error instanceof IndexSearchError && error.code === "index_identity_unavailable");
	assert.equal(called, 0);
});

test("fetch transport lowercases headers, parses JSON and joins the base URL", async () => {
	const seen: { url: string; init: { method: string; headers: Record<string, string>; body?: string } | null } = { url: "", init: null };
	const transport = fetchTransport("http://127.0.0.1:8000/", async (url, init) => {
		seen.url = url;
		seen.init = init;
		return {
			status: 429,
			headers: { forEach(cb: (v: string, k: string) => void) { cb("9", "Retry-After"); } },
			text: async () => JSON.stringify({ code: "index_public_rate_limited", detail: "x" })
		};
	});
	const response = await transport({ method: "POST", path: INDEX_PUBLIC_SEARCH_PATH, headers: {}, body: { mode: "fast", query: "q" }, identity: "anonymous" });
	assert.equal(seen.url, `http://127.0.0.1:8000${INDEX_PUBLIC_SEARCH_PATH}`);
	assert.equal(seen.init?.headers["content-type"], "application/json");
	assert.equal(seen.init?.body, JSON.stringify({ mode: "fast", query: "q" }));
	assert.equal(response.headers["retry-after"], "9");
	assert.equal(errorFromResponse(response).retryAfterS, 9);
	assert.equal(isLoopbackBackend("http://127.0.0.1:8000"), true);
	assert.equal(isLoopbackBackend("https://api.usesynth.ai"), false);
	assert.equal(isLoopbackBackend("not a url"), false);
});
