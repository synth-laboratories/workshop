import {useEffect,useMemo,useRef,useState} from "react";
import {FRAME_CAPTURE_RUNTIME,useFramePixelCapture,useFrameSession,useVisualEvidence,useVisualSessionClient,useVisualSessionSnapshot} from "@synth/visuals-react";
import {retainVisualRead} from "@synth/visuals-sdk";

type ManagedFrameRef={seed:number;frameSequence:number;contentType:string};
export interface ManagedVisualMediaPorts {
 framesLatest(runId:string,afterFrameSequence?:number):Promise<{frameCursor:number;frames:ManagedFrameRef[]}>;
 framesList(runId:string,seed:number,beforeFrameSequence?:number,limit?:number):Promise<ManagedFrameRef[]>;
 frameContent(runId:string,seed:number,frameSequence:number):Promise<{frame:ManagedFrameRef;base64:string}>;
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
    if (event.source !== parent) return;
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
	const runtime = `data:text/javascript;charset=utf-8,${encodeURIComponent(FRAME_CAPTURE_RUNTIME+'\n'+MANAGED_HTML_RUNTIME).replaceAll('"','%22')}`;
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

export function ManagedHtmlFrame({ source, payload, title, media, formatError }: { source: string; payload: unknown; title?: string; media?: ManagedVisualMediaPorts | null; formatError:(reason:unknown,fallback:string)=>string }) {
	const client=useVisualSessionClient(),session=useVisualSessionSnapshot();
	const replay=Boolean(session?.state.replay);
	const preparedPayload=useMemo(()=>managedHtmlPayload(payload),[payload]);
	const retainedPayload=useVisualEvidence("managed-payload",preparedPayload);
	const frame = useRef<HTMLIFrameElement>(null);
	const [loaded, setLoaded] = useState(false);
	const [runtimeReady,setRuntimeReady]=useState(false);
  useFrameSession(frame,loaded&&runtimeReady);
  useFramePixelCapture(frame,loaded,source);
	const [runtimeError, setRuntimeError] = useState<string | null>(null);
	const [frameStreamError, setFrameStreamError] = useState<string | null>(null);
	const [nativeFrameCount, setNativeFrameCount] = useState(0);
	const [mediaPending,setMediaPending]=useState(false);
	const admittedPayload = useMemo(() => managedHtmlPayload(retainedPayload.value), [retainedPayload.value]);
	const optimizerRunId = admittedPayload && typeof admittedPayload === "object" && !Array.isArray(admittedPayload)
		? (((admittedPayload as Record<string, unknown>).run as Record<string, unknown> | undefined)?.id as string | undefined)
		: undefined;
	useEffect(() => {
    let cancelled=false;
		const onMessage = (event: MessageEvent) => {
			if (event.source !== frame.current?.contentWindow) return;
			const data = event.data as { type?: unknown; message?: unknown } | null;
			if(data?.type === "synth.visual.managed.ready")setRuntimeReady(true);
			if (data?.type === "synth.visual.managed.error") {
				setRuntimeError(typeof data.message === "string" ? data.message : "Managed renderer failed");
			}
			if (
				data?.type === "synth.visual.managed.frame-request.v1"
				&& optimizerRunId
				&& media?.frameContent
			) {
				const request = data as { seed?: unknown; frameSequence?: unknown };
				const seed = Number(request.seed);
				const frameSequence = Number(request.frameSequence);
				if (!Number.isSafeInteger(seed) || !Number.isSafeInteger(frameSequence)) return;
				void media.frameContent(optimizerRunId, seed, frameSequence).then((content) => {
					if(cancelled)return;
					if (frame.current?.contentWindow) {
						postManagedFrame(frame.current.contentWindow, content, "history");
						setFrameStreamError(null);
					}
				}).catch((reason) => {if(!cancelled)setFrameStreamError(formatError(reason, "Historical frame load failed"));});
			}
		};
		addEventListener("message", onMessage);
		return () => {cancelled=true;removeEventListener("message", onMessage);};
	}, [optimizerRunId,media,formatError]);
	useEffect(() => {
		if (!loaded || !retainedPayload.ready || !frame.current?.contentWindow) return;
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
	}, [admittedPayload, loaded, source,retainedPayload.ready]);
	useEffect(() => {
		setMediaPending(false);
		if (!loaded || !optimizerRunId || !media?.framesLatest || !media.frameContent) return;
		let cancelled = false;
		let frameCursor = 0;
		let timer: ReturnType<typeof globalThis.setTimeout> | null = null;
		let polling = false;
		const latestBySeed = new Map<number, number>();
		const retainedBySeed = new Map<number, ManagedFrameRef>();
		const historyLoaded = new Set<number>();
		const poll = async () => {
			if (cancelled || polling) return;
			polling = true;
			setMediaPending(true);
			try {
				const delta = await retainVisualRead(client,"managed.frames",[optimizerRunId],async()=>{
					const next=await media!.framesLatest(optimizerRunId, frameCursor);
					for(const ref of next.frames)retainedBySeed.set(ref.seed,ref);
					return {frameCursor:next.frameCursor,frames:[...retainedBySeed.values()]};
				});
				const nextCursor = Math.max(frameCursor, delta.frameCursor);
				// Deliberately sequential: at most one decoded base64 frame is live in
				// the host while ten rollout thumbnails advance together.
				for (const next of delta.frames) {
					if (cancelled) return;
					if ((latestBySeed.get(next.seed) ?? -1) >= next.frameSequence) continue;
					const content = await media!.frameContent(optimizerRunId, next.seed, next.frameSequence);
					if (cancelled || !frame.current?.contentWindow) return;
					latestBySeed.set(next.seed, next.frameSequence);
					postManagedFrame(frame.current.contentWindow, content, "live");
					if (!historyLoaded.has(next.seed)) {
						historyLoaded.add(next.seed);
						void retainVisualRead(client,"managed.history",[optimizerRunId,next.seed,200],()=>media!.framesList(optimizerRunId, next.seed, undefined, 200)).then((frames) => {
							if(cancelled)return;
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
				if (!cancelled) setFrameStreamError(formatError(reason, "Native frame stream failed"));
			} finally {
				polling = false;
				if(!cancelled)setMediaPending(false);
				if (!cancelled&&!replay) timer = globalThis.setTimeout(poll, 750);
			}
		};
		void poll();
		return () => {
			cancelled = true;
			if (timer != null) globalThis.clearTimeout(timer);
		};
	}, [loaded, optimizerRunId,media,formatError,client,replay]);
	if(retainedPayload.error)return <p role="alert" data-visual-capture-blocked>Managed source unavailable: {retainedPayload.error}</p>;
	if (runtimeError) return <p role="alert" data-testid="visual-managed-html-error">Managed visual failed: {runtimeError}</p>;
	return <iframe
		data-visual-capture-blocked={!retainedPayload.ready||mediaPending||Boolean(frameStreamError)||undefined}
		ref={frame}
		title={`${title ?? "Managed visual"} · ${nativeFrameCount} native frames${frameStreamError ? " · media retrying" : ""}`}
		data-testid="visual-managed-html"
		sandbox="allow-scripts"
		srcDoc={managedRuntimeDocument()}
		onLoad={() => setLoaded(true)}
		style={{ border: 0, display: "block", height: "100%", minHeight: 420, width: "100%" }}
	/>;
}
