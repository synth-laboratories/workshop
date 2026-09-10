import { Component, useEffect, useMemo, useRef, useState, type ComponentType, type ErrorInfo, type MouseEvent, type ReactNode } from "react";
import { ReactVisualRendererRegistry, useVisualSessionClient, useVisualSessionSnapshot } from "@synth/visuals-react";
import { useTemplateEvidence } from "../visuals/useTemplateEvidence";
import { ManagedHtmlFrame } from "@synth/workshop-visuals/components/ManagedHtmlFrame.tsx";
export { managedHtmlPayload } from "@synth/workshop-visuals/components/ManagedHtmlFrame.tsx";
import { WorkshopVisualSession } from "../visuals/WorkshopVisualSession";
import { WorkshopSubagents } from "@synth/workshop-visuals/subagents";
import { resolveBoundVisual } from "@synth/workshop-visuals/runtime/bindingProjection.ts";
import { observeOptimizerVisual } from "@synth/workshop-visuals/runtime/optimizerOrchestration.ts";
import {retainWorkshopPorts} from "@synth/workshop-visuals/runtime/retainedPorts.ts";
import { resolveSealedTrialProjections, resolveComparisonProjection } from "@synth/workshop-visuals/runtime/relatedProjections.ts";
import type { ArtifactRef } from "../types/landing";
import type { VisualRecord } from "@synth/runtime-protocol";
import {
	anonymousDataProp,
	bindingInputName,
	consumeInjectedRendererCrash,
	createMediaClient,
	createReplayClient,
	isVisualBindings,
	rememberLastKnownGood,
	propsFromBindings,
	replayStreamsFromBindings,
	resolveTemplate,
	registerRuntimeTemplate,
	visualExtensions,
	resolveVisualBindings,
	selectObservationSurface,
	selectRenderedProjection,
	compileSourcedModule,
	isSourcedTemplate,
	sourcedInvalidShell
} from "@synth/visuals";
import { publicError, toPublicError, type PublicError } from "../runtime/publicError";
import type { VisualAnnotation, VisualSeal, VisualSealBundle, VisualUpload } from "../bridge";
import { loadVisualShell } from "../runtime/visualsLoader";
import { loadPackagedFixture } from "../visuals/packagedFixtures";
import { bridges } from "../runtime/desktopBridge";
import { subscribeToRun, type RunProgressSnapshot } from "../runtime/runProgress/subscription";
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
import { DocumentPane, isDocumentArtifact } from "../documents/DocumentPane";
import { SystemsMapVisual } from "./SystemsMapVisual";
import { ChartVisual } from "./ChartVisual";
import { SystemsDynamicVisual } from "./SystemsDynamicVisual";
import { conversationThreadEvents, eventsToMessages, eventsToLocalActivity, type SubagentState } from "../runtime/sessionView";
import { useEventsBySession } from "../stores/sessionStore";
import { ChatTranscript } from "./ChatTranscript";
import { bindingAuthorityKey } from "../runtime/visualRevisionState";
import { openTraceReference, VISUAL_REFERENCE_ERROR_EVENT, VISUAL_REFERENCE_OPENED_EVENT } from "../runtime/visualReferences";
import {
	previewVariantForTemplate,
	SEALED_TRACE_WORKBENCH_TEMPLATES,
	runProgressEvidenceMode
} from "../runtime/templatePresentation";
import { optimizerRunIdFromBindings } from "../runtime/visualBindings";
import { projectVisualRunLifecycle } from "../runtime/visualRunLifecycle";
import { isTerminalRunStatus } from "../runtime/runProgress/types";
import { runFacets } from "./optimizers/runPresentation";
import { VisualPaneChrome, type VisualPaneDebugState } from "./VisualPaneChrome";

import { traceResearchClient } from "../runtime/traceResearch";

type ShellProps = {
	title?: string;
	lede?: string;
	bindings?: Record<string, unknown>;
	[key: string]: unknown;
};

