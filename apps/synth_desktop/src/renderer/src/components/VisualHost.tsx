import { Component, useEffect, useMemo, useRef, useState, type ComponentType, type ErrorInfo, type MouseEvent, type ReactNode } from "react";
import type { ArtifactRef } from "../types/landing";
import type { VisualRecord } from "@synth/runtime-protocol";
import {
	bindTemplateSlots,
	bindingInputName,
	consumeInjectedRendererCrash,
	createMediaClient,
	createReplayClient,
	isVisualBindings,
	rememberLastKnownGood,
	propsFromBindings,
	replayStreamsFromBindings,
	resolveTemplate,
	resolveVisualBindings,
	selectRenderedProjection,
	compileSourcedModule,
	isSourcedTemplate,
	sourcedInvalidShell
} from "@synth/visuals";
import { publicError, toPublicError, type PublicError } from "../runtime/publicError";
import type { VisualAnnotation, VisualSeal, VisualSealBundle, VisualUpload } from "../bridge";
import { loadVisualShell } from "../runtime/visualsLoader";
import { bridges } from "../runtime/desktopBridge";
import { subscribeToRun } from "../runtime/runProgress/subscription";
import { useOptimizerRun } from "../hooks/useRunRead";
import {
	subscribeRunCollection,
	subscribeRunCollectionItem
} from "../runtime/runRead/store";
import { createEvidenceClient } from "../runtime/runProgress/evidence";
import { verifyAgainstReceipt, visualDataDigest, type ReceiptVerdict } from "../runtime/runProgress/receipt";
import { progressAgreement, projectRunProgress, splitSnapshotEvents } from "../runtime/runProgress/project";
import { semanticCountsFromRunView } from "../runtime/runProgress/semanticCounts";
import type { ProgressAgreement } from "../runtime/runProgress/project";
import { DIAGNOSTIC_CODES, reportDiagnostic } from "../runtime/diagnostics";
import { MermaidVisual } from "./MermaidVisual";
import { SystemsMapVisual } from "./SystemsMapVisual";
import { ChartVisual } from "./ChartVisual";
import { SystemsDynamicVisual } from "./SystemsDynamicVisual";
import type { SubagentState } from "../runtime/sessionView";
import { bindingAuthorityKey } from "../runtime/visualRevisionState";
import { openTraceReference, VISUAL_REFERENCE_ERROR_EVENT, VISUAL_REFERENCE_OPENED_EVENT } from "../runtime/visualReferences";
import { previewVariantForTemplate, SEALED_TRACE_WORKBENCH_TEMPLATES } from "../runtime/templatePresentation";
import { optimizerRunIdFromBindings } from "../runtime/visualBindings";
import { projectVisualRunLifecycle } from "../runtime/visualRunLifecycle";
import { isTerminalRunStatus } from "../runtime/runProgress/types";
import { runFacets } from "./optimizers/runPresentation";
import { VisualPaneChrome, type VisualPaneDebugState } from "./VisualPaneChrome";

type ShellProps = {
	title?: string;
	lede?: string;
	bindings?: Record<string, unknown>;
	[key: string]: unknown;
};

export function artifactFromVisualRecord(visual: VisualRecord): ArtifactRef {
	const bindings = visual.bindings && typeof visual.bindings === "object"
		? visual.bindings as Record<string, unknown>
		: undefined;
	const metadata = visual.metadata && typeof visual.metadata === "object"
		? visual.metadata as Record<string, unknown>
		: undefined;
	const metadataDisplayName = typeof metadata?.displayName === "string"
		? metadata.displayName.trim()
		: typeof metadata?.display_name === "string"
			? metadata.display_name.trim()
			: "";
	return {
		id: visual.id,
		kind: "report",
		title: visual.title,
		displayName: visual.displayName?.trim() || metadataDisplayName || visual.title,
		updatedAt: visual.updatedAt,
		templateId: visual.templateId,
		visualId: visual.id,
		revision: visual.currentRevision,
		contentDigest: visual.contentDigest ?? undefined,
		rendererKind: visual.rendererKind,
		bindings,
		metadata,
		status: visual.status,
		sessionId: visual.sessionId ?? undefined,
		ownerSessionId: visual.sessionId ?? undefined,
		runId: optimizerRunIdFromBindings(visual.bindings) ?? visual.runId ?? undefined,
		traceId: visual.traceId ?? undefined,
		summary: typeof metadata?.summary === "string" ? metadata.summary : undefined,
		preview: { variant: previewVariantForTemplate(visual.templateId) }
	};
}

function elapsedLabel(value: string, now: number): string {
	const seconds = Math.max(0, Math.floor((now - Date.parse(value)) / 1000));
	if (seconds < 60) return `${seconds}s`;
	if (seconds < 3600) return `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
	return `${Math.floor(seconds / 3600)}h`;
}

function subagentStatusLabel(status: SubagentState["status"]): string {
	return ({
		starting: "Starting",
		working: "Working",
		completed: "Completed",
		interrupted: "Interrupted",
		failed: "Failed",
		stopped: "Stopped",
		unavailable: "Unavailable"
	})[status];
}

function subagentMarker(id: string): string {
	let value = 0;
	for (let index = 0; index < id.length; index += 1) value = (value + id.charCodeAt(index)) % 2;
	return value ? "✣" : "✺";
}

function SubagentsVisual({ artifact }: { artifact: ArtifactRef }) {
	const resolved = propsFromBindings(artifact.bindings);
	const agents = Array.isArray(resolved.props.agents) ? resolved.props.agents as SubagentState[] : [];
	const sessionId = typeof resolved.props.sessionId === "string" ? resolved.props.sessionId : undefined;
	const [now, setNow] = useState(Date.now());
	const [selectedId, setSelectedId] = useState<string | null>(null);
	const [detail, setDetail] = useState<unknown>(null);
	const [detailError, setDetailError] = useState<string | null>(null);
	useEffect(() => {
		const timer = window.setInterval(() => setNow(Date.now()), 1_000);
		return () => window.clearInterval(timer);
	}, []);
	useEffect(() => {
		if (!selectedId || !sessionId || !bridges.codex?.readThread) {
			setDetail(null);
			setDetailError(null);
			return;
		}
		let cancelled = false;
		void bridges.codex.readThread(sessionId, selectedId, true).then(
			(payload) => {
				if (!cancelled) {
					setDetail(payload);
					setDetailError(null);
				}
			},
			(reason) => {
				if (!cancelled) {
					setDetail(null);
					setDetailError(publicError(reason));
				}
			}
		);
		return () => {
			cancelled = true;
		};
	}, [selectedId, sessionId]);
	const groups = [
		{ label: "Working", agents: agents.filter((agent) => agent.status === "starting" || agent.status === "working") },
		{ label: "Needs attention", agents: agents.filter((agent) => agent.status === "interrupted" || agent.status === "failed" || agent.status === "stopped" || agent.status === "unavailable") },
		{ label: "Completed", agents: agents.filter((agent) => agent.status === "completed") }
	];
	const selected = agents.find((agent) => agent.id === selectedId) ?? null;
	const working = groups[0].agents.length;
	const attention = groups[1].agents.length;
	const completed = groups[2].agents.length;
	if (selected) {
		return (
			<div className="subagents-visual" data-testid="visual-subagents">
				<button type="button" className="subagents-back" data-testid="subagents-back" onClick={() => setSelectedId(null)}>
					← {selected.title}
				</button>
				<p className="subagents-workspace-summary" data-testid="subagents-workspace-summary">
					{subagentStatusLabel(selected.status)} · {selected.status === "starting" || selected.status === "working" ? elapsedLabel(selected.startedAt, now) : elapsedLabel(selected.updatedAt, now)}
				</p>
				<div className="subagents-detail" data-testid="subagents-detail">
					{selected.summary ? <p>{selected.summary}</p> : <p>No result yet</p>}
					{detailError ? <p className="subagents-empty">{detailError}</p> : null}
					{detail ? <pre>{JSON.stringify(detail, null, 2)}</pre> : null}
				</div>
			</div>
		);
	}
	return (
		<div className="subagents-visual" data-testid="visual-subagents">
			<p className="subagents-workspace-summary" data-testid="subagents-workspace-summary">
				{working} working · {attention} need attention · {completed} completed
			</p>
			{groups.map((group) => (
				<section key={group.label} className="subagents-group">
					<h3>{group.label} · {group.agents.length}</h3>
					{group.agents.length === 0 ? <p className="subagents-empty">No {group.label.toLowerCase()} subagents</p> : null}
					{group.agents.map((agent) => (
						<button
							type="button"
							className="subagent-row"
							key={agent.id}
							data-status={agent.status}
							data-testid={`subagent-row-${agent.id}`}
							onClick={() => setSelectedId(agent.id)}
						>
							<span className={`subagent-mark mark-${agent.status}`} aria-hidden>{subagentMarker(agent.id)}</span>
							<div className="subagent-copy">
								<div className="subagent-title-row"><strong>{agent.title}</strong><span className={`subagent-state state-${agent.status}`}>{subagentStatusLabel(agent.status)}</span></div>
								{agent.summary ? <p>{agent.summary}</p> : null}
							</div>
							<time dateTime={agent.updatedAt}>{agent.status === "starting" || agent.status === "working" ? elapsedLabel(agent.startedAt, now) : elapsedLabel(agent.updatedAt, now) + " ago"}</time>
						</button>
					))}
				</section>
			))}
		</div>
	);
}

function CraftaxEvalVisual({ artifact }: { artifact: ArtifactRef }) {
	const models = [
		{ name: "Laguna XS", ach: 11.4, cost: 0.12, accent: true },
		{ name: "Luna", ach: 10.1, cost: 0.09 },
		{ name: "Terra", ach: 9.4, cost: 0.15 },
		{ name: "Flash Lite", ach: 7.2, cost: 0.04 },
		{ name: "Kimi K3", ach: 8.8, cost: 0.11 }
	];
	const achievements = [
		["drink", "food", "sapling", "wood", "cow", "zombie"],
		["pickaxe", "sword", "plant", "table", "coal", "stone"],
		["skeleton", "iron", "furnace", "ladder", "bow", "arrow"]
	];
	return (
		<div className="craftax-visual" data-testid="visual-craftax-pareto">
			<div className="craftax-visual-hero">
				<p className="visual-kicker">Open-ended agents · Craftax</p>
				<h2>{artifact.title}</h2>
				{artifact.summary ? <p className="visual-lede">{artifact.summary}</p> : null}
			</div>
			<section className="craftax-section">
				<div className="craftax-section-head">
					<h3>Cost vs performance</h3>
					<span>ACH ↑ · $ / rollout →</span>
				</div>
				<div className="pareto-plot" role="img" aria-label="Pareto chart of achievements vs cost">
					<svg viewBox="0 0 320 200" className="pareto-svg">
						{[40, 80, 120, 160].map((y) => (
							<line key={y} x1="36" y1={y} x2="300" y2={y} stroke="#e8eaee" strokeWidth="1" />
						))}
						{models.map((m, i) => {
							const x = 70 + i * 45;
							const y = 155 - m.ach * 9;
							return <circle key={m.name} cx={x} cy={y} r={m.accent ? 6 : 4} fill={m.accent ? "#f05f22" : "#9aa3b2"} />;
						})}
					</svg>
				</div>
			</section>
			<section className="craftax-section">
				<h3>Achievement matrix</h3>
				<div className="achievement-matrix">
					{achievements.map((row, rowIndex) => (
						<div key={rowIndex} className="achievement-row">
							{row.map((cell) => <span key={cell}>{cell}</span>)}
						</div>
					))}
				</div>
			</section>
		</div>
	);
}

function CraftaxFrameVisual({ artifact }: { artifact: ArtifactRef }) {
	return (
		<div className="craftax-visual" data-testid="visual-craftax-frame">
			<div className="craftax-visual-hero">
				<p className="visual-kicker">Environment frame</p>
				<h2>{artifact.title}</h2>
				{artifact.summary ? <p className="visual-lede">{artifact.summary}</p> : null}
			</div>
			<div className="env-frame" role="img" aria-label="Craftax frame">
				<div className="env-grid">
					{Array.from({ length: 96 }, (_, i) => (
						<span key={i} className={`env-tile t-${(i * 7) % 5}`} />
					))}
				</div>
			</div>
		</div>
	);
}

function MockFallback({ artifact }: { artifact: ArtifactRef }) {
	const variant = artifact.preview?.variant ?? "generic";
	if (variant === "craftax_pareto") return <CraftaxEvalVisual artifact={artifact} />;
	if (variant === "craftax_frame") return <CraftaxFrameVisual artifact={artifact} />;
	return (
		<div className="visual-generic" data-testid="visual-fallback">
			<h2>{artifact.title}</h2>
			{artifact.summary ? <p>{artifact.summary}</p> : null}
			{artifact.templateId ? <p className="visual-template-id">template · {artifact.templateId}</p> : null}
		</div>
	);
}

function decodeBase64Utf8(base64: string): string {
	const bytes = Uint8Array.from(atob(base64), (char) => char.charCodeAt(0));
	return new TextDecoder().decode(bytes);
}

// The runtime is loaded as an external `data:` script in an opaque iframe.
// WebKit correctly treats a sandboxed document as having no `self` origin,
// which blocks a Tauri-asset script even though the asset is app-bundled. A
// data script avoids that origin ambiguity without allowing inline scripts in
// the desktop document. The imported renderer itself still arrives only via
// postMessage after native admission checks.
const MANAGED_HTML_RUNTIME = String.raw`(() => {
  let initialized = false;
  let latestPayload = {};
  const frameChunks = new Map();
  const report = (type, message) => parent.postMessage({ type, message: String(message || "Managed renderer failed") }, "*");
  const deliver = () => window.postMessage({ type: "synth.visual.update.v1", payload: latestPayload }, "*");
  const renderSource = (source) => {
    const parsed = new DOMParser().parseFromString(source, "text/html");
    if (parsed.querySelector("script[src]")) throw new Error("Managed renderer contains an external script");
    document.querySelectorAll("style[data-synth-managed]").forEach((node) => node.remove());
    for (const style of parsed.querySelectorAll("style")) {
      const copy = document.createElement("style");
      copy.dataset.synthManaged = "true";
      copy.textContent = style.textContent;
      document.head.append(copy);
    }
    document.body.replaceChildren(...[...parsed.body.childNodes].filter((node) => node.nodeName !== "SCRIPT"));
    const scripts = [...parsed.querySelectorAll("script:not([src])")];
    if (scripts.length === 0) throw new Error("Managed renderer has no inline runtime");
    for (const script of scripts) new Function(script.textContent || "")();
  };
  addEventListener("error", (event) => report("synth.visual.managed.error", event.message));
  addEventListener("unhandledrejection", (event) => report("synth.visual.managed.error", event.reason));
  addEventListener("message", (event) => {
    const data = event.data || {};
    try {
      if (data.type === "synth.visual.managed.load.v1") {
        if (!initialized) { renderSource(String(data.source || "")); initialized = true; }
        latestPayload = data.payload || {};
        // The telemetry lane can update independently of media. Never wait for
        // a frame body before delivering run progress.
        deliver();
        report("synth.visual.managed.ready", "ready");
        return;
      }
      if (data.type === "synth.visual.managed.frame-history.v1") {
        window.postMessage({
          type: "synth.visual.frame-history.v1",
          payload: { seed: data.seed, frames: Array.isArray(data.frames) ? data.frames : [] }
        }, "*");
        return;
      }
      if (data.type !== "synth.visual.managed.frame-chunk.v1") return;
      const seed = String(data.seed || "");
      const frameSequence = Number(data.frameSequence);
      const index = Number(data.index);
      const total = Number(data.total);
      if (!seed || !Number.isSafeInteger(frameSequence) || !Number.isSafeInteger(index) || !Number.isSafeInteger(total) || total < 1 || index < 0 || index >= total || typeof data.chunk !== "string") return;
      const key = seed + ":" + frameSequence;
      const chunks = frameChunks.get(key) || new Array(total);
      if (chunks.length !== total) return;
      chunks[index] = data.chunk;
      frameChunks.set(key, chunks);
      if (chunks.filter((chunk) => typeof chunk === "string").length !== total) return;
      const dataUrl = "data:" + String(data.contentType || "image/png") + ";base64," + chunks.join("");
      frameChunks.delete(key);
      // Media has its own delta lane. Reposting latestPayload here would clone
      // all ten base64 thumbnails for every single changed seed.
      window.postMessage({
        type: "synth.visual.frame-delta.v1",
        payload: { seed: Number(seed), frameSequence, dataUrl, mode: data.mode || "live", frame: data.frameRef || null }
      }, "*");
    } catch (error) {
      report("synth.visual.managed.error", error && error.message ? error.message : error);
    }
  });
})();`;

function managedRuntimeDocument() {
	const runtime = `data:text/javascript;charset=utf-8,${encodeURIComponent(MANAGED_HTML_RUNTIME)}`;
	return `<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; script-src data: 'unsafe-eval'; img-src data:"><script src="${runtime}"></script></head><body><p id="managed-visual-status">Loading managed visual…</p></body></html>`;
}

