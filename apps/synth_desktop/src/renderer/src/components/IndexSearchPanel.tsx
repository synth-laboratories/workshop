import { useEffect, useRef, useState } from "react";
import type { SynthBackendSettings } from "../bridge";
import {
	IndexSearchClient,
	IndexSearchError,
	canSubmit,
	citationMarker,
	fetchTransport,
	indexSearchCopy,
	isLoopbackBackend,
	rateLimitSecondsRemaining,
	viewStateFromError,
	type IndexSearchCapabilities,
	type IndexSearchMode,
	type IndexSearchViewState
} from "../runtime/indexSearch/index.ts";

/**
 * Smallest real Index search surface: query, mode, capabilities-driven copy,
 * results with "Insert citation". The client is injectable so the panel works
 * against the loopback transport today and the native relay later without
 * changing this file.
 */
export function createIndexSearchClientForConnection(connection: SynthBackendSettings | null): IndexSearchClient | null {
	if (!connection || !isLoopbackBackend(connection.backendUrl)) return null;
	return new IndexSearchClient({ transport: fetchTransport(connection.backendUrl, (url, init) => fetch(url, init)), identity: "anonymous" });
}

type Props = {
	client: IndexSearchClient | null;
	onInsertCitation?: (citation: string) => void;
	now?: () => number;
};

const wallClock = () => Date.now();

export function IndexSearchPanel({ client, onInsertCitation, now = wallClock }: Props) {
	const [capabilities, setCapabilities] = useState<IndexSearchCapabilities | null>(null);
	const [capabilitiesState, setCapabilitiesState] = useState<"loading" | "ready" | "failed">("loading");
	const [query, setQuery] = useState("");
	const [mode, setMode] = useState<IndexSearchMode>("fast");
	const [state, setState] = useState<IndexSearchViewState>({ phase: "idle" });
	const [tick, setTick] = useState(0);
	const inflight = useRef<AbortController | null>(null);

	useEffect(() => {
		if (!client) return;
		const controller = new AbortController();
		setCapabilitiesState("loading");
		client
			.capabilities(controller.signal)
			.then((next) => {
				setCapabilities(next);
				setCapabilitiesState("ready");
				if (!next.publicSearch.modes.includes(mode) && next.publicSearch.modes[0]) setMode(next.publicSearch.modes[0]);
			})
			.catch((error: unknown) => {
				if (controller.signal.aborted) return;
				setCapabilitiesState("failed");
				if (error instanceof IndexSearchError) setState(viewStateFromError(error, now()));
			});
		return () => controller.abort();
		// The capability read is tied to the client, not to the mode toggle.
	}, [client]);

	// Drive the 429 countdown once a second while it is visible.
	useEffect(() => {
		if (state.phase !== "rate_limited" || rateLimitSecondsRemaining(state, now()) === 0) return;
		const timer = setInterval(() => setTick((value) => value + 1), 1000);
		return () => clearInterval(timer);
	}, [state, now, tick]);

	useEffect(() => () => inflight.current?.abort(), []);

	const copy = indexSearchCopy(capabilities);
	const nowMs = now();
	const modes = capabilities?.publicSearch.modes ?? [];
	const enabled = capabilities?.publicSearch.enabled === true;
	const submittable = Boolean(client) && enabled && query.trim().length > 0 && canSubmit(state, nowMs);

	const submit = async () => {
		if (!client || !submittable) return;
		inflight.current?.abort();
		const controller = new AbortController();
		inflight.current = controller;
		setState({ phase: "searching", mode, searchId: null, pollAttempt: 0 });
		try {
			const handle = await client.search(
				{ query: query.trim(), mode },
				{
					signal: controller.signal,
					onProgress: (progress) =>
						setState({ phase: "searching", mode, searchId: progress.searchId, pollAttempt: progress.kind === "polling" ? progress.attempt : 0 })
				}
			);
			if (!controller.signal.aborted) setState({ phase: "complete", envelope: handle.envelope });
		} catch (error) {
			if (error instanceof IndexSearchError) setState(viewStateFromError(error, now()));
			else setState({ phase: "failed", code: "index_transport_failed", detail: error instanceof Error ? error.message : String(error) });
		} finally {
			if (inflight.current === controller) inflight.current = null;
		}
	};

	const cancel = () => {
		inflight.current?.abort();
		inflight.current = null;
		setState({ phase: "cancelled" });
	};

	if (!client) {
		return (
			<p className="finetune-meta" data-testid="index-search-unavailable">
				Index search runs through the Synth backend connection. Hosted profiles need the native Index relay; a loopback backend works from this build.
			</p>
		);
	}

	return (
		<div className="backend-settings-grid" data-testid="index-search-panel">
			<label className="backend-settings-wide">
				<span>Query</span>
				<input
					value={query}
					onChange={(event) => setQuery(event.target.value)}
					onKeyDown={(event) => {
						if (event.key === "Enter") void submit();
					}}
					placeholder="Search the public Synth Index"
					spellCheck={false}
					disabled={state.phase === "searching"}
					data-testid="index-search-query"
				/>
			</label>
			<label>
				<span>Mode</span>
				<select value={mode} onChange={(event) => setMode(event.target.value as IndexSearchMode)} disabled={modes.length === 0 || state.phase === "searching"} data-testid="index-search-mode">
					{modes.map((candidate) => (
						<option key={candidate} value={candidate}>
							{copy.modeLabel(candidate)}
							{copy.priceLabel(candidate) ? ` · ${copy.priceLabel(candidate)}` : ""}
						</option>
					))}
				</select>
			</label>
			<div className="backend-config-facts" data-testid="index-search-terms">
				{capabilitiesState === "loading" ? <div><span>Terms</span><code>Reading Index capabilities…</code></div> : null}
				{capabilitiesState === "failed" ? <div><span>Terms</span><code>Index capabilities unavailable; search is disabled until they load.</code></div> : null}
				{copy.priceLabel(mode) ? <div><span>Price</span><code>{copy.priceLabel(mode)}</code></div> : null}
				{copy.limitsLabel(mode) ? <div><span>Limits</span><code>{copy.limitsLabel(mode)}</code></div> : null}
				{copy.retentionLabel() ? <div><span>Retention</span><code>{copy.retentionLabel()}</code></div> : null}
				{copy.privacyCopy() ? <div><span>Privacy</span><code>{copy.privacyCopy()}</code></div> : null}
			</div>
			<div className="backend-settings-actions">
				<span role="status" className="finetune-meta" data-testid="index-search-status">{statusLine(state, nowMs)}</span>
				{state.phase === "searching" ? (
					<button type="button" className="settings-secondary-btn" onClick={cancel} data-testid="index-search-cancel">Cancel</button>
				) : (
					<button type="button" className="settings-secondary-btn" disabled={!submittable} onClick={() => void submit()} data-testid="index-search-submit">Search</button>
				)}
			</div>
			{state.phase === "complete" ? (
				<div className="backend-settings-wide" data-testid="index-search-results">
					<p data-testid="index-search-response" style={{ whiteSpace: "pre-wrap" }}>{state.envelope.response}</p>
					<ol>
						{state.envelope.citations.map((citation) => (
							<li key={`${citation.contributionId}@${citation.revisionId}`}>
								<code>{citationMarker(citation)}</code>
								<span className="finetune-meta"> revision {citation.revisionId}</span>
								{onInsertCitation ? (
									<button type="button" className="settings-secondary-btn" onClick={() => onInsertCitation(citationMarker(citation))}>Insert citation</button>
								) : null}
							</li>
						))}
					</ol>
				</div>
			) : null}
		</div>
	);
}