const TEMPLATE_PORTS=new Set(["replay","media","traceResearch","onReviewFinding","evidence","history","collections","visualState"]);
function PinnedTemplate({Shell,...props}:{Shell:ComponentType<ShellProps>}&ShellProps){
	const client=useVisualSessionClient();
	const replay=Boolean(useVisualSessionSnapshot()?.state.replay);
	const ports=useMemo(()=>retainWorkshopPorts(client,Object.fromEntries(Object.entries(props).filter(([key])=>TEMPLATE_PORTS.has(key)))),[client,replay,props.replay,props.media,props.traceResearch,props.onReviewFinding,props.evidence,props.history,props.collections,props.visualState]);
	const values:ShellProps={},aliases:Record<string,string>={},seen=new Map<object,string>();
	for(const [key,value] of Object.entries(props)){
		if(TEMPLATE_PORTS.has(key))continue;
		const prior=value&&typeof value==="object"?seen.get(value):undefined;
		if(prior){aliases[key]=prior;continue;}
		values[key]=value;
		if(value&&typeof value==="object")seen.set(value,key);
	}
	const data=JSON.parse(JSON.stringify({values,aliases})) as {values:ShellProps;aliases:Record<string,string>};
	const cut=useTemplateEvidence(data);
	if(!cut.ready||!cut.value)return <div data-visual-capture-blocked="true" role={cut.error?"alert":"status"}>{cut.error??"Restoring pinned visual evidence…"}</div>;
	const restored={...cut.value.values};
	for(const [key,source] of Object.entries(cut.value.aliases))restored[key]=restored[source];
	return <Shell {...restored} {...ports}/>;
}

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

function SubagentConversation({sessionId, agent}: {sessionId?: string; agent: SubagentState}) {
 const sessionEvents = useEventsBySession();
 const childEvents = useMemo(() => conversationThreadEvents(sessionId ? sessionEvents[sessionId] ?? [] : [], agent.id), [sessionEvents, sessionId, agent.id]);
 const messages = useMemo(() => eventsToMessages(childEvents), [childEvents]);
 const activity = useMemo(() => eventsToLocalActivity(childEvents, messages), [childEvents, messages]);
 return <div data-testid="subagent-conversation">
  {agent.summary && !messages.some(message => message.body === agent.summary) ? <p>{agent.summary}</p> : null}
  <span data-testid="subagent-status">{agent.status[0].toUpperCase() + agent.status.slice(1)}</span>
  <ChatTranscript readOnly chat={{id:agent.id,title:agent.title,messages,activityByMessageId:activity,artifacts:[]}} events={childEvents} openArtifactId={null} onOpenArtifact={id => {if(id) void bridges.visuals?.show(id);}} activityMode="detailed" />
 </div>;
}