function postManagedFrame(
	target: Window,
	content: { frame: { seed: number; frameSequence: number; contentType: string }; base64: string },
	mode: "live" | "history"
) {
	const chunks = content.base64.match(/[\s\S]{1,16384}/g) ?? [];
	for (const [index, chunk] of chunks.entries()) {
		target.postMessage({
			type: "synth.visual.managed.frame-chunk.v1",
			seed: content.frame.seed,
			frameSequence: content.frame.frameSequence,
			contentType: content.frame.contentType,
			mode,
			frameRef: content.frame,
			index,
			total: chunks.length,
			chunk,
		}, "*");
	}
}

function promoteRetainedFrames(value: unknown): unknown {
	if (!value || typeof value !== "object" || Array.isArray(value)) return value;
	const record = value as Record<string, unknown>;
	const mediaBySeed = record.mediaBySeed && typeof record.mediaBySeed === "object"
		? { ...(record.mediaBySeed as Record<string, unknown>) }
		: {};
	let changed = false;
	const project = (candidate: unknown): unknown => {
		if (!candidate || typeof candidate !== "object" || Array.isArray(candidate)) return candidate;
		const event = candidate as Record<string, unknown>;
		if (!event.item || typeof event.item !== "object" || Array.isArray(event.item)) return candidate;
		const item = event.item as Record<string, unknown>;
		const retained = item.retainedFrame ?? item.retained_frame;
		if (!retained || typeof retained !== "object" || Array.isArray(retained)) return candidate;
		const frame = retained as Record<string, unknown>;
		const seed = Number(frame.seed);
		const chunks = frame.chunks;
		if (!Number.isSafeInteger(seed) || !Array.isArray(chunks) || !chunks.every((chunk) => typeof chunk === "string")) return candidate;
		const dataUrl = chunks.join("");
		if (!dataUrl.startsWith("data:image/png;base64,")) return candidate;
		mediaBySeed[String(seed)] = { frame: { data_url: dataUrl } };
		const { retainedFrame: _camel, retained_frame: _snake, ...rest } = item;
		changed = true;
		return { ...event, item: Object.keys(rest).length > 0 ? rest : null };
	};
	const events = Array.isArray(record.events) ? record.events.map(project) : record.events;
	const enrichmentEvents = Array.isArray(record.enrichmentEvents)
		? record.enrichmentEvents.map(project)
		: record.enrichmentEvents;
	return changed ? { ...record, events, enrichmentEvents, mediaBySeed } : value;
}

export function managedHtmlPayload(value: unknown): unknown {
	if (value && typeof value === "object") {
		const record = value as Record<string, unknown>;
		if (record.type === "synth.visual.update.v1") return record.payload ?? {};
		// Canonical bindings expose an inline value by its declared input name.
		// A managed package's input is commonly named `payload`, so unwrap that
		// binding envelope only when it is itself an update message; never guess
		// through an arbitrary application payload that happens to have a field
		// with the same name.
		if (record.payload && typeof record.payload === "object") {
			const nested = record.payload as Record<string, unknown>;
			if (nested.type === "synth.visual.update.v1") return nested.payload ?? {};
		}
		if (Array.isArray(record.frames) && record.frames.length > 0) {
			return managedHtmlPayload(record.frames[record.frames.length - 1]);
		}
		// Managed imports may declare their inline update input as `frames` even
		// when the persisted value is a single canonical update envelope. Treat
		// that declared binding envelope the same way as an array replay frame.
		if (record.frames && typeof record.frames === "object") {
			return managedHtmlPayload(record.frames);
		}
	}
	return promoteRetainedFrames(value) ?? {};
}