function statusLine(state: IndexSearchViewState, nowMs: number): string {
	switch (state.phase) {
		case "idle":
			return "";
		case "searching":
			return state.pollAttempt > 0 ? `Deep search running (poll ${state.pollAttempt})…` : state.searchId ? "Deep search accepted…" : "Searching…";
		case "complete":
			return `${state.envelope.status === "partial" ? "Partial answer" : "Answer"} · ${state.envelope.citations.length} citation${state.envelope.citations.length === 1 ? "" : "s"} · monitor ${state.envelope.monitor.releaseId ?? "n/a"}`;
		case "rate_limited": {
			const remaining = rateLimitSecondsRemaining(state, nowMs);
			const scope = state.scope ? ` (${state.scope})` : "";
			if (remaining === null) return `Rate limited${scope}. ${state.detail}`;
			return remaining === 0 ? `Rate limit lifted${scope}. Search again.` : `Rate limited${scope}. Retry in ${remaining}s.`;
		}
		case "unavailable":
			return `Index search is unavailable (${state.code}). ${state.detail}`;
		case "disabled":
			return `Public Index search is disabled on this backend. ${state.detail}`;
		case "too_large":
			return `Query too large. ${state.detail}`;
		case "cancelled":
			return "Search cancelled.";
		case "failed":
			return `Search failed (${state.code}). ${state.detail}`;
	}
}