function SubagentsVisual({artifact}:{artifact:ArtifactRef}) {
  const resolved=propsFromBindings(artifact.bindings);
  const agents=Array.isArray(resolved.props.agents)?resolved.props.agents as SubagentState[]:[];
  const sessionId=typeof resolved.props.sessionId==="string"?resolved.props.sessionId:undefined;
  return <WorkshopSubagents agents={agents} sessionId={sessionId} readThread={bridges.codex?.readThread} formatError={publicError} renderConversation={agent => <SubagentConversation sessionId={sessionId} agent={agent as SubagentState} />}/>;
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
	// Callers build `artifact` inline while rendering, so its `bindings` object is
	// a new identity on every parent render even when the bindings are unchanged.
	// Keyed on that identity, the async resolution effect below aborts its own
	// in-flight read and restarts on every parent render; an AbortError is
	// deliberately silent, so a binding that keeps restarting looks exactly like
	// a pane that was never opened. Key it on the value instead.
	const bindingsSignature = useMemo(
		() => JSON.stringify(artifact.bindings ?? null),
		[artifact.bindings]
	);
	const asyncBindings = useMemo(
		() =>
			resolvedBindings.slots.filter((binding) => {
				if (binding.kind === "trace_v5") return true;
				if (
					binding.kind === "live_sse"
					|| binding.kind === "optimizer_run"
					|| binding.kind === "inline"
				) return false;
				// A stored fixture binding carries a path, not a payload.
				// Treating it as synchronous meant nothing ever loaded it and
				// the pane failed with "has not been resolved by the Rust
				// runtime". It resolves here, from the packaged assets.
				if (binding.kind === "fixture") return binding.data === undefined;
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
	const [analysisFindings, setAnalysisFindings] = useState<unknown[]>([]);
	const [analysisCampaigns, setAnalysisCampaigns] = useState<unknown[]>([]);
	const [userTemplateDigest, setUserTemplateDigest] = useState<string | null>(null);
	const [templateReload, setTemplateReload] = useState(0);
	const [templateCatalogEpoch, setTemplateCatalogEpoch] = useState(0);
	useEffect(() => {
		if (!userTemplateDigest || !artifact.templateId || !bridges.visuals) return;
		let cancelled = false;
		const timer = window.setInterval(() => {
			void bridges.visuals!.getTemplate(artifact.templateId!).then(meta => {
				if (!cancelled && meta.templateDigest !== userTemplateDigest) setTemplateReload(value => value + 1);
			}).catch(reason => { if (!cancelled) setShell(() => sourcedInvalidShell(publicError(reason))); });
		}, 1000);
		return () => { cancelled = true; window.clearInterval(timer); };
	}, [artifact.templateId, userTemplateDigest]);

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
	const inspectorTraceDigest = resolvedBindings.status !== "rejected"
		? resolvedBindings.slots.find((entry) => entry.kind === "trace_v5")?.source
			?? (typeof artifact.metadata?.traceDigest === "string" ? artifact.metadata.traceDigest : undefined)
		: (typeof artifact.metadata?.traceDigest === "string" ? artifact.metadata.traceDigest : undefined);
	const evidenceHeadDigest = resolvedBindings.status !== "rejected"
		? resolvedBindings.slots.find((entry) => entry.kind === "annotation_evidence_head")?.source
		: undefined;
	useEffect(() => {
		if (artifact.templateId !== "trace.rollout_inspector.v1" || !inspectorTraceDigest || !bridges.analysis) {
			setAnalysisFindings([]);
			return;
		}
		let cancelled = false;
		void bridges.analysis.findings(inspectorTraceDigest).then((row) => {
			if (!cancelled) setAnalysisFindings(Array.isArray(row?.findings) ? row.findings : []);
		}).catch(() => {
			if (!cancelled) setAnalysisFindings([]);
		});
		return () => { cancelled = true; };
	}, [artifact.templateId, inspectorTraceDigest]);
	useEffect(() => {
		if (artifact.templateId !== "optimizer.eval.live.v1" || !optimizerRunId || !bridges.analysis) {
			setAnalysisCampaigns([]);
			return;
		}
		let cancelled = false;
		const pull = () => {
			void bridges.analysis!.campaigns(optimizerRunId).then((row) => {
				if (!cancelled) setAnalysisCampaigns(Array.isArray(row?.campaigns) ? row.campaigns : []);
			}).catch(() => {
				if (!cancelled) setAnalysisCampaigns([]);
			});
		};
		pull();
		const timer = globalThis.setInterval(pull, 10_000);
		return () => {
			cancelled = true;
			globalThis.clearInterval(timer);
		};
	}, [artifact.templateId, optimizerRunId]);
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
					if (!cancelled) setShell(() => (props: ShellProps) => <ManagedHtmlFrame source={source} payload={props.data} title={artifact.title} media={bridges.optimizers} formatError={publicError} />);
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
			const nativeTemplate = typeof bridges.visuals?.getTemplate === "function" ? await bridges.visuals.getTemplate(templateId) : null;
			const userAuthored = nativeTemplate?.sourceKind === "user";
			if (cancelled) return;
			if (userAuthored) {
				registerRuntimeTemplate(nativeTemplate);
				setUserTemplateDigest(nativeTemplate.templateDigest ?? null);
				setTemplateCatalogEpoch(value => value + 1);
			} else setUserTemplateDigest(null);
			if (isSourcedTemplate(templateId) || userAuthored) {
				const visualId = artifact.visualId ?? artifact.id;
				let source = "";
				try {
					if (userAuthored) {
						if (!bridges.visuals?.templateShellSource) throw new Error("User-template source requires a native host");
						source = await bridges.visuals.templateShellSource(templateId);
					} else {
						const asset = await bridges.visuals?.content?.(visualId);
						if (asset?.base64) source = decodeBase64Utf8(asset.base64);
					}
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
	}, [artifact.templateId, artifact.visualId, artifact.id, artifact.contentDigest, artifact.revision, visualIdentity, templateReload]);

	useEffect(() => {
		const controller=new AbortController();
		if(asyncBindings.length===0 || !isVisualBindings(artifact.bindings)){
			setTraceResolution({status:"idle",props:{}});return ()=>controller.abort();
		}
		const template=artifact.templateId?resolveTemplate(artifact.templateId):undefined;
		if(!template){setTraceResolution({status:"error",props:{},error:"Visual template is unavailable"});return ()=>controller.abort();}
		setTraceResolution({status:"loading",props:{}});
		void resolveBoundVisual(template,artifact.bindings,{
			loadFixture:loadPackagedFixture,
			traceWindow:digest=>traceResearchClient.request("window",{trace_digest:digest,offset:0,limit:200}),
			traceProjection:bridges.inventory?digest=>bridges.inventory!.resolveTraceProjection(digest,"rollout-inspector"):undefined,
			loadLocalCas:bridges.runtime?source=>bridges.runtime!.request(`/v1/cas/${encodeURIComponent(source)}`):undefined,
			loadQuerySnapshot:source=>traceResearchClient.request("snapshot",{snapshot_id:source}),
			loadRun:bridges.optimizers?source=>bridges.optimizers!.get(source):undefined,
			loadAnnotationEvidenceHead:bridges.analysis?source=>bridges.analysis!.projection("annotation_evidence_head",source):undefined,
			loadVerifierResult:bridges.analysis?source=>bridges.analysis!.projection("verifier_result_v2",source):undefined,
		},controller.signal).then(props=>{
			if(controller.signal.aborted)return;
			setTraceResolution({status:"ready",props});
			setLastKnownGoodProps(current=>rememberLastKnownGood(current,props,false));
		}).catch(reason=>{
			if(controller.signal.aborted)return;
			const failure=toPublicError(reason,"Trace projection resolution failed");
			const message=publicError(reason,"Trace projection resolution failed");
			setTraceResolution({status:"error",props:{},error:message,failure});
			reportDiagnostic({...visualIdentity,severity:"error",component:"visual-host",event:"visual.projection.failed",
				code:message.includes("projection schema")?DIAGNOSTIC_CODES.unsupportedTraceProjectionSchema:DIAGNOSTIC_CODES.visualBindingUnresolved,
				message,details:{templateId:artifact.templateId ?? null}});
		});
		return ()=>controller.abort();
	},[artifact.id,artifact.revision,artifact.templateId,bindingsSignature,asyncBindings.length,visualIdentity,templateCatalogEpoch]);
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
		setOptimizerPayload(null);setOptimizerLoadError(null);setProgressView(null);
		setReceiptVerdict({kind:"unverified",reason:"no_receipt"});
		return observeOptimizerVisual({
			subscribe:(listener:(snapshot:RunProgressSnapshot)=>void)=>subscribeToRun(optimizerRunId,listener,{evidence:runProgressEvidenceMode(artifact.templateId)}),
			project:snapshot=>{
				const projection=projectRunProgress(snapshot,Date.now());
				const agreement=projection?progressAgreement(projection):null;
				const lanes=snapshot.run?splitSnapshotEvents(snapshot.run,snapshot.events):null;
				return {progress:agreement,payload:snapshot.run&&lanes?{
					run:snapshot.run,runViewV2:snapshot.viewV2,runProgress:agreement,
					events:lanes.terminalEvents,enrichmentEvents:lanes.enrichmentEvents,
					terminalCursor:lanes.terminalCursor,enrichmentCursor:lanes.enrichmentCursor,evidenceState:snapshot.evidence
				}:null};
			},
			onFrame:frame=>{setOptimizerPayload(frame.payload);setProgressView(frame.progress);setOptimizerLoadError(frame.error);setConnectionState(frame.connection);},
			onDiagnostic:(kind,snapshot)=>reportDiagnostic({
				...visualIdentity,optimizerRunId,streamId:optimizerRunId,severity:kind==="stale"?"warn":"error",component:"visual-host",
				event:kind==="stale"?"stream.replay.gap":"stream.interrupted",
				code:kind==="stale"?DIAGNOSTIC_CODES.streamReplayGap:DIAGNOSTIC_CODES.streamInterrupted,
				message:kind==="stale"?`Optimizer event history is incomplete at ${snapshot.cursor}`:snapshot.error??"Optimizer stream interrupted",
				retryable:true,details:kind==="stale"?{cursor:snapshot.cursor,gap:snapshot.gap}:undefined,
			}),
			readReceipt:()=>Promise.resolve(bridges.optimizers?.visualRenderReceipt?.(artifact.id,typeof artifact.revision==="number"?artifact.revision:0)),
			verifyReceipt:(receipt,snapshot)=>verifyAgainstReceipt(receipt,{
				optimizerRunId,projectionRevision:snapshot.viewV2!.header.projectionRevision,
				dataDigest:visualDataDigest(snapshot.viewV2!),templateVersion:templateDigest??""
			}),
			onReceipt:verdict=>{
				setReceiptVerdict(verdict);
				if(verdict.kind==="regressed"||verdict.kind==="content_changed")reportDiagnostic({
					...visualIdentity,optimizerRunId,severity:"warn",component:"visual-host",event:"visual.receipt.mismatch",code:DIAGNOSTIC_CODES.streamReplayGap,
					message:verdict.kind==="regressed"
						?`Local evidence is at projection revision ${verdict.localRevision}, behind the ${verdict.renderedRevision} this visual already rendered.`
						:`Projection revision ${verdict.projectionRevision} now carries different content than when this visual rendered.`,
					retryable:true,details:{verdict:verdict.kind,renderedAt:verdict.renderedAt},
				});
			},
			recordReady:async(snapshot,signal)=>{
				if(signal.aborted)return;
				await bridges.optimizers?.recordVisualReady?.({
					visualId:artifact.id,optimizerRunId,templateId:artifact.templateId??"optimizer.run.v1",
					replayedThrough:snapshot.cursor,subscribedFrom:snapshot.cursor+1,templateDigest,
					visualRevision:typeof artifact.revision==="number"?artifact.revision:0,
					projectionRevision:snapshot.viewV2!.header.projectionRevision,dataDigest:visualDataDigest(snapshot.viewV2!)
				});
			}
		});
	}, [artifact.id, artifact.revision, artifact.templateId, optimizerRunId, templateDigest, visualIdentity]);

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
		const controller=new AbortController();
		void resolveSealedTrialProjections(allEvents,
			digest=>bridges.inventory!.resolveTraceProjection(digest,"rollout-inspector"),controller.signal).then((rows) => {
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
		return () => { cancelled = true; controller.abort(); };
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
		const controller=new AbortController();
		setComparisonPayload(null);
		void resolveComparisonProjection(boundRunId,{
			list:()=>bridges.optimizers!.list({algorithmId:"gepa"}),
			view:id=>bridges.optimizers!.runViewV2(id),
		},controller.signal).then(payload=>{if(!cancelled)setComparisonPayload(payload);}).catch(()=>undefined);
		return () => {
			cancelled = true;
			controller.abort();
		};
	}, [boundRunId]);

	if (failed) return <VisualInvalidState title="Template unavailable" detail={`No bundled shell is registered for ${artifact.templateId ?? "this visual"}.`} />;
	if (resolvedBindings.status === "rejected") {
		return <VisualInvalidState title="Visual bindings unreadable" detail={resolvedBindings.error ?? "This visual's bindings could not be read."} />;
	}
	if (synchronouslyResolved.errors.length > 0) return <VisualInvalidState title="Visual data unavailable" detail={synchronouslyResolved.errors.join(" · ")} />;
	// A cached shell can load before the binding effect's first state update.
	// The initial idle state is not an empty resolved payload: passing it to a
	// strict family would crash and permanently unmount the in-flight resolver.
	if (asyncBindings.length>0 && (traceResolution.status === "idle" || traceResolution.status === "loading") && !lastKnownGoodProps) return <p className="visual-loading" role="status">Resolving visual inputs…</p>;
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
			<PinnedTemplate Shell={Shell}
				{...(resolvedProps as ShellProps)}
				title={artifact.title}
				lede={artifact.summary}
				bindings={artifact.bindings}
				visualMetadata={artifact.metadata}
				loadError={optimizerLoadError ?? undefined}
				{...(optimizerPayload ?? {})}
				data={anonymousDataProp(resolvedProps, optimizerPayload)}
				comparison={comparisonPayload ?? undefined}
				replay={replayClient}
				media={mediaClient}
				traceResearch={traceResearchClient}
				sealedTraceProjections={sealedTraceProjections}
				analysisFindings={analysisFindings}
				analysisCampaigns={analysisCampaigns}
				onReviewFinding={
					artifact.templateId === "analysis.annotation_workbench.v1" && bridges.analysis
						? (input: { findingId: string; decision: string; rationale: string; evidenceHeadDigest?: string }) =>
							bridges.analysis!.review({
								findingId: input.findingId,
								decision: input.decision,
								rationale: input.rationale,
								evidenceHeadDigest: input.evidenceHeadDigest ?? evidenceHeadDigest ?? ""
							})
						: undefined
				}
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
				visualState={bridges.visuals ? {
					putSnapshot: (snapshot: import("@synth/visuals-protocol").VisualSnapshot) => bridges.visuals!.putSnapshot(artifact.visualId ?? artifact.id, snapshot),
					putRecording: (recording: import("@synth/visuals-protocol").VisualRecording) => bridges.visuals!.putRecording(artifact.visualId ?? artifact.id, recording),
				} : undefined}
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
  const visualClient=useVisualSessionClient();
  const visualSession=useVisualSessionSnapshot();
  const renderedStateVersion=visualSession?.state.stateVersion;
	const root = useRef<HTMLDivElement>(null);
	const lastPublishedObservation = useRef<string | null>(null);
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
		const visualBridge = bridges.visuals;
		if (!host || !contract || !bindingsDigest || !artifact.visualId || !artifact.revision || !visualBridge) return;
		let frame: number | null = null;
		let fallback: number | null = null;
		lastPublishedObservation.current = null;
		const publish = () => {
			frame = null;
			if (fallback != null) {
				window.clearTimeout(fallback);
				fallback = null;
			}
			// The host wraps every shell in its own transport element, so a bare
			// `[data-visual-transport-state]` query harvested the wrapper rather
			// than the template's published observation — reading `idle` over a
			// declared `terminal`, and zero frames over a surface that has none
			// of the count attributes at all. Prefer the template's own.
			const surface = selectObservationSurface(
				Array.from(host.querySelectorAll("[data-visual-transport-state]"))
			);
			if (!surface) return;
			const rawError = surface.getAttribute("data-visual-error")?.trim();
			const observation = {
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
			} as const;
      const committed=visualClient?.getSnapshot();
      if(committed?.ready&&committed.state.stateVersion===renderedStateVersion){
        visualClient!.publishSceneContribution("rendered",renderedStateVersion,{
          truth:{rolloutCount:{state:"observed",value:observation.rolloutCount},frameCount:{state:"observed",value:observation.renderedFrameCount},semanticEventCount:{state:"observed",value:observation.semanticEventCount},transportState:{state:"observed",value:observation.transportState},bindingsDigest:{state:"observed",value:bindingsDigest}},
          diagnostics:observation.error?[observation.error]:[],
        });
      }
			// Rich visuals can mutate many descendants while their readiness facts
			// stay unchanged. Publishing every mutation feeds the resulting app
			// event back into run invalidation and creates a render/report loop.
			// The timestamp is deliberately excluded: only semantic observation
			// changes deserve another durable receipt.
			const observationKey = JSON.stringify({
				bindingsDigest,
				transportState: observation.transportState,
				rolloutCount: observation.rolloutCount,
				renderedFrameCount: observation.renderedFrameCount,
				semanticEventCount: observation.semanticEventCount,
				terminal: observation.terminal,
				error: observation.error
			});
			if (lastPublishedObservation.current === observationKey) return;
			lastPublishedObservation.current = observationKey;
			void visualBridge.reportObservation(observation);
		};
		const schedule = () => {
			if (frame == null) frame = window.requestAnimationFrame(publish);
			// macOS suspends requestAnimationFrame for an occluded Workshop
			// window. Review capture is intentionally host-driven and must still
			// receive the exact rendered-observation receipt while the app is in
			// the background, so race rAF with one bounded timer.
			if (fallback == null) fallback = window.setTimeout(publish, 250);
		};
		const observer = new MutationObserver(schedule);
		observer.observe(host, { subtree: true, childList: true, attributes: true });
		schedule();
		return () => {
			observer.disconnect();
			if (frame != null) window.cancelAnimationFrame(frame);
			if (fallback != null) window.clearTimeout(fallback);
		};
	}, [artifact.revision, artifact.visualId, bindingsDigest, contract,visualClient,renderedStateVersion]);

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
		return <div className="visual-host-boundary" data-visual-id={this.props.visualId ?? undefined} data-visual-revision={this.props.visualRevision ?? undefined} key={this.state.retry}>{this.props.children}</div>;
	}
}