function ManagedHtmlFrame({ source, payload, title }: { source: string; payload: unknown; title?: string }) {
	const frame = useRef<HTMLIFrameElement>(null);
	const [loaded, setLoaded] = useState(false);
	const [runtimeError, setRuntimeError] = useState<string | null>(null);
	const [frameStreamError, setFrameStreamError] = useState<string | null>(null);
	const [nativeFrameCount, setNativeFrameCount] = useState(0);
	const admittedPayload = useMemo(() => managedHtmlPayload(payload), [payload]);
	const optimizerRunId = admittedPayload && typeof admittedPayload === "object" && !Array.isArray(admittedPayload)
		? (((admittedPayload as Record<string, unknown>).run as Record<string, unknown> | undefined)?.id as string | undefined)
		: undefined;
	useEffect(() => {
		const onMessage = (event: MessageEvent) => {
			if (event.source !== frame.current?.contentWindow) return;
			const data = event.data as { type?: unknown; message?: unknown } | null;
			if (data?.type === "synth.visual.managed.error") {
				setRuntimeError(typeof data.message === "string" ? data.message : "Managed renderer failed");
			}
			if (
				data?.type === "synth.visual.managed.frame-request.v1"
				&& optimizerRunId
				&& bridges.optimizers?.frameContent
			) {
				const request = data as { seed?: unknown; frameSequence?: unknown };
				const seed = Number(request.seed);
				const frameSequence = Number(request.frameSequence);
				if (!Number.isSafeInteger(seed) || !Number.isSafeInteger(frameSequence)) return;
				void bridges.optimizers.frameContent(optimizerRunId, seed, frameSequence).then((content) => {
					if (frame.current?.contentWindow) {
						postManagedFrame(frame.current.contentWindow, content, "history");
						setFrameStreamError(null);
					}
				}).catch((reason) => setFrameStreamError(publicError(reason, "Historical frame load failed")));
			}
		};
		addEventListener("message", onMessage);
		return () => removeEventListener("message", onMessage);
	}, [optimizerRunId]);
	useEffect(() => {
		if (!loaded || !frame.current?.contentWindow) return;
		// The public runtime is an external, app-bundled script. That matters on
		// WebKit: srcdoc and data documents inherit the host's inline-script CSP,
		// so a reviewed renderer can bind successfully yet paint nothing. The
		// sandboxed runtime accepts the immutable source once, executes its inline
		// script under its narrower CSP, and relays subsequent update frames.
		const record = admittedPayload && typeof admittedPayload === "object" && !Array.isArray(admittedPayload)
			? admittedPayload as Record<string, unknown>
			: {};
		const { mediaBySeed: _media, ...basePayload } = record;
		frame.current.contentWindow.postMessage({
			type: "synth.visual.managed.load.v1",
			source,
			payload: basePayload,
		}, "*");
	}, [admittedPayload, loaded, source]);
	useEffect(() => {
		if (!loaded || !optimizerRunId || !bridges.optimizers?.framesLatest || !bridges.optimizers.frameContent) return;
		let cancelled = false;
		let frameCursor = 0;
		let timer: ReturnType<typeof globalThis.setTimeout> | null = null;
		let polling = false;
		const latestBySeed = new Map<number, number>();
		const historyLoaded = new Set<number>();
		const poll = async () => {
			if (cancelled || polling) return;
			polling = true;
			try {
				const delta = await bridges.optimizers!.framesLatest(optimizerRunId, frameCursor);
				const nextCursor = Math.max(frameCursor, delta.frameCursor);
				// Deliberately sequential: at most one decoded base64 frame is live in
				// the host while ten rollout thumbnails advance together.
				for (const next of delta.frames) {
					if (cancelled) return;
					if ((latestBySeed.get(next.seed) ?? -1) >= next.frameSequence) continue;
					const content = await bridges.optimizers!.frameContent(optimizerRunId, next.seed, next.frameSequence);
					if (cancelled || !frame.current?.contentWindow) return;
					latestBySeed.set(next.seed, next.frameSequence);
					postManagedFrame(frame.current.contentWindow, content, "live");
					if (!historyLoaded.has(next.seed) && bridges.optimizers?.framesList) {
						historyLoaded.add(next.seed);
						void bridges.optimizers.framesList(optimizerRunId, next.seed, undefined, 200).then((frames) => {
							frame.current?.contentWindow?.postMessage({
								type: "synth.visual.managed.frame-history.v1",
								seed: next.seed,
								frames,
							}, "*");
						}).catch(() => historyLoaded.delete(next.seed));
					}
				}
				if (!cancelled) {
					// Commit the durable cursor only after every changed body was posted.
					// A failed content read then retries the same delta; already-delivered
					// seeds are skipped by latestBySeed without cloning their PNG again.
					frameCursor = nextCursor;
					setNativeFrameCount(latestBySeed.size);
					setFrameStreamError(null);
				}
			} catch (reason) {
				// Media is an independent, retryable lane. A transient read failure must
				// never replace the still-valid telemetry visual or reset its state.
				if (!cancelled) setFrameStreamError(publicError(reason, "Native frame stream failed"));
			} finally {
				polling = false;
				if (!cancelled) timer = globalThis.setTimeout(poll, 750);
			}
		};
		void poll();
		return () => {
			cancelled = true;
			if (timer != null) globalThis.clearTimeout(timer);
		};
	}, [loaded, optimizerRunId]);
	if (runtimeError) return <p role="alert" data-testid="visual-managed-html-error">Managed visual failed: {runtimeError}</p>;
	return <iframe
		ref={frame}
		title={`${title ?? "Managed visual"} · ${nativeFrameCount} native frames${frameStreamError ? " · media retrying" : ""}`}
		data-testid="visual-managed-html"
		sandbox="allow-scripts"
		srcDoc={managedRuntimeDocument()}
		onLoad={() => setLoaded(true)}
		style={{ border: 0, display: "block", height: "100%", minHeight: 420, width: "100%" }}
	/>;
}