const visualRenderers = new ReactVisualRendererRegistry<ArtifactRef>()
    .register({ id: "document", matches: isDocumentArtifact, component: DocumentPane })
	.register({ id: "systems-dynamic", matches: (artifact) => artifact.rendererKind === "systems-dynamic", component: SystemsDynamicVisual })
	.register({ id: "systems", matches: (artifact) => artifact.rendererKind === "systems", component: SystemsMapVisual })
	.register({ id: "chart", matches: (artifact) => artifact.rendererKind === "chart", component: ChartVisual })
	.register({ id: "mermaid", matches: (artifact) => artifact.rendererKind === "mermaid", component: MermaidVisual })
	.register({ id: "subagents", matches: (artifact) => artifact.templateId === "synth.subagents.v1", component: SubagentsVisual })
	.register({ id: "preview", matches: (artifact) => Boolean(artifact.preview?.variant && artifact.preview.variant !== "generic" && !artifact.templateId), component: MockFallback })
	.register({ id: "template", matches: () => true, component: TemplateVisualHost, observe: true });

/** Shared host used by chat cards, the right pane, and the Visuals library. */
export function VisualHost({ artifact }: { artifact: ArtifactRef }) {
	const bindingsKey = artifact.templateId === "synth.subagents.v1" ? "live-subagents" : bindingAuthorityKey(artifact.bindings);
	const definition = artifact.templateId ? visualExtensions.definition(artifact.templateId) : undefined;
	const renderer = visualRenderers.resolve({...artifact,rendererKind:artifact.rendererKind??definition?.renderer});
	const Renderer = renderer.component;
	const content = renderer.observe
		? <VisualObservationBoundary artifact={artifact}><Renderer artifact={artifact} /></VisualObservationBoundary>
		: <Renderer artifact={artifact} />;
	return <div data-visual-definition={definition?.id} data-visual-definition-version={definition?.version}><VisualErrorBoundary
		key={`${artifact.id}:${artifact.revision ?? "unversioned"}:${renderer.id}:${artifact.templateId ?? "missing"}:${bindingsKey}`}
		visualId={artifact.visualId ?? artifact.id}
		visualRevision={typeof artifact.revision === "number" ? artifact.revision : null}
		templateId={artifact.templateId ?? null}
	><WorkshopVisualSession artifact={artifact}>{content}</WorkshopVisualSession></VisualErrorBoundary></div>;
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
	const qualityGate = artifact.metadata?.qualityGate as {
		ready?: boolean;
		revision?: number;
		state?: "ready" | "stale";
		staleReasons?: string[];
	} | undefined;
	const authoringGateReady = Boolean(
		qualityGate?.ready
		&& qualityGate.state !== "stale"
		&& qualityGate.revision === revision
	);
	const sealEligible = Boolean(visualId && revision && (
		primaryOptimizerRunId ? optimizerSealGate.ready : authoringGateReady
	));
	const sealDisabledReason = primaryOptimizerRunId
		? optimizerSealGate.reason
		: authoringGateReady
			? null
			: qualityGate?.state === "stale"
				? `Seal requires fresh visual review; certification is stale${qualityGate.staleReasons?.length ? ` (${qualityGate.staleReasons.join(", ")})` : ""}.`
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