function TemplateVisualHost({ artifact }: { artifact: ArtifactRef }) {
	const [Shell, setShell] = useState<ComponentType<ShellProps> | null>(null);
	const [failed, setFailed] = useState(false);
	const [optimizerPayload, setOptimizerPayload] = useState<Record<string, unknown> | null>(null);
	const [receiptVerdict, setReceiptVerdict] = useState<ReceiptVerdict>({ kind: "unverified", reason: "no_receipt" });
	const [optimizerLoadError, setOptimizerLoadError] = useState<string | null>(null);
	const [comparisonPayload, setComparisonPayload] = useState<Record<string, unknown> | null>(null);
	const [progressView, setProgressView] = useState<ProgressAgreement | null>(null);
	// One reader decides whether these bindings are legible, and it can say no.
	// Returning an empty slot list for a shape it did not understand is how a
	// visual with ten declared streams rendered an empty pane with no error.
	const resolvedBindings = useMemo(() => resolveVisualBindings(artifact.bindings), [artifact.bindings]);
	const asyncBindings = useMemo(
		() =>
			resolvedBindings.slots.filter((binding) => {
				if (binding.kind === "trace_v5") return true;
				if (
					binding.kind === "live_sse"
					|| binding.kind === "optimizer_run"
					|| binding.kind === "inline"
					|| binding.kind === "fixture"
				) return false;
				return binding.data === undefined;
			}),
		[resolvedBindings]
	);
	const traceBindings = useMemo(
		() => asyncBindings.filter((binding) => binding.kind === "trace_v5"),
		[asyncBindings]
	);
	const replay = useMemo(
		() => replayStreamsFromBindings(resolvedBindings.slots),
		[resolvedBindings]
	);
	const replayClient = useMemo(
		() =>
			// Native allowlisted polling when the host offers it. Outside the
			// packaged app — browser preview, tests — the client falls back to
			// fetch. The capability is checked, not assumed: a bridge without
			// `pollStream` would otherwise throw on the first poll.
			createReplayClient(replay.streams, typeof bridges.visuals?.pollStream === "function"
				? (pollUrl, after, limit) =>
						bridges.visuals!.pollStream({ visualId: artifact.id, pollUrl, after, limit })
				: undefined),
		[artifact.id, replay.streams]
	);
	const mediaClient = useMemo(
		() =>
			// Same shape as the replay client: the capability is checked rather
			// than assumed, and a template that gets no transport renders its
			// frame references as unavailable instead of throwing on first load.
			createMediaClient(
				typeof bridges.visuals?.readMedia === "function"
					? (casDigest) =>
							bridges.visuals!.readMedia({ visualId: artifact.id, casDigest })
					: undefined
			),
		[artifact.id]
	);
	const synchronouslyResolved = useMemo(() => {
		if (!isVisualBindings(artifact.bindings) || asyncBindings.length === 0) {
			return propsFromBindings(artifact.bindings);
		}
		const skip = new Set(asyncBindings.map((binding) => `${bindingInputName(binding)}:${binding.kind}:${binding.source ?? ""}`));
		return propsFromBindings({
			schemaVersion: "synth.visual-bindings.v1",
			inputs: resolvedBindings.slots.filter((binding) =>
				!skip.has(`${bindingInputName(binding)}:${binding.kind}:${binding.source ?? ""}`)
			),
			slots: resolvedBindings.slots.filter((binding) =>
				!skip.has(`${bindingInputName(binding)}:${binding.kind}:${binding.source ?? ""}`)
			)
		});
	}, [artifact.bindings, asyncBindings]);
	const [traceResolution, setTraceResolution] = useState<{
		status: "idle" | "loading" | "ready" | "error";
		props: Record<string, unknown>;
		error?: string;
		/** Structured form of the same failure, so the pane can show the stable
		 * code and its remediation instead of only a sentence. */
		failure?: PublicError;
	}>({ status: "idle", props: {} });
	const [lastKnownGoodProps, setLastKnownGoodProps] = useState<Record<string, unknown> | null>(null);
	const [connectionState, setConnectionState] = useState<
		"loading" | "replaying" | "subscribed" | "stale" | "reconnecting" | "terminal" | "failed" | "interrupted"
	>("loading");
	const [sealedTraceProjections, setSealedTraceProjections] = useState<Array<{
		trialId: string;
		rolloutId: string | null;
		digest: string;
		projection: unknown;
	}>>([]);

	const visualIdentity = useMemo(
		() => ({
			visualId: artifact.visualId ?? artifact.id,
			visualRevision: typeof artifact.revision === "number" ? artifact.revision : null,
		}),
		[artifact.id, artifact.visualId, artifact.revision]
	);
	const optimizerRunId = resolvedBindings.status !== "rejected"
		? resolvedBindings.slots.find(
			(entry) => bindingInputName(entry) === "optimizer_run" && entry.kind === "optimizer_run"
		)?.source
		: undefined;
	// Keep the bounded read-model summary live for the visual. Besides making
	// config/runtime/count facts available without the journal, this is the
	// invalidation source for any mounted collection pages below.
	const optimizerSummaryState = useOptimizerRun(optimizerRunId);
	const evidenceClient = useMemo(
		() =>
			// Lazy raw-journal access for the detail surfaces — Replay, the
			// transcript, frame drill-down — that genuinely need events. The
			// aggregate never waits for it. Same capability check as the two
			// clients above: without the bridge method there is no client, and
			// a template renders its evidence panel as unavailable rather than
			// throwing on first open.
			optimizerRunId && typeof bridges.optimizers?.evidencePage === "function"
				? createEvidenceClient(optimizerRunId, {
					evidencePage: (runId, window, held, limit) =>
						bridges.optimizers!.evidencePage(runId, window, held, limit)
				})
				: undefined,
		[optimizerRunId]
	);
	const collectionsClient = useMemo(
		() =>
			// Keyset-paged durable collections for the bound run. Templates page
			// candidates, rollouts, and proposer calls through this on intent.
			optimizerRunId && typeof bridges.optimizers?.runCollection === "function"
				? {
					page: (collection: Parameters<NonNullable<typeof bridges.optimizers>["runCollection"]>[1], query: Parameters<NonNullable<typeof bridges.optimizers>["runCollection"]>[2]) =>
						bridges.optimizers!.runCollection(optimizerRunId, collection, query),
					item: (collection: Parameters<NonNullable<typeof bridges.optimizers>["runCollectionItem"]>[1], itemId: string) =>
						bridges.optimizers!.runCollectionItem(optimizerRunId, collection, itemId),
					subscribePage: (collection: Parameters<NonNullable<typeof bridges.optimizers>["runCollection"]>[1], query: Parameters<NonNullable<typeof bridges.optimizers>["runCollection"]>[2], listener: (state: unknown) => void) =>
						subscribeRunCollection(optimizerRunId, collection, query, listener as Parameters<typeof subscribeRunCollection>[3]),
					subscribeItem: (collection: Parameters<NonNullable<typeof bridges.optimizers>["runCollectionItem"]>[1], itemId: string, listener: (state: unknown) => void) =>
						subscribeRunCollectionItem(optimizerRunId, collection, itemId, listener as Parameters<typeof subscribeRunCollectionItem>[3])
				}
				: undefined,
		[optimizerRunId]
	);
	const historyClient = useMemo(
		() =>
			// Backend checkpointed projections for the historical scrubber. The
			// shell reads the state at a sequence through this instead of
			// reducing the journal in the renderer.
			optimizerRunId && typeof bridges.optimizers?.projectionAt === "function"
				? {
					projectionAt: (sequence: number) => bridges.optimizers!.projectionAt(optimizerRunId, sequence)
				}
				: undefined,
		[optimizerRunId]
	);
	const templateDigest = typeof artifact.metadata?.templateDigest === "string"
		? artifact.metadata.templateDigest
		: undefined;

	useEffect(() => {
		if (resolvedBindings.status === "canonical") return;
		if (resolvedBindings.status === "rejected") {
			reportDiagnostic({
				...visualIdentity,
				severity: "error",
				component: "visual-host",
				event: "visual.bindings.invalid",
				code: DIAGNOSTIC_CODES.visualBindingsInvalid,
				message: resolvedBindings.error ?? "Visual bindings are unreadable",
				details: { templateId: artifact.templateId ?? null }
			});
			return;
		}
		// COMPAT: rendered from an upgraded legacy shape. Loud so the writer is
		// fixed before the upgrade path is removed.
		reportDiagnostic({
			...visualIdentity,
			severity: "warn",
			component: "visual-host",
			event: "visual.bindings.upgraded",
			code: DIAGNOSTIC_CODES.visualBindingsUpgraded,
			message: `Rendered from upgraded legacy bindings on ${resolvedBindings.upgradedSlots.join(", ")}`,
			details: { templateId: artifact.templateId ?? null, inputs: resolvedBindings.upgradedSlots }
		});
	}, [artifact.templateId, resolvedBindings, visualIdentity]);

	useEffect(() => {
		let cancelled = false;
		const templateId = artifact.templateId;
		setFailed(false);
		setShell(null);
		if (!templateId) {
			setFailed(true);
			reportDiagnostic({
				...visualIdentity,
				severity: "error",
				component: "visual-host",
				event: "visual.template.missing",
				code: DIAGNOSTIC_CODES.visualTemplateUnavailable,
				message: "Visual has no template id to render",
			});
			return;
		}
		const load = async () => {
			if (artifact.rendererKind === "html") {
				const visualId = artifact.visualId ?? artifact.id;
				try {
					const asset = await bridges.visuals?.content?.(visualId);
					const source = asset?.base64 ? decodeBase64Utf8(asset.base64) : "";
					if (!source) throw new Error("Managed renderer source is unavailable");
					if (!cancelled) setShell(() => (props: ShellProps) => <ManagedHtmlFrame source={source} payload={props.data} title={artifact.title} />);
				} catch (reason) {
					if (!cancelled) {
						setFailed(true);
						reportDiagnostic({
							...visualIdentity,
							severity: "error",
							component: "visual-host",
							event: "visual.managed_html.load_failed",
							code: DIAGNOSTIC_CODES.visualShellLoadFailed,
							message: publicError(reason),
							details: { templateId },
						});
					}
				}
				return;
			}
			if (isSourcedTemplate(templateId)) {
				const visualId = artifact.visualId ?? artifact.id;
				let source = "";
				try {
					const asset = await bridges.visuals?.content?.(visualId);
					if (asset?.base64) source = decodeBase64Utf8(asset.base64);
				} catch (reason) {
					if (!cancelled) setShell(() => sourcedInvalidShell(publicError(reason)));
					return;
				}
				const compiled = compileSourcedModule(source);
				if (cancelled) return;
				if (!compiled.ok) {
					setShell(() => sourcedInvalidShell(compiled.error));
					return;
				}
				setShell(() => compiled.Shell);
				return;
			}
			const Component = await loadVisualShell(templateId);
			if (cancelled) return;
			if (!Component) {
				setFailed(true);
				reportDiagnostic({
					...visualIdentity,
					severity: "error",
					component: "visual-host",
					event: "visual.shell.unavailable",
					code: DIAGNOSTIC_CODES.visualTemplateUnavailable,
					message: `Template ${templateId} resolved no shell component`,
					details: { templateId },
				});
			} else setShell(() => Component);
		};
		void load().catch((reason) => {
			if (cancelled) return;
			if (isSourcedTemplate(templateId)) {
				setShell(() => sourcedInvalidShell(publicError(reason)));
				return;
			}
			setFailed(true);
			reportDiagnostic({
				...visualIdentity,
				severity: "error",
				component: "visual-host",
				event: "visual.shell.load_failed",
				code: DIAGNOSTIC_CODES.visualShellLoadFailed,
				message: publicError(reason),
				details: { templateId },
			});
		});
		return () => { cancelled = true; };
	}, [artifact.templateId, artifact.visualId, artifact.id, artifact.contentDigest, artifact.revision, visualIdentity]);

	useEffect(() => {
		let cancelled = false;
		if (asyncBindings.length === 0 || !isVisualBindings(artifact.bindings)) {
			setTraceResolution({ status: "idle", props: {} });
			return () => { cancelled = true; };
		}
		const bindings = artifact.bindings;
		const template = artifact.templateId ? resolveTemplate(artifact.templateId) : undefined;
		if (!template) {
			setTraceResolution({ status: "error", props: {}, error: `Template ${artifact.templateId ?? "unknown"} is unavailable` });
			reportDiagnostic({
				...visualIdentity,
				severity: "error",
				component: "visual-host",
				event: "visual.template.unavailable",
				code: DIAGNOSTIC_CODES.visualTemplateUnavailable,
				message: `Template ${artifact.templateId ?? "unknown"} is unavailable`,
				details: { templateId: artifact.templateId ?? null },
			});
			return () => { cancelled = true; };
		}
		if (traceBindings.length > 0 && !bridges.inventory) {
			setTraceResolution({ status: "error", props: {}, error: "Trace projection resolver is unavailable" });
			return () => { cancelled = true; };
		}
		const unsupportedBinding = traceBindings.find((binding) =>
			binding.schema && binding.schema !== "synth.trace-projection.rollout-inspector.v1"
		);
		if (unsupportedBinding) {
			setTraceResolution({ status: "error", props: {}, error: `Unsupported trace projection schema: ${unsupportedBinding.schema}` });
			// The failure this whole diagnostic system was built for: ten sealed
			// traces, an empty pane, and no way to ask why. Emit the received
			// schema, the accepted one, and every identity the binding names.
			reportDiagnostic({
				...visualIdentity,
				traceId: typeof unsupportedBinding.source === "string" ? unsupportedBinding.source : null,
				severity: "error",
				component: "visual-host",
				event: "visual.projection.rejected",
				code: DIAGNOSTIC_CODES.unsupportedTraceProjectionSchema,
				message: `Unsupported trace projection schema: ${unsupportedBinding.schema}`,
				details: {
					receivedSchema: unsupportedBinding.schema ?? null,
					expectedSchemas: ["synth.trace-projection.rollout-inspector.v1"],
					templateId: artifact.templateId ?? null,
					slot: bindingInputName(unsupportedBinding) ?? null,
					input: bindingInputName(unsupportedBinding) ?? null,
					remediation: "Project Trace V5 into the visual's accepted input contract.",
				},
			});
			return () => { cancelled = true; };
		}

		setTraceResolution({ status: "loading", props: {} });
		const projectionByDigest = new Map<string, Promise<unknown>>();
		const loadTraceV5 = (source: string) => {
			let pending = projectionByDigest.get(source);
			if (!pending) {
				pending = bridges.inventory!.resolveTraceProjection(source, "rollout-inspector").then((projection) => {
					if (projection.traceDigest !== source) {
						throw new Error(`Trace resolver returned digest ${projection.traceDigest} for ${source}`);
					}
					if (projection.projectionKind !== "rollout-inspector") {
						throw new Error(`Unsupported trace projection kind: ${projection.projectionKind}`);
					}
					if (projection.projectionSchema !== "synth.trace-projection.rollout-inspector.v1") {
						throw new Error(`Unsupported trace projection schema: ${projection.projectionSchema}`);
					}
					return projection.payload;
				});
				projectionByDigest.set(source, pending);
			}
			return pending;
		};
		const loadLocalCas = (source: string) => {
			if (!bridges.runtime) throw new Error(`No local CAS loader for ${source}`);
			return bridges.runtime.request(`/v1/cas/${encodeURIComponent(source)}`);
		};
		const loadQuerySnapshot = (source: string) => {
			if (!bridges.runtime) throw new Error(`No query snapshot loader for ${source}`);
			return bridges.runtime.request("/v1/traces/snapshot", { method: "POST", body: { snapshot_id: source } });
		};
		const loadRun = (source: string) => {
			if (!bridges.optimizers) throw new Error(`No run loader for ${source}`);
			return bridges.optimizers.get(source);
		};
		void bindTemplateSlots(template, bindings, { loadTraceV5, loadLocalCas, loadQuerySnapshot, loadRun, skipOptional: true })
			.then((result) => {
				if (cancelled) return;
				if (result.errors.length > 0) {
					setTraceResolution({ status: "error", props: {}, error: result.errors.join(" · ") });
					reportDiagnostic({
						...visualIdentity,
						severity: "error",
						component: "visual-host",
						event: "visual.binding.unresolved",
						code: DIAGNOSTIC_CODES.visualBindingUnresolved,
						message: result.errors.join(" · "),
						details: { templateId: artifact.templateId ?? null, errorCount: result.errors.length },
					});
					return;
				}
				const props = Object.fromEntries(
					Object.values(result.slots)
						.filter((slot) => slot.kind !== "inline" && slot.kind !== "live_sse" && slot.kind !== "optimizer_run")
						.map((slot) => [slot.input ?? slot.slot, slot.data])
				);
				setTraceResolution({ status: "ready", props });
				setLastKnownGoodProps((current) => rememberLastKnownGood(current, props, false));
			})
			.catch((reason) => {
				if (cancelled) return;
				const failure = toPublicError(reason, "Trace projection resolution failed");
				const message = publicError(reason, "Trace projection resolution failed");
				setTraceResolution({ status: "error", props: {}, error: message, failure });
				reportDiagnostic({
					...visualIdentity,
					severity: "error",
					component: "visual-host",
					event: "visual.projection.failed",
					// A projection-schema mismatch thrown from the resolver is the
					// same defect as the one caught above; keep one code so a
					// query for it finds both spellings of the failure.
					code: message.includes("projection schema")
						? DIAGNOSTIC_CODES.unsupportedTraceProjectionSchema
						: DIAGNOSTIC_CODES.visualBindingUnresolved,
					message,
					details: { templateId: artifact.templateId ?? null },
				});
			});
		return () => { cancelled = true; };
	}, [artifact.id, artifact.revision, artifact.templateId, artifact.bindings, asyncBindings.length, traceBindings.length, visualIdentity]);

	/*
	 * The optimizer stream is read through the shared `RunProgressSubscription`
	 * store, not a private loop here. One run can be open in the transcript card,
	 * its dialog, and this pane at once; the store gives all three the same
	 * cursor, the same gap recovery, and one set of upstream reads.
	 *
	 * What stays local to the pane is what is genuinely the visual's: the ready
	 * receipt, and a visual-scoped copy of a stream failure so a blank pane and
	 * the run behind it remain joinable by visual id.
	 */
	useEffect(() => {
		if (!optimizerRunId) {
			setOptimizerPayload(null);
			setOptimizerLoadError(null);
			setProgressView(null);
			return;
		}
		let postedReady = false;
		let verifiedReceipt = false;
		setReceiptVerdict({ kind: "unverified", reason: "no_receipt" });
		return subscribeToRun(optimizerRunId, (snapshot) => {
			const projection = projectRunProgress(snapshot, Date.now());
			const agreement = projection ? progressAgreement(projection) : null;
			setProgressView(agreement);
			const lanes = snapshot.run ? splitSnapshotEvents(snapshot.run, snapshot.events) : null;
			const payload = snapshot.run && lanes
				? {
					run: snapshot.run,
					runViewV2: snapshot.viewV2,
					runProgress: agreement,
					events: lanes.terminalEvents,
					enrichmentEvents: lanes.enrichmentEvents,
					terminalCursor: lanes.terminalCursor,
					enrichmentCursor: lanes.enrichmentCursor,
					// Aggregate surfaces are already authoritative here; raw
					// evidence may still be arriving. A template that draws
					// from `events` — Replay, the transcript, frame drill-down
					// — reads this instead of inferring emptiness from a
					// zero-length array it cannot distinguish from a run that
					// genuinely produced nothing.
					evidenceState: snapshot.evidence
				}
				: null;

			if (snapshot.state === "unavailable") {
				setOptimizerPayload(payload);
				setOptimizerLoadError(snapshot.error ?? "Optimizer bridge is unavailable");
				setConnectionState("failed");
				return;
			}
			if (snapshot.state === "interrupted" || snapshot.state === "failed") {
				if (payload) setOptimizerPayload(payload);
				setOptimizerLoadError(snapshot.error ?? "Optimizer stream interrupted");
				setConnectionState(snapshot.state);
				reportDiagnostic({
					...visualIdentity,
					optimizerRunId,
					streamId: optimizerRunId,
					severity: "error",
					component: "visual-host",
					event: "stream.interrupted",
					code: DIAGNOSTIC_CODES.streamInterrupted,
					message: snapshot.error ?? "Optimizer stream interrupted",
					retryable: true,
				});
				return;
			}
			if (snapshot.state === "stale") {
				if (payload) setOptimizerPayload(payload);
				setOptimizerLoadError(null);
				setConnectionState("stale");
				reportDiagnostic({
					...visualIdentity,
					optimizerRunId,
					streamId: optimizerRunId,
					severity: "warn",
					component: "visual-host",
					event: "stream.replay.gap",
					code: DIAGNOSTIC_CODES.streamReplayGap,
					message: `Optimizer event history is incomplete at ${snapshot.cursor}`,
					retryable: true,
					details: { cursor: snapshot.cursor, gap: snapshot.gap },
				});
				return;
			}
			if (!snapshot.run || !payload) {
				setConnectionState(snapshot.state === "loading" ? "loading" : "replaying");
				return;
			}
			setOptimizerPayload(payload);
			setOptimizerLoadError(null);
			setConnectionState(
				snapshot.state === "terminal" ? "terminal"
					: snapshot.state === "reconnecting" ? "reconnecting"
						: snapshot.state === "replaying" ? "replaying"
							: "subscribed"
			);
			if (!verifiedReceipt && snapshot.viewV2) {
				verifiedReceipt = true;
				const view = snapshot.viewV2;
				// Compare before recording, or the receipt this render is about
				// to write would be the thing it is compared against.
				void Promise.resolve(
					bridges.optimizers?.visualRenderReceipt?.(
						artifact.id,
						typeof artifact.revision === "number" ? artifact.revision : 0
					)
				)
					.then((receipt) => {
						const verdict = verifyAgainstReceipt(receipt, {
							optimizerRunId,
							projectionRevision: view.header.projectionRevision,
							dataDigest: visualDataDigest(view),
							templateVersion: templateDigest ?? ""
						});
						setReceiptVerdict(verdict);
						if (verdict.kind === "regressed" || verdict.kind === "content_changed") {
							reportDiagnostic({
								...visualIdentity,
								optimizerRunId,
								severity: "warn",
								component: "visual-host",
								event: "visual.receipt.mismatch",
								code: DIAGNOSTIC_CODES.streamReplayGap,
								message: verdict.kind === "regressed"
									? `Local evidence is at projection revision ${verdict.localRevision}, behind the ${verdict.renderedRevision} this visual already rendered.`
									: `Projection revision ${verdict.projectionRevision} now carries different content than when this visual rendered.`,
								retryable: true,
								details: { verdict: verdict.kind, renderedAt: verdict.renderedAt },
							});
						}
					})
					.catch(() => undefined);
			}
			if (!postedReady && snapshot.viewV2) {
				postedReady = true;
				const projectionRevision = snapshot.viewV2.header.projectionRevision;
				void bridges.optimizers?.recordVisualReady?.({
					visualId: artifact.id,
					optimizerRunId,
					templateId: artifact.templateId ?? "optimizer.run.v1",
					replayedThrough: snapshot.cursor,
					subscribedFrom: snapshot.cursor + 1,
					templateDigest,
					visualRevision: typeof artifact.revision === "number" ? artifact.revision : 0,
					projectionRevision,
					// Identity of what was actually rendered, not of the whole
					// run: the same projection revision carrying different
					// content is exactly the case a bare revision misses.
					dataDigest: visualDataDigest(snapshot.viewV2)
				}).catch(() => undefined);
			}
		}, { evidence: "auto" });
	}, [artifact.id, artifact.templateId, optimizerRunId, templateDigest, visualIdentity]);

	// A container eval imports one sealed Trace V5 bundle per terminal trial.
	// The digest is recorded inside that trial's durable terminal event rather
	// than as a static visual binding, because it does not exist when the live
	// workbench is minted. Resolve those digests here and hand the projections
	// to the same shell that is already rendering the live fold.
	useEffect(() => {
		let cancelled = false;
		// Keyed off the trace-workbench template set, not one hardcoded id, so
		// the family-agnostic workstation resolves its sealed trials the same way.
		if (!artifact.templateId || !SEALED_TRACE_WORKBENCH_TEMPLATES.has(artifact.templateId) || !bridges.inventory) {
			setSealedTraceProjections([]);
			return () => { cancelled = true; };
		}
		const allEvents = [
			...(Array.isArray(optimizerPayload?.events) ? optimizerPayload.events : []),
			...(Array.isArray(optimizerPayload?.enrichmentEvents) ? optimizerPayload.enrichmentEvents : [])
		] as Array<Record<string, any>>;
		const refs = allEvents.flatMap((event) => {
			if ((event.type ?? event.eventType) !== "eval.trial.terminal") return [];
			const item = event.item ?? {};
			const record = item.raw ?? item;
			const sealed = record.sealedTrace ?? record.sealed_trace;
			if (!sealed?.inspectable || !Array.isArray(sealed.traces)) return [];
			const trialId = String(event.delta?.trial_id ?? record.trialId ?? item.id ?? "");
			const rolloutId = typeof record.rolloutId === "string" ? record.rolloutId : null;
			return sealed.traces.flatMap((trace: Record<string, unknown>) =>
				typeof trace.digest === "string" && trialId
					? [{ trialId, rolloutId, digest: trace.digest }]
					: []
			);
		});
		if (refs.length === 0) {
			setSealedTraceProjections([]);
			return () => { cancelled = true; };
		}
		void Promise.all(refs.map(async (ref) => {
			const resolved = await bridges.inventory!.resolveTraceProjection(ref.digest, "rollout-inspector");
			if (resolved.traceDigest !== ref.digest || resolved.projectionKind !== "rollout-inspector") {
				throw new Error(`Sealed trace projection identity changed for ${ref.digest}`);
			}
			return { ...ref, projection: resolved.payload };
		})).then((rows) => {
			if (!cancelled) setSealedTraceProjections(rows);
		}).catch((reason) => {
			if (cancelled) return;
			setSealedTraceProjections([]);
			reportDiagnostic({
				...visualIdentity,
				optimizerRunId: optimizerRunId ?? null,
				severity: "error",
				component: "visual-host",
				event: "visual.sealed_trace.resolve_failed",
				code: DIAGNOSTIC_CODES.visualBindingUnresolved,
				message: publicError(reason, "Sealed trace projection failed")
			});
		});
		return () => { cancelled = true; };
	}, [artifact.templateId, optimizerPayload, optimizerRunId, visualIdentity]);

	const boundRun = optimizerPayload?.run as { id?: string; algorithmId?: string } | undefined;
	const boundRunId = boundRun?.algorithmId === "gepa" ? boundRun.id ?? null : null;
	useEffect(() => {
		// Best-effort companion run for the GEPA comparison card (Luna vs Sol):
		// the most recent sibling GEPA run sharing the recipe prefix of the id.
		// Comparison state comes from the same backend projection as the primary
		// run; this surface never reconstructs a sibling from raw events.
		if (!boundRunId || !bridges.optimizers) {
			setComparisonPayload(null);
			return;
		}
		let cancelled = false;
		void (async () => {
			try {
				const prefixOf = (id: string) => id.split("_").slice(0, 2).join("_");
				const runs = await bridges.optimizers!.list({ algorithmId: "gepa" });
				const sibling = runs
					.filter((item) => item.id !== boundRunId && prefixOf(item.id) === prefixOf(boundRunId))
					.sort((a, b) => Date.parse(b.createdAt ?? "") - Date.parse(a.createdAt ?? ""))[0];
				if (!sibling) return;
				const runViewV2 = await bridges.optimizers!.runViewV2(sibling.id);
				if (!cancelled) setComparisonPayload({ run: sibling, runViewV2 });
			} catch {
				// The comparison card is optional; the primary run view stands alone.
			}
		})();
		return () => {
			cancelled = true;
		};
	}, [boundRunId]);

	if (failed) return <VisualInvalidState title="Template unavailable" detail={`No bundled shell is registered for ${artifact.templateId ?? "this visual"}.`} />;
	if (resolvedBindings.status === "rejected") {
		return <VisualInvalidState title="Visual bindings unreadable" detail={resolvedBindings.error ?? "This visual's bindings could not be read."} />;
	}
	if (synchronouslyResolved.errors.length > 0) return <VisualInvalidState title="Visual data unavailable" detail={synchronouslyResolved.errors.join(" · ")} />;
	if (traceResolution.status === "loading" && !lastKnownGoodProps) return <p className="visual-loading" role="status">Loading sealed trace…</p>;
	const liveFailed = traceResolution.status === "error";
	const selected = selectRenderedProjection({
		live: liveFailed ? null : { ...synchronouslyResolved.props, ...traceResolution.props },
		lastKnownGood: lastKnownGoodProps,
		liveFailed
	});
	if (liveFailed && !selected.projection) {
		const detail = traceResolution.error ?? "Trace projection resolution failed";
		const lower = detail.toLowerCase();
		const title = lower.includes("quarant") ? "Trace is quarantined"
			: lower.includes("extractor") || lower.includes("projection kind") || lower.includes("not registered") ? "Trace extractor unavailable"
				: lower.includes("unsupported") || lower.includes("schema") ? "Unsupported trace schema"
					: lower.includes("not found") || lower.includes("missing") || lower.includes("archive") ? "Sealed trace archive missing"
						: lower.includes("unavailable") ? "Trace resolver unavailable" : "Trace data unavailable";
		return <VisualInvalidState
			title={title}
			detail={traceResolution.failure?.message ?? detail}
			code={traceResolution.failure?.code}
			remediation={traceResolution.failure?.remediation}
			traceId={typeof traceBindings[0]?.source === "string" ? traceBindings[0].source : undefined}
		/>;
	}
	if (optimizerRunId && !optimizerPayload) {
		if (optimizerLoadError) {
			return <VisualInvalidState
				title="Run evidence unavailable"
				detail={optimizerLoadError}
				remediation="Retry after Workshop reconnects to the optimizer journal."
			/>;
		}
		return (
			<div className="visual-optimizer-hydrating" role="status" aria-live="polite" data-testid="visual-optimizer-hydrating">
				<div className="visual-optimizer-hydrating-copy">
					<strong>Restoring run evidence…</strong>
					<span>Metrics and rollouts will appear together after the journal is hydrated.</span>
				</div>
				<div className="visual-optimizer-skeleton" aria-hidden="true">
					<span />
					<span />
					<span />
				</div>
			</div>
		);
	}
	if (!Shell) return <p className="visual-loading">Loading visual shell…</p>;
	if (
		consumeInjectedRendererCrash(
			artifact.visualId ?? artifact.id,
			typeof artifact.revision === "number" ? artifact.revision : null,
			artifact.metadata?.__crashRenderer === true
		)
	) {
		throw new Error("injected renderer crash");
	}
	const resolvedProps = selected.projection ?? { ...synchronouslyResolved.props, ...traceResolution.props };
	const degradedConnection = ["reconnecting", "failed", "interrupted"].includes(connectionState);
	const boundEvents = Array.isArray(optimizerPayload?.events) ? optimizerPayload.events as unknown[] : [];
	// Readiness describes the run, not the renderer's hydration. A
	// projection-only visual proves its candidates and rollouts from the
	// durable view; raw event length is the floor only when no view exists.
	const semanticCounts = semanticCountsFromRunView(
		optimizerPayload?.runViewV2 as Parameters<typeof semanticCountsFromRunView>[0],
		boundEvents.length
	);
	const optimizerEvidenceState = typeof optimizerPayload?.evidenceState === "string"
		? optimizerPayload.evidenceState
		: undefined;
	const runLifecycle = projectVisualRunLifecycle(
		optimizerPayload?.run as Parameters<typeof projectVisualRunLifecycle>[0],
		progressView
	);
	const boundStatus = typeof (optimizerPayload?.run as { status?: string } | undefined)?.status === "string"
		? (optimizerPayload?.run as { status?: string }).status ?? ""
		: "";
	const transportTerminal = runLifecycle?.terminal === true
		|| connectionState === "terminal"
		|| ["completed", "failed", "cancelled", "succeeded"].includes(boundStatus);
	return (
		<div
			data-testid="visual-template-shell"
			data-connection-state={connectionState}
			data-visual-transport-state={connectionState === "loading" ? "idle" : connectionState}
			data-visual-terminal={transportTerminal ? "true" : "false"}
			data-visual-evidence={optimizerEvidenceState}
			data-visual-receipt={receiptVerdict.kind}
			data-visual-semantic-event-count={String(semanticCounts.semanticEvents)}
			data-visual-rollout-count={String(semanticCounts.rollouts)}
			data-visual-semantic-source={semanticCounts.source}
			data-visual-raw-event-count={String(boundEvents.length)}
			data-visual-error={optimizerLoadError ?? (liveFailed ? traceResolution.error : undefined)}
			data-visual-projection-source={selected.source ?? "live"}
			data-visual-projection-stale={selected.stale ? "true" : undefined}
			data-visual-subscription={connectionState}
			data-visual-compute={transportTerminal ? "terminal" : "running"}
			data-visual-status={artifact.status ?? "draft"}
			data-visual-review={Array.isArray(artifact.metadata?.reviews) && artifact.metadata.reviews.length > 0 ? "review" : "none"}
			data-visual-readiness={artifact.status === "live" || artifact.status === "saved" ? "ready" : "waiting"}
			data-visual-pinning={artifact.metadata?.pinned === true ? "pinned" : "unpinned"}
			data-visual-sealing={artifact.metadata?.sealed === true || artifact.metadata?.seal ? "sealed" : "unsealed"}
			data-visual-sharing={typeof artifact.metadata?.visibility === "string" ? String(artifact.metadata.visibility) : "private"}
			data-progress-phase={progressView?.phaseId}
			data-progress-phase-label={progressView?.phaseLabel}
			data-progress-status={progressView?.status}
			data-progress-completed={progressView?.completed != null ? String(progressView.completed) : undefined}
			data-progress-total={progressView?.total != null ? String(progressView.total) : undefined}
			data-progress-cost={progressView ? (progressView.costUsd == null ? "unavailable" : String(progressView.costUsd)) : undefined}
			data-progress-tokens={progressView ? (progressView.promptTokens == null ? "unavailable" : String(progressView.promptTokens)) : undefined}
			data-progress-terminal={progressView ? String(progressView.terminal) : undefined}
			data-progress-result={progressView?.resultHeadline ?? progressView?.resultAbsentReason}
		>
			{selected.stale ? (
				<p className="visual-stale-projection" role="status" data-testid="visual-last-known-good">
					Showing last known good projection while live rendering recovers.
				</p>
			) : null}
			{degradedConnection ? <p className="visual-connection-state" role="status" data-testid="visual-connection-state">Visual connection {connectionState}.</p> : null}
			<Shell
				{...(resolvedProps as ShellProps)}
				title={artifact.title}
				lede={artifact.summary}
				bindings={artifact.bindings}
				visualMetadata={artifact.metadata}
				loadError={optimizerLoadError ?? undefined}
				{...(optimizerPayload ?? {})}
				data={optimizerPayload ?? resolvedProps.optimizer_run ?? resolvedProps}
				comparison={comparisonPayload ?? undefined}
				replay={replayClient}
				media={mediaClient}
				sealedTraceProjections={sealedTraceProjections}
				evidence={evidenceClient}
				history={historyClient}
				collections={collectionsClient}
				runSummary={optimizerSummaryState.summary ?? undefined}
				runSummaryStatus={optimizerSummaryState.status}
				tailCursor={typeof optimizerPayload?.terminalCursor === "number" ? optimizerPayload.terminalCursor : undefined}
				runLifecycle={runLifecycle}
				replayMissingTransport={replay.missingTransport}
				visualId={artifact.visualId ?? artifact.id}
				revision={typeof artifact.revision === "number" ? artifact.revision : null}
			/>
		</div>
	);
}

function numericAttribute(element: Element, name: string): number {
	const value = Number(element.getAttribute(name));
	return Number.isFinite(value) && value >= 0 ? value : 0;
}

/** The template reports runtime facts as DOM data, but Workshop owns the
 * extractor and readiness decision. Nothing supplied by a template is treated
 * as a passing boolean. */
function VisualObservationBoundary({ artifact, children }: { artifact: ArtifactRef; children: ReactNode }) {
	const root = useRef<HTMLDivElement>(null);
	const [bindingsDigest, setBindingsDigest] = useState<string | null>(null);
	const template = artifact.templateId ? resolveTemplate(artifact.templateId) : undefined;
	const contract = template?.observationContract;

	useEffect(() => {
		let cancelled = false;
		setBindingsDigest(null);
		if (!contract || !artifact.visualId || !artifact.revision || !bridges.visuals) return;
		void bridges.visuals.revisions(artifact.visualId).then((revisions) => {
			const digest = revisions.find((candidate) => candidate.revision === artifact.revision)?.bindingsDigest;
			if (!cancelled && digest) setBindingsDigest(digest);
		});
		return () => { cancelled = true; };
	}, [artifact.revision, artifact.visualId, contract]);

	useEffect(() => {
		const host = root.current;
		if (!host || !contract || !bindingsDigest || !artifact.visualId || !artifact.revision || !bridges.visuals) return;
		let frame: number | null = null;
		const publish = () => {
			frame = null;
			const surface = host.querySelector("[data-visual-transport-state]");
			if (!surface) return;
			const rawError = surface.getAttribute("data-visual-error")?.trim();
			void bridges.visuals?.reportObservation({
				schemaVersion: "synth.rendered-visual-observation.v1",
				visualId: artifact.visualId!,
				renderedRevision: artifact.revision!,
				bindingsDigest,
				transportState: surface.getAttribute("data-visual-transport-state") ?? "unknown",
				rolloutCount: numericAttribute(surface, "data-visual-rollout-count"),
				renderedFrameCount: numericAttribute(surface, "data-visual-rendered-frame-count"),
				semanticEventCount: numericAttribute(surface, "data-visual-semantic-event-count"),
				terminal: surface.getAttribute("data-visual-terminal") === "true",
				error: rawError || null,
				observedAt: new Date().toISOString()
			});
		};
		const schedule = () => {
			if (frame == null) frame = window.requestAnimationFrame(publish);
		};
		const observer = new MutationObserver(schedule);
		observer.observe(host, { subtree: true, childList: true, attributes: true });
		schedule();
		return () => {
			observer.disconnect();
			if (frame != null) window.cancelAnimationFrame(frame);
		};
	}, [artifact.revision, artifact.visualId, bindingsDigest, contract]);

	const openReference = async (event: MouseEvent<HTMLDivElement>) => {
		const target = event.target instanceof Element ? event.target.closest<HTMLElement>("[data-reference-kind]") : null;
		if (!target || target.dataset.referenceKind !== "trace" || !target.dataset.referenceValue) return;
		event.preventDefault();
		event.stopPropagation();
		if (target.getAttribute("aria-busy") === "true") return;
		target.setAttribute("aria-busy", "true");
		try {
			const visual = await openTraceReference(target.dataset.referenceValue, target.dataset.referenceContainerId);
			window.dispatchEvent(new CustomEvent(VISUAL_REFERENCE_OPENED_EVENT, { detail: visual }));
		} catch (reason) {
			window.dispatchEvent(new CustomEvent(VISUAL_REFERENCE_ERROR_EVENT, { detail: publicError(reason) }));
		} finally {
			target.removeAttribute("aria-busy");
		}
	};

	return <div ref={root} data-visual-observation-contract={contract?.schemaVersion} onClick={openReference}>{children}</div>;
}

/** A failed pane still has to be diagnosable. The sentence goes on top; the
 * stable code, the trace identity, and the remediation go underneath, because
 * "Trace data unavailable" alone sent agents into blind capture retries. */
function VisualInvalidState({ title, detail, code, remediation, traceId, onRetry }: {
	title: string;
	detail: string;
	code?: string;
	remediation?: string;
	traceId?: string;
	onRetry?: () => void;
}) {
	return (
		<div className="visual-invalid" role="alert" data-testid="visual-invalid" data-error-code={code}>
			<strong>{title}</strong>
			<p>{detail}</p>
			{remediation ? <p className="visual-invalid-remediation">{remediation}</p> : null}
			{onRetry ? <button type="button" className="visual-invalid-retry" onClick={onRetry}>Retry</button> : null}
			{code || traceId ? (
				<p className="visual-invalid-identity">
					{code ? <code data-testid="visual-invalid-code">{code}</code> : null}
					{traceId ? <code data-testid="visual-invalid-trace">{traceId}</code> : null}
				</p>
			) : null}
		</div>
	);
}

class VisualErrorBoundary extends Component<
	{ children: ReactNode; visualId?: string; visualRevision?: number | null; templateId?: string | null },
	{ error: Error | null; retry: number }
> {
	state: { error: Error | null; retry: number } = { error: null, retry: 0 };
	static getDerivedStateFromError(error: Error) { return { error }; }
	componentDidUpdate(prevProps: VisualErrorBoundary["props"]) {
		if (prevProps.visualRevision !== this.props.visualRevision && this.state.error) {
			this.setState({ error: null });
		}
	}
	componentDidCatch(error: Error, info: ErrorInfo) {
		// `console.error` reaches a devtools console nobody has open. The
		// structured record is what the agent can actually query, so the
		// boundary emits both.
		console.error("Visual shell render failed", error, info.componentStack);
		reportDiagnostic({
			severity: "error",
			component: "visual-host",
			event: "visual.render.failed",
			code: DIAGNOSTIC_CODES.visualRenderFailed,
			message: error.message,
			visualId: this.props.visualId ?? null,
			visualRevision: this.props.visualRevision ?? null,
			details: {
				templateId: this.props.templateId ?? null,
				componentStack: info.componentStack?.slice(0, 1_000) ?? null,
			},
		});
	}
	render() {
		if (this.state.error) {
			const presented = toPublicError(this.state.error, "Visual failed to render");
			return <VisualInvalidState
				title="Visual failed to render"
				detail={presented.message}
				code={presented.code}
				remediation={presented.remediation}
				onRetry={() => this.setState((current) => ({ error: null, retry: current.retry + 1 }))}
			/>;
		}
		return <div className="visual-host-boundary" key={this.state.retry}>{this.props.children}</div>;
	}
}

/** Shared host used by chat cards, the right pane, and the Visuals library. */
export function VisualHost({ artifact }: { artifact: ArtifactRef }) {
	const bindingsKey = bindingAuthorityKey(artifact.bindings);
	const isSystemsDynamic =
		artifact.templateId === "diagram.systems.dynamic.v1" || artifact.rendererKind === "systems-dynamic";
	if (isSystemsDynamic) {
		return <VisualErrorBoundary key={`${artifact.id}:systems-dynamic`} visualId={artifact.visualId ?? artifact.id} visualRevision={typeof artifact.revision === "number" ? artifact.revision : null} templateId={artifact.templateId ?? null}><SystemsDynamicVisual artifact={artifact} /></VisualErrorBoundary>;
	}
	const isSystems = artifact.templateId === "diagram.systems.v1" || artifact.rendererKind === "systems";
	if (isSystems) {
		return <VisualErrorBoundary key={`${artifact.id}:systems`} visualId={artifact.visualId ?? artifact.id} visualRevision={typeof artifact.revision === "number" ? artifact.revision : null} templateId={artifact.templateId ?? null}><SystemsMapVisual artifact={artifact} /></VisualErrorBoundary>;
	}
	const isChart = artifact.templateId === "analysis.chart.v1" || artifact.rendererKind === "chart";
	if (isChart) {
		return <VisualErrorBoundary key={`${artifact.id}:chart`} visualId={artifact.visualId ?? artifact.id} visualRevision={typeof artifact.revision === "number" ? artifact.revision : null} templateId={artifact.templateId ?? null}><ChartVisual artifact={artifact} /></VisualErrorBoundary>;
	}
	const isMermaid =
		artifact.templateId === "diagram.mermaid.v1" || artifact.rendererKind === "mermaid";
	if (isMermaid) {
		return (
			<VisualErrorBoundary key={`${artifact.id}:mermaid`} visualId={artifact.visualId ?? artifact.id} visualRevision={typeof artifact.revision === "number" ? artifact.revision : null} templateId={artifact.templateId ?? null}>
				<MermaidVisual artifact={artifact} />
			</VisualErrorBoundary>
		);
	}
	if (artifact.templateId === "synth.subagents.v1") {
		return (
			<VisualErrorBoundary key={`${artifact.id}:${artifact.templateId ?? "subagents"}`} visualId={artifact.visualId ?? artifact.id} visualRevision={typeof artifact.revision === "number" ? artifact.revision : null} templateId={artifact.templateId ?? null}>
				<SubagentsVisual artifact={artifact} />
			</VisualErrorBoundary>
		);
	}
	if (artifact.preview?.variant && artifact.preview.variant !== "generic" && !artifact.templateId) {
		return (
			<VisualErrorBoundary key={`${artifact.id}:preview`} visualId={artifact.visualId ?? artifact.id} visualRevision={typeof artifact.revision === "number" ? artifact.revision : null} templateId={artifact.templateId ?? null}>
				<MockFallback artifact={artifact} />
			</VisualErrorBoundary>
		);
	}
	return (
		<VisualErrorBoundary key={`${artifact.id}:${artifact.templateId ?? "missing"}:${bindingsKey}`} visualId={artifact.visualId ?? artifact.id} visualRevision={typeof artifact.revision === "number" ? artifact.revision : null} templateId={artifact.templateId ?? null}>
			<VisualObservationBoundary artifact={artifact}>
				<TemplateVisualHost artifact={artifact} />
			</VisualObservationBoundary>
		</VisualErrorBoundary>
	);
}

const SHARED_URL_INVALID = "Enter an http(s) private artifact URL.";

function isSharedArtifactUrl(value: string): boolean {
	const trimmed = value.trim();
	if (!trimmed) return false;
	try {
		const parsed = new URL(trimmed);
		return parsed.protocol === "http:" || parsed.protocol === "https:";
	} catch {
		return false;
	}
}

function restoreFocusAfterVisualPaneClose() {
	const grid = document.querySelector<HTMLElement>('[data-testid="visuals-grid"]');
	const next =
		(grid && !grid.hidden ? grid : null)
		?? document.querySelector<HTMLElement>("main.main-pane")
		?? document.querySelector<HTMLElement>("main");
	if (!next) return;
	if (next.tabIndex < 0) next.tabIndex = -1;
	next.focus();
}

function productOwnedPrimaryOptimizerRunId(artifact: ArtifactRef): string | null {
	const runId = optimizerRunIdFromBindings(artifact.bindings);
	if (!runId || artifact.metadata?.optimizerRunId !== runId) return null;
	const role = typeof artifact.metadata?.optimizerVisualRole === "string"
		? artifact.metadata.optimizerVisualRole
		: null;
	const semantics = typeof artifact.metadata?.semantics === "string"
		? artifact.metadata.semantics
		: null;
	if (role === "trace_workbench" || semantics === "baseline_eval_trace") return null;
	return role === "primary" || semantics === "baseline_eval" || typeof artifact.metadata?.algorithmId === "string"
		? runId
		: null;
}

type OptimizerSealGate = { ready: boolean; reason: string | null };

function optimizerSealGateFromPane(host: HTMLElement): OptimizerSealGate {
	const shell = host.querySelector<HTMLElement>('[data-testid="visual-template-shell"]');
	const evidence = host.querySelector<HTMLElement>("[data-run-evidence-state]");
	if (!shell || !evidence) {
		return { ready: false, reason: "Seal available after run evidence finishes loading." };
	}
	const state = evidence.dataset.runEvidenceState;
	if (state === "rejected") {
		const sealed = Number(evidence.dataset.runSealedTraces ?? 0);
		return {
			ready: false,
			reason: `Seal unavailable — run failed with ${Number.isFinite(sealed) ? sealed : 0} sealed traces (evidence rejected).`
		};
	}
	if (shell.dataset.visualTerminal !== "true") {
		return { ready: false, reason: "Seal available after the optimizer run finishes." };
	}
	if (state !== "accepted") {
		return { ready: false, reason: `Seal unavailable — run evidence is ${state ?? "still loading"}, not complete.` };
	}
	return { ready: true, reason: null };
}

export function VisualPane({ artifact, onClose }: { artifact: ArtifactRef; onClose: () => void }) {
	const paneRef = useRef<HTMLElement>(null);
	const overflowRef = useRef<HTMLDivElement>(null);
	const moreButtonRef = useRef<HTMLButtonElement>(null);
	const primaryOptimizerRunId = productOwnedPrimaryOptimizerRunId(artifact);
	const [optimizerSealGate, setOptimizerSealGate] = useState<OptimizerSealGate>(() => ({
		ready: false,
		reason: primaryOptimizerRunId ? "Seal available after run evidence finishes loading." : null
	}));
	const [expanded, setExpanded] = useState(false);
	const [annotations, setAnnotations] = useState<VisualAnnotation[]>([]);
	const [seals, setSeals] = useState<VisualSeal[]>([]);
	const [sealedBundle, setSealedBundle] = useState<VisualSealBundle | null>(null);
	const [compareBundle, setCompareBundle] = useState<VisualSealBundle | null>(null);
	const [shareUpload, setShareUpload] = useState<VisualUpload | null>(null);
	const [sharedUrl, setSharedUrl] = useState("");
	const [labeling, setLabeling] = useState(false);
	const [labelPoint, setLabelPoint] = useState<{ x: number; y: number; selector?: Record<string, unknown>; targetLabel?: string } | null>(null);
	const [labelBody, setLabelBody] = useState("");
	const [artifactError, setArtifactError] = useState<string | null>(null);
	const [artifactActionStatus, setArtifactActionStatus] = useState<string | null>(null);
	const [busy, setBusy] = useState(false);
	const [inspectorOpen, setInspectorOpen] = useState(false);
	const [debugState, setDebugState] = useState<VisualPaneDebugState>({
		connectionState: null,
		transportState: null,
		projectionSource: null,
		stale: false
	});
	function cancelLabeling() {
		setLabeling(false);
		setLabelPoint(null);
		requestAnimationFrame(() => moreButtonRef.current?.focus());
	}

	function readDebugState(): VisualPaneDebugState {
		const shell = paneRef.current?.querySelector<HTMLElement>('[data-testid="visual-template-shell"]');
		return {
			connectionState: shell?.dataset.connectionState ?? null,
			transportState: shell?.dataset.visualTransportState ?? null,
			projectionSource: shell?.dataset.visualProjectionSource ?? null,
			stale: shell?.dataset.visualProjectionStale === "true"
		};
	}

	function closeInspector(restoreFocus = true) {
		setInspectorOpen(false);
		if (restoreFocus) requestAnimationFrame(() => moreButtonRef.current?.focus());
	}

	function toggleInspector() {
		setInspectorOpen((open) => {
			if (!open) setDebugState(readDebugState());
			return !open;
		});
	}

	useEffect(() => {
		if (!inspectorOpen) return;
		const closeOnPointerDown = (event: PointerEvent) => {
			if (!overflowRef.current?.contains(event.target as Node)) closeInspector(false);
		};
		document.addEventListener("pointerdown", closeOnPointerDown);
		return () => document.removeEventListener("pointerdown", closeOnPointerDown);
	}, [inspectorOpen]);

	useEffect(() => {
		if (!labeling && !inspectorOpen && !expanded) return;
		const onKeyDown = (event: KeyboardEvent) => {
			if (event.key !== "Escape") return;
			// Escape hierarchy: labeling, inspector, then expanded; pane close stays in the controller.
			if (labeling) {
				event.preventDefault();
				event.stopPropagation();
				cancelLabeling();
				return;
			}
			if (inspectorOpen) {
				event.preventDefault();
				event.stopPropagation();
				closeInspector();
				return;
			}
			if (expanded) {
				event.preventDefault();
				event.stopPropagation();
				setExpanded(false);
			}
		};
		window.addEventListener("keydown", onKeyDown, true);
		return () => window.removeEventListener("keydown", onKeyDown, true);
	}, [labeling, inspectorOpen, expanded]);
	useEffect(() => {
		const root = document.documentElement;
		root.classList.toggle("visual-expanded", expanded);
		return () => root.classList.remove("visual-expanded");
	}, [expanded]);
	const visualId = artifact.visualId;
	const revision = artifact.revision;
	const qualityGate = artifact.metadata?.qualityGate as { ready?: boolean; revision?: number } | undefined;
	const authoringGateReady = Boolean(qualityGate?.ready && qualityGate.revision === revision);
	const sealEligible = Boolean(visualId && revision && (
		primaryOptimizerRunId ? optimizerSealGate.ready : authoringGateReady
	));
	const sealDisabledReason = primaryOptimizerRunId
		? optimizerSealGate.reason
		: authoringGateReady
			? null
			: "Seal requires the E1 visual quality gate for this exact revision.";

	useEffect(() => {
		if (!primaryOptimizerRunId) {
			setOptimizerSealGate({ ready: false, reason: null });
			return;
		}
		const host = paneRef.current;
		if (!host) return;
		const read = () => setOptimizerSealGate(optimizerSealGateFromPane(host));
		const observer = new MutationObserver(read);
		observer.observe(host, { subtree: true, childList: true, attributes: true, attributeFilter: ["data-run-evidence-state", "data-run-sealed-traces", "data-visual-terminal"] });
		read();
		return () => observer.disconnect();
	}, [artifact.revision, primaryOptimizerRunId]);

	useEffect(() => {
		let cancelled = false;
		if (!visualId || !bridges.visuals) return;
		void Promise.all([bridges.visuals.annotations(visualId), bridges.visuals.listSeals(visualId)])
			.then(([nextAnnotations, nextSeals]) => {
				if (!cancelled) {
					setAnnotations(nextAnnotations.filter((row) => !row.tombstoned));
					setSeals(nextSeals);
				}
			})
			.catch((reason) => { if (!cancelled) setArtifactError(publicError(reason)); });
		return () => { cancelled = true; };
	}, [visualId, revision]);

	async function createLabel() {
		if (!visualId || !revision || !labelPoint || !bridges.visuals) return;
		setBusy(true);
		setArtifactError(null);
		try {
			const annotation = await bridges.visuals.createAnnotation(visualId, {
				visualRevision: revision,
				sourceDigest: artifact.contentDigest ?? null,
				selector: labelPoint.selector ?? { type: "chart_mark", markId: "visual-pane", x: labelPoint.x, y: labelPoint.y },
				kind: "note",
				body: labelBody.trim() || null,
				metadata: {
					coordinateSpace: "normalized",
					createdFrom: "visual-pane",
					...(labelPoint.targetLabel ? { semanticTarget: labelPoint.targetLabel } : {})
				}
			});
			setAnnotations((current) => [...current, annotation]);
			setLabeling(false);
			setLabelPoint(null);
			setLabelBody("");
		} catch (reason) {
			setArtifactError(publicError(reason));
		} finally {
			setBusy(false);
		}
	}

	async function sealCurrentRevision() {
		if (!visualId || !revision || !bridges.visuals) return;
		setBusy(true);
		setArtifactError(null);
		try {
			const nextSeal = await bridges.visuals.seal(visualId, revision);
			setSeals((current) => [nextSeal, ...current.filter((row) => row.receiptDigest !== nextSeal.receiptDigest)]);
			setSealedBundle(await bridges.visuals.getSeal(nextSeal.receiptDigest));
		} catch (reason) {
			setArtifactError(publicError(reason));
		} finally {
			setBusy(false);
		}
	}

	async function rerenderWithCurrentTemplate() {
		if (!visualId || !bridges.visuals) return;
		setBusy(true);
		setArtifactError(null);
		setArtifactActionStatus(null);
		try {
			const updated = await bridges.visuals.update(visualId, {
				metadata: {
					...(artifact.metadata ?? {}),
					templateRerender: {
						requestedAt: new Date().toISOString(),
						fromRevision: revision ?? null,
						templateId: artifact.templateId ?? null
					}
				},
				bumpRevision: true
			});
			setArtifactActionStatus(`Rendered revision ${updated.currentRevision} with the current ${updated.templateId} template.`);
		} catch (reason) {
			setArtifactError(publicError(reason, "Could not re-render this visual."));
		} finally {
			setBusy(false);
		}
	}

	async function restartEvaluator() {
		if (!primaryOptimizerRunId || !bridges.optimizers || !bridges.inventory) return;
		const sessionId = artifact.sessionId ?? artifact.ownerSessionId;
		if (!sessionId) {
			setArtifactError("Evaluator restart requires the visual's owning Workshop session.");
			return;
		}
		setBusy(true);
		setArtifactError(null);
		setArtifactActionStatus(null);
		try {
			const run = await bridges.optimizers.get(primaryOptimizerRunId);
			if (!isTerminalRunStatus(run.status)) {
				throw new Error(`Finish or cancel optimizer run ${run.id} before restarting its evaluator.`);
			}
			const containerId = runFacets(run).containerId;
			if (!containerId) throw new Error(`Optimizer run ${run.id} has no recorded evaluator container.`);
			const container = await bridges.inventory.restartContainer(containerId, sessionId);
			if (container.status !== "ready") {
				throw new Error(`Evaluator ${containerId} restarted but reported ${container.status}.`);
			}
			setArtifactActionStatus(`Evaluator ${container.name} restarted and is ready; durable run evidence was retained.`);
		} catch (reason) {
			setArtifactError(publicError(reason, "Could not safely restart the evaluator."));
		} finally {
			setBusy(false);
		}
	}

	async function reopenSeal(receiptDigest: string) {
		if (!bridges.visuals) return;
		setBusy(true);
		setArtifactError(null);
		try {
			const [bundle, upload] = await Promise.all([
				bridges.visuals.getSeal(receiptDigest),
				bridges.visuals.uploadStatus(receiptDigest)
			]);
			setSealedBundle(bundle);
			setCompareBundle(null);
			setShareUpload(upload);
		} catch (reason) {
			setArtifactError(publicError(reason));
		} finally {
			setBusy(false);
		}
	}

	async function compareSeal(receiptDigest: string) {
		if (!bridges.visuals) return;
		setBusy(true);
		setArtifactError(null);
		try {
			const bundle = await bridges.visuals.getSeal(receiptDigest);
			if (!sealedBundle) setSealedBundle(bundle);
			else setCompareBundle(bundle);
		} catch (reason) {
			setArtifactError(publicError(reason));
		} finally {
			setBusy(false);
		}
	}

	async function openSharedUrl() {
		const url = sharedUrl.trim();
		if (!isSharedArtifactUrl(url)) return;
		if (!bridges.visuals) return;
		setBusy(true);
		setArtifactError(null);
		try {
			const bundle = await bridges.visuals.openShared(url);
			setSealedBundle(bundle);
			setCompareBundle(null);
			setShareUpload(null);
		} catch (reason) {
			setArtifactError(publicError(reason, "Could not open the shared visual."));
		} finally {
			setBusy(false);
		}
	}

	function closeVisualPane() {
		setInspectorOpen(false);
		onClose();
		requestAnimationFrame(restoreFocusAfterVisualPaneClose);
	}

	async function shareCurrentSeal() {
		if (!sealedBundle || !bridges.visuals) return;
		setBusy(true);
		setArtifactError(null);
		try {
			const upload = await bridges.visuals.shareSeal(sealedBundle.seal.receiptDigest);
			setShareUpload(upload);
			if (upload.committedUrl) await navigator.clipboard?.writeText(upload.committedUrl).catch(() => undefined);
		} catch (reason) {
			setArtifactError(publicError(reason));
		} finally {
			setBusy(false);
		}
	}
	const isSubagents = artifact.templateId === "synth.subagents.v1";
	const sharedUrlValid = isSharedArtifactUrl(sharedUrl);
	const sharedUrlError = sharedUrl.trim() && !sharedUrlValid ? SHARED_URL_INVALID : null;
	const revisionSync = artifact.metadata?.revisionSync as {
		error?: string | null;
	} | undefined;
	const paneAlert = artifactError ?? (revisionSync?.error ? `Visual refresh failed · ${revisionSync.error}` : null);
	return (
		<aside
			ref={paneRef}
			className={`visual-pane${expanded ? " visual-pane-expanded" : ""}`}
			data-testid="visual-pane"
			aria-label={isSubagents ? "Subagents" : "Visual artifact"}
		>
			<VisualPaneChrome
				artifact={artifact}
				expanded={expanded}
				inspectorOpen={inspectorOpen}
				overflowRef={overflowRef}
				moreButtonRef={moreButtonRef}
				busy={busy}
				artifactOperationsEnabled={!isSubagents}
				evaluatorRestartAvailable={!isSubagents && Boolean(primaryOptimizerRunId)}
				actionStatus={artifactActionStatus}
				annotationsCount={annotations.length}
				sealEligible={!isSubagents && sealEligible}
				sealDisabledReason={isSubagents ? null : sealDisabledReason}
				seals={isSubagents ? [] : seals}
				sealedBundle={isSubagents ? null : sealedBundle}
				compareBundle={isSubagents ? null : compareBundle}
				shareUpload={isSubagents ? null : shareUpload}
				sharedUrl={sharedUrl}
				sharedUrlValid={!isSubagents && sharedUrlValid}
				sharedUrlError={isSubagents ? null : sharedUrlError}
				debugState={debugState}
				onToggleInspector={toggleInspector}
				onBeginLabeling={() => { closeInspector(false); setLabeling(true); setLabelPoint(null); }}
				onRerender={() => void rerenderWithCurrentTemplate()}
				onRestartEvaluator={() => void restartEvaluator()}
				onSeal={() => void sealCurrentRevision()}
				onLiveRevision={() => { setSealedBundle(null); setCompareBundle(null); setShareUpload(null); }}
				onCloseComparison={() => setCompareBundle(null)}
				onShare={() => void shareCurrentSeal()}
				onReopenSeal={(receiptDigest) => void reopenSeal(receiptDigest)}
				onCompareSeal={(receiptDigest) => void compareSeal(receiptDigest)}
				onSharedUrlChange={setSharedUrl}
				onOpenShared={() => void openSharedUrl()}
				onCopySharedUrl={() => void navigator.clipboard?.writeText(shareUpload?.committedUrl ?? "")}
				onToggleExpanded={() => { closeInspector(false); setExpanded((current) => !current); }}
				onClose={closeVisualPane}
			/>
			{paneAlert ? <div className="visual-artifact-error" role="alert">{paneAlert}</div> : null}
			{labeling ? (
				<form className="visual-label-form visual-label-form-stack" onSubmit={(event) => { event.preventDefault(); void createLabel(); }}>
					<span className="visual-label-status">{labelPoint ? (labelPoint.targetLabel ? `Attached to ${labelPoint.targetLabel}` : `Placed at ${Math.round(labelPoint.x * 100)}%, ${Math.round(labelPoint.y * 100)}%`) : "Click the visual to place the label."}</span>
					<input value={labelBody} onChange={(event) => setLabelBody(event.target.value)} placeholder="Label note (optional)" aria-label="Label note" />
					<div className="visual-label-actions">
						<button type="submit" disabled={!labelPoint || busy}>Save label</button>
						<button type="button" onClick={() => cancelLabeling()}>Cancel</button>
					</div>
				</form>
			) : null}
			<div
				className={`visual-pane-body${labeling ? " visual-label-target" : ""}`}
				onClick={labeling ? (event) => {
					const bounds = event.currentTarget.getBoundingClientRect();
					const semantic = event.target instanceof Element
						? event.target.closest<HTMLElement>("[data-annotation-kind][data-annotation-id]")
						: null;
					const kind = semantic?.dataset.annotationKind;
					const id = semantic?.dataset.annotationId;
					const selector = id && kind === "candidate"
						? { type: "candidate", candidateId: id }
						: id && kind === "evaluation"
							? { type: "trial", trialId: id }
							: id && kind === "trace_item"
								? { type: "span", spanId: id }
								: undefined;
					setLabelPoint({
						x: Math.max(0, Math.min(1, (event.clientX - bounds.left) / bounds.width)),
						y: Math.max(0, Math.min(1, (event.clientY - bounds.top) / bounds.height)),
						...(selector ? { selector, targetLabel: `${kind?.replaceAll("_", " ")} ${id}` } : {})
					});
				} : undefined}
			>
				{labelPoint ? (
					<span
						className="visual-label-pin"
						data-testid="visual-label-pin"
						style={{ left: `${labelPoint.x * 100}%`, top: `${labelPoint.y * 100}%` }}
						aria-hidden="true"
					/>
				) : null}
				{sealedBundle ? (
					<div className={compareBundle ? "visual-sealed-compare" : "visual-sealed-single"}>
						<iframe
							className="visual-sealed-frame"
							title={`Sealed ${artifact.title} revision ${sealedBundle.seal.visualRevision}`}
							sandbox=""
							srcDoc={sealedBundle.indexHtml}
							data-receipt-digest={sealedBundle.seal.receiptDigest}
						/>
						{compareBundle ? (
							<iframe
								className="visual-sealed-frame"
								title={`Sealed ${artifact.title} revision ${compareBundle.seal.visualRevision}`}
								sandbox=""
								srcDoc={compareBundle.indexHtml}
								data-receipt-digest={compareBundle.seal.receiptDigest}
							/>
						) : null}
					</div>
				) : <VisualHost artifact={artifact} />}
			</div>
		</aside>
	);
}
