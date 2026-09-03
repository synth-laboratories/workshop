import { useEffect, useMemo, useRef, useState } from "react";
import type { PointerEvent as ReactPointerEvent } from "react";
import type { ArtifactRef } from "../types/landing";
import type { HumanAnnotationSessionView } from "../bridge";
import { bridges } from "../runtime/desktopBridge";
import { publicError } from "../runtime/publicError";
import { blobToBase64, recordingToWhisperWav } from "../runtime/whisperAudio";
import { copyText } from "../runtime/clipboard";
import { VisualPane } from "./VisualHost";
import { MicIcon } from "./MicIcon";
import "./HumanAnnotationWorkspace.css";

type Question = {
	id: string;
	type: string;
	prompt: string;
	helpText?: string;
	required?: boolean;
	options?: Array<{ id: string; label?: string }>;
	validation?: { min?: number; max?: number };
	anchors?: Record<string, string>;
	allowTie?: boolean;
	allowNeither?: boolean;
	allowAbstain?: boolean;
	decisionCriteria?: { evidence?: string; answerRule?: string; requiresHumanJudgment?: boolean };
	answerGuidance?: Record<string, string>;
};

export function HumanAnnotationWorkspace({
	sessionId,
	onClose,
	focused = false,
	onFocusedChange
}: {
	sessionId: string;
	onClose: () => void;
	focused?: boolean;
	onFocusedChange?: (focused: boolean) => void;
}) {
	const api = bridges.humanAnnotations;
	const [view, setView] = useState<HumanAnnotationSessionView | null>(null);
	const [active, setActive] = useState(0);
	const [error, setError] = useState<string | null>(null);
	const [saving, setSaving] = useState(false);
	const [comment, setComment] = useState("");
	const [visual, setVisual] = useState<ArtifactRef | null>(null);
	const [whisperAvailable, setWhisperAvailable] = useState(false);
	const [dictating, setDictating] = useState(false);
	const [dictationState, setDictationState] = useState<string | null>(null);
	const [dictationAttachmentId, setDictationAttachmentId] = useState<string | null>(null);
	const [activeEvidenceIndex,setActiveEvidenceIndex]=useState(0);
	const [selector,setSelector]=useState<Record<string,unknown>>({kind:"visual_target",targetId:"subject"});
	const [campaign,setCampaign]=useState<Record<string,unknown>|null>(null);
	const [campaignNote,setCampaignNote]=useState("");
	const [resultCopyState,setResultCopyState]=useState<"idle"|"copied">("idle");
	const chunkWrites = useRef<Promise<unknown>[]>([]);
	const chunkPayloads = useRef<Array<{ index:number; base64Data:string }>>([]);
	const chunkIndex = useRef(0);
	const attachmentId = useRef<string | null>(null);
	const dictationRecorder = useRef<MediaRecorder | null>(null);
	const dictationChunks = useRef<Blob[]>([]);
	const dictationStream = useRef<MediaStream | null>(null);

	const task = (view?.task ?? {}) as Record<string, unknown>;
	const questions = useMemo(() => (Array.isArray(task.questions) ? task.questions : []) as Question[], [task.questions]);
	const presentedQuestions = useMemo(() => questions.filter((candidate) => isPresented(candidate, view?.answers ?? {})), [questions, view?.answers]);
	const question = presentedQuestions[active];
	const canSubmit = presentedQuestions.every((candidate) => !candidate.required || Boolean(view?.answers?.[candidate.id]));
	const subject = (task.subject ?? {}) as Record<string, unknown>;
	const evidenceItems=useMemo(()=>[subject,...(Array.isArray(task.evidence)?task.evidence as Array<Record<string,unknown>>:[])].filter((item,index,all)=>index===all.findIndex((candidate)=>evidenceKey(candidate)===evidenceKey(item))),[task.evidence,task.subject]);
	const activeEvidence=evidenceItems[activeEvidenceIndex]??subject;
	const campaignId=typeof task.campaignId==="string"?task.campaignId:null;

	async function refresh() {
		if (!api) throw new Error("Human annotations require Synth Desktop");
		setView(await api.open(sessionId));
	}
	useEffect(() => {
		if (dictationRecorder.current) {
			dictationRecorder.current.onstop = null;
			dictationRecorder.current.stop();
			dictationRecorder.current = null;
		}
		dictationStream.current?.getTracks().forEach((track) => track.stop());
		dictationStream.current = null;
		dictationChunks.current = [];
		setActive(0);
		setError(null);
		setComment("");
		setDictating(false);
		setDictationState(null);
		setDictationAttachmentId(null);
		setActiveEvidenceIndex(0);
		setSelector({kind:"visual_target",targetId:"subject"});
		setCampaign(null);
		setCampaignNote("");
		setResultCopyState("idle");
	}, [sessionId]);
	useEffect(() => { void refresh().catch((reason) => setError(publicError(reason))); }, [sessionId]);
	useEffect(() => {
		const firstUnanswered = presentedQuestions.findIndex((candidate) => !view?.answers?.[candidate.id]);
		if (firstUnanswered >= 0) setActive(firstUnanswered);
	}, [view?.taskId, view?.draftRevision, presentedQuestions]);
	useEffect(() => {
		let live = true;
		void bridges.whisper?.listModels().then((models) => {
			if (live) setWhisperAvailable(models.some((model) => model.selected && Boolean(model.path || model.installedBytes)));
		}).catch(() => { if (live) setWhisperAvailable(false); });
		return () => {
			live = false;
			if (dictationRecorder.current) {
				dictationRecorder.current.onstop = null;
				dictationRecorder.current.stop();
			}
			dictationStream.current?.getTracks().forEach((track) => track.stop());
		};
	}, []);
	useEffect(() => {
		const kind=String(activeEvidence.kind??""); const id=typeof activeEvidence.id==="string"?activeEvidence.id:typeof activeEvidence.visualId==="string"?activeEvidence.visualId:null;
		if (!view || !["visual_revision","visual"].includes(kind) || !id || !bridges.visuals) {setVisual(null);return;}
		void bridges.visuals.get(id).then((record) => setVisual({
			id: record.id, visualId: record.id, kind: "react_app", title: record.title,
			displayName: record.displayName ?? undefined, templateId: record.templateId,
			rendererKind: record.rendererKind, revision: record.currentRevision,
			contentDigest: record.contentDigest ?? undefined, bindings: record.bindings as Record<string, unknown>,
			metadata: record.metadata as Record<string, unknown>, sessionId: record.sessionId ?? undefined,
			runId: record.runId ?? undefined, traceId: record.traceId ?? undefined, status: record.status,
			updatedAt: record.updatedAt
		})).catch((reason) => setError(publicError(reason)));
	}, [view?.taskId, activeEvidence.kind, activeEvidence.id, activeEvidence.visualId]);
	useEffect(()=>{setSelector(defaultSelector(activeEvidence));},[activeEvidenceIndex,view?.taskId]);
	useEffect(()=>{if(!api||!campaignId){setCampaign(null);return;}void api.campaignStatus(campaignId).then(setCampaign).catch(()=>setCampaign(null));},[api,campaignId,view?.updatedAt]);

	async function saveAnswer(value: unknown) {
		if (!api || !view || !question) return;
		setSaving(true); setError(null);
		try {
			await api.setAnswer({ sessionId, expectedRevision: view.draftRevision, questionId: question.id, answer: { value } });
			await refresh();
		} catch (reason) { setError(publicError(reason)); } finally { setSaving(false); }
	}
	async function saveComment() {
		if (!api || !view || (!comment.trim() && !dictationAttachmentId)) return;
		setSaving(true); setError(null);
		try {
			await api.createComment({ sessionId, expectedRevision: view.draftRevision, evidenceDigest: String(activeEvidence.digest ?? view.taskDigest), selector, bodyText: comment.trim() || null, audioAttachmentId: dictationAttachmentId });
			setComment(""); setDictationAttachmentId(null); setDictationState(null); await refresh();
		} catch (reason) { setError(publicError(reason)); } finally { setSaving(false); }
	}
	async function finishDictation(mimeType: string) {
		dictationStream.current?.getTracks().forEach((track) => track.stop());
		dictationStream.current = null;
		const chunks = dictationChunks.current;
		dictationChunks.current = [];
		if (!chunks.length) { setDictationState(null); return; }
		setDictationState("Transcribing and saving audio…");
		try {
			await Promise.all(chunkWrites.current);
			if (!api || !attachmentId.current) throw new Error("Audio attachment was not started.");
			const saved = await api.audioFinish({ attachmentId: attachmentId.current, durationMs: null });
			const wav = await recordingToWhisperWav(new Blob(chunks, { type: mimeType }));
			const text = await bridges.whisper?.transcribeAudio?.(await blobToBase64(wav), "audio/wav");
			if (text?.trim()) setComment((current) => current.trim() ? `${current.trim()} ${text.trim()}` : text.trim());
			setDictationAttachmentId(String(saved.attachmentId));
			setDictationState(text?.trim() ? "Dictation and audio ready to save" : "Audio ready to save · no speech detected");
		} catch (reason) {
			setDictationState("Dictation could not be saved · tap the microphone to retry");
			setError(publicError(reason));
		} finally {
			chunkWrites.current = [];
			chunkPayloads.current = [];
			attachmentId.current = null;
		}
	}
	async function startDictation() {
		if (!api || !navigator.mediaDevices?.getUserMedia) { setError("Microphone recording is unavailable."); return; }
		setError(null);
		setDictationState(null);
		try {
			void bridges.whisper?.warmSelected?.().catch((reason) => setError(publicError(reason)));
			const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
			dictationStream.current = stream;
			const mimeType = ["audio/mp4", "audio/webm"].find((candidate) => MediaRecorder.isTypeSupported(candidate)) ?? "";
			const media = mimeType ? new MediaRecorder(stream, { mimeType }) : new MediaRecorder(stream);
			const receipt = await api.audioBegin({ sessionId, mediaType: media.mimeType || mimeType || "audio/webm", metadata: { capture: "review_comment_dictation" } });
			attachmentId.current = String(receipt.attachmentId);
			chunkIndex.current = 0;
			chunkWrites.current = [];
			chunkPayloads.current = [];
			dictationChunks.current = [];
			media.ondataavailable = (event) => {
				if (!event.data.size || !attachmentId.current) return;
				dictationChunks.current.push(event.data);
				const index = chunkIndex.current++;
				chunkWrites.current.push(event.data.arrayBuffer().then((buffer) => {
					const bytes = new Uint8Array(buffer); let binary = "";
					for (const byte of bytes) binary += String.fromCharCode(byte);
					const base64Data = btoa(binary);
					chunkPayloads.current.push({ index, base64Data });
					return api.audioAppend({ attachmentId: attachmentId.current, chunkIndex: index, base64Data });
				}));
			};
			media.onstop = () => { void finishDictation(media.mimeType || mimeType || "audio/webm"); };
			dictationRecorder.current = media;
			media.start(1000);
			setDictating(true);
			setDictationState("Listening…");
		} catch (reason) {
			setDictationState("Dictation unavailable");
			setError(publicError(reason));
		}
	}
	function stopDictation() {
		dictationRecorder.current?.stop();
		dictationRecorder.current = null;
		setDictating(false);
	}
	async function submit() {
		if (!api || !view) return;
		setSaving(true); setError(null);
		try { await api.submit({ sessionId, expectedRevision: view.draftRevision }); await refresh(); }
		catch (reason) { setError(publicError(reason)); } finally { setSaving(false); }
	}
	async function correctTranscript(attachmentId:string,text:string){
		if(!api||!view)return;
		setSaving(true);setError(null);
		try{await api.correctTranscript({sessionId,expectedRevision:view.draftRevision,attachmentId,correctedText:text});await refresh();}
		catch(reason){setError(publicError(reason));}finally{setSaving(false);}
	}
	async function startCorrection(){
		if(!api||!view?.result)return;
		const resultId=String((view.result as Record<string,unknown>).resultId??"");
		if(!resultId)return;
		setSaving(true);setError(null);
		try{await api.supersede({resultId,reason:"Reviewer correction from sealed result"});}
		catch(reason){setError(publicError(reason));}finally{setSaving(false);}
	}
	async function adjudicateCampaign(){
		if(!api||!campaignId||!campaignNote.trim()||!campaign)return;
		const resultIds=(Array.isArray(campaign.results)?campaign.results:[]).filter((item):item is Record<string,unknown>=>Boolean(item)&&typeof item==="object"&&(item as Record<string,unknown>).state==="submitted").map((item)=>String(item.resultId));
		if(resultIds.length<2){setError("At least two current results are required for adjudication.");return;}
		setSaving(true);setError(null);
		try{setCampaign(await api.campaignAdjudicate({campaignId,resultIds,decision:{kind:"reviewed_disagreement",resolution:"retain_individual_results"},rationale:campaignNote.trim(),adjudicatorId:"local-adjudicator"}));setCampaignNote("");}
		catch(reason){setError(publicError(reason));}finally{setSaving(false);}
	}
	async function closeCampaign(){
		if(!api||!campaignId||!campaignNote.trim())return;
		setSaving(true);setError(null);
		try{setCampaign(await api.campaignClose({campaignId,rationale:campaignNote.trim()}));setCampaignNote("");}
		catch(reason){setError(publicError(reason));}finally{setSaving(false);}
	}
	async function copyResultReference(){
		const resultId=String((view?.result as Record<string,unknown>|null)?.resultId??"");
		if(!resultId)return;
		setError(null);
		try{
			await copyText(`Human annotation result: ${resultId}`);
			setResultCopyState("copied");
		}catch(reason){setError(publicError(reason));}
	}

	if (!view) return <div className="human-annotation-loading" role="status">Loading review…</div>;
	const presentedAnswered = presentedQuestions.filter((item) => view.answers[item.id] != null).length;
	const sealed = view.state === "submitted";
	const resultId=sealed?String((view.result as Record<string,unknown>|null)?.resultId??""):"";
	return <section className="human-annotation-workspace-shell" aria-label="Human annotation review"><div className="human-annotation-workspace">
		<header className="human-annotation-header">
			<div><strong>{String(task.title ?? "Review")}</strong><span>{`Question ${Math.min(active + 1, presentedQuestions.length)} of ${presentedQuestions.length}`}</span></div>
			<div className="human-annotation-header-actions"><span role="status">{saving ? "Saving…" : sealed ? "Submitted" : "Saved"}</span>{onFocusedChange ? <button type="button" aria-pressed={focused} onClick={() => onFocusedChange(!focused)}>{focused ? "Show chat" : "Focus review"}</button> : null}{!sealed?<button type="button" onClick={onClose}>Exit & resume</button>:null}</div>
		</header>
		<div className="human-annotation-evidence" aria-label="Evidence">
			<div className="human-annotation-evidence-tabs" role="tablist">{evidenceItems.map((item,index)=><button type="button" role="tab" aria-selected={index===activeEvidenceIndex} key={evidenceKey(item)} onClick={()=>setActiveEvidenceIndex(index)}>{evidenceLabel(item,index)}</button>)}</div>
			<div className="human-annotation-evidence-label">Evidence · {String(activeEvidence.kind ?? "subject").replaceAll("_", " ")}</div>
			{visual ? <VisualPane artifact={visual} onClose={() => undefined} /> : <EvidenceViewer evidence={activeEvidence} selector={selector} onSelect={setSelector} />}
		</div>
		<div className="human-annotation-review-rail">
			<details className="human-annotation-instructions"><summary>Review instructions</summary><p>{String(task.instructions??"Review only the evidence shown.")}</p></details>
			{sealed ? <><div className="human-annotation-success"><strong>Review submitted</strong><p>This result is sealed and immutable.</p>{resultId?<div className="human-annotation-result-reference"><span>Result ID</span><code>{resultId}</code></div>:null}<div className="human-annotation-completion-actions">{resultId?<button type="button" onClick={()=>void copyResultReference()}>{resultCopyState==="copied"?"Copied":"Copy for chat"}</button>:null}<button type="button" className="primary" onClick={onClose}>Exit & resume chat</button></div><button type="button" className="human-annotation-correction" disabled={saving} onClick={()=>void startCorrection()}>Create correction</button></div>{campaign?<CampaignPanel campaign={campaign} note={campaignNote} onNote={setCampaignNote} onAdjudicate={()=>void adjudicateCampaign()} onClose={()=>void closeCampaign()} saving={saving}/>:null}</> : question ? <>
				<div className="human-annotation-progress"><span>{presentedAnswered} of {presentedQuestions.length} answered</span><progress value={presentedAnswered} max={presentedQuestions.length} /></div>
				<fieldset className="human-annotation-question"><legend>{question.prompt}{question.required ? <span aria-label="required"> *</span> : null}</legend>{question.decisionCriteria ? <div className="human-question-criteria"><p><strong>Focus</strong><span>{question.decisionCriteria.evidence}</span></p><p><strong>Choose</strong><span>{question.decisionCriteria.answerRule}</span></p></div> : question.helpText ? <p>{question.helpText}</p> : null}<QuestionControl question={question} value={(view.answers[question.id] as { value?: unknown } | undefined)?.value} optionOrder={optionOrder(view.presentation,question)} onChange={(value) => void saveAnswer(value)} /></fieldset>
				<SelectorEditor evidence={activeEvidence} value={selector} onChange={setSelector}/><div className="human-annotation-comment"><label htmlFor="annotation-comment">Contextual comment</label><textarea id="annotation-comment" value={comment} onChange={(event) => setComment(event.target.value)} placeholder="Explain what you noticed in this evidence…" /><div>{whisperAvailable ? <button type="button" className={`human-annotation-dictation${dictating ? " is-recording" : ""}`} disabled={dictationState==="Transcribing and saving audio…"} aria-label={dictating ? "Stop dictation" : dictationState==="Transcribing and saving audio…" ? "Transcribing comment" : "Dictate comment with Whisper"} title={dictating ? "Stop dictation" : "Dictate comment with Whisper"} aria-pressed={dictating} onClick={() => dictating ? stopDictation() : void startDictation()}><MicIcon /></button> : null}<button type="button" disabled={(!comment.trim() && !dictationAttachmentId) || saving || dictating} onClick={() => void saveComment()}>Save comment</button></div>{dictationState ? <span role="status">{dictationState}</span> : null}</div>
				{view.attachments.filter((item)=>item.kind==="audio"&&item.state!=="recording").map((item)=><AudioReview key={String(item.attachmentId)} sessionId={sessionId} attachment={item} onCorrect={(text)=>void correctTranscript(String(item.attachmentId),text)} />)}
			</> : <p>No questions were provided.</p>}
			{error ? <div className="human-annotation-error" role="alert">{error}</div> : null}
			{!sealed ? <footer className="human-annotation-nav"><button type="button" disabled={active === 0} onClick={() => setActive((i) => Math.max(0, i - 1))}>Previous</button>{active < presentedQuestions.length - 1 ? <button type="button" onClick={() => setActive((i) => Math.min(presentedQuestions.length - 1, i + 1))}>Next</button> : <button type="button" className="primary" disabled={!canSubmit || saving} onClick={() => void submit()}>Submit</button>}</footer> : null}
		</div>
	</div></section>;
}

function EvidenceViewer({evidence,selector,onSelect}:{evidence:Record<string,unknown>;selector:Record<string,unknown>;onSelect:(value:Record<string,unknown>)=>void}){
	const kind=String(evidence.kind??"");
	const snapshot=(evidence.snapshot&&typeof evidence.snapshot==="object"?evidence.snapshot:{}) as Record<string,unknown>;
	if(["file","file_snapshot"].includes(kind)&&typeof snapshot.text==="string"){
		const start=Number(snapshot.lineStart??1);const lines=snapshot.text.split("\n");
		return <div className="human-evidence-file" aria-label="File snapshot"><header><strong>{String(evidence.label??evidence.artifactId??evidence.id??"File snapshot")}</strong><span>immutable snapshot · {lines.length} lines</span></header><pre>{lines.map((line,index)=>{const number=start+index;return <button type="button" key={number} className={selector.kind==="text_quote"&&Number(selector.lineStart)<=number&&Number(selector.lineEnd)>=number?"is-selected":""} onClick={()=>onSelect({kind:"text_quote",quote:line||"(blank line)",lineStart:number,lineEnd:number})}><span>{number}</span><code>{line||" "}</code></button>})}</pre></div>;
	}
	if(kind==="dataset_row"){
		const row=(evidence.row??snapshot.row) as Record<string,unknown>|undefined;
		if(row&&typeof row==="object")return <div className="human-evidence-dataset"><strong>{String(evidence.label??"Dataset row")}</strong><table><tbody>{Object.entries(row).map(([field,value])=><tr key={field} className={selector.kind==="dataset_field"&&selector.field===field?"is-selected":""} onClick={()=>onSelect({kind:"dataset_field",field})}><th>{field}</th><td>{formatEvidenceValue(value)}</td></tr>)}</tbody></table></div>;
	}
	if(kind==="image"&&typeof snapshot.dataUrl==="string")return <ImageEvidenceViewer evidence={evidence} dataUrl={snapshot.dataUrl} selector={selector} onSelect={onSelect}/>;
	if(kind==="video"&&typeof snapshot.dataUrl==="string")return <div className="human-evidence-media"><video controls src={snapshot.dataUrl}/>{typeof snapshot.transcript==="string"?<details><summary>Transcript</summary><p>{snapshot.transcript}</p></details>:null}</div>;
	if(["rollout","trace_v5","trace_span","model_call"].includes(kind)&&Array.isArray(snapshot.events))return <div className="human-evidence-trace"><header><strong>{String(evidence.label??kind.replaceAll("_"," "))}</strong><span>{snapshot.events.length} retained events</span></header><ol>{snapshot.events.map((raw,index)=>{const event=(raw&&typeof raw==="object"?raw:{value:raw}) as Record<string,unknown>;const sequence=Number(event.sequence??index+1);return <li key={`${sequence}:${index}`}><button type="button" onClick={()=>onSelect({kind:"trace_range",startSequence:sequence,endSequence:sequence,...(event.spanId?{spanId:event.spanId}:{}),...(event.modelCallId?{modelCallId:event.modelCallId}:{})})}><span>#{sequence}</span><strong>{String(event.kind??event.type??"event")}</strong><p>{String(event.summary??event.text??event.output??"")}</p></button></li>})}</ol></div>;
	if(["optimizer_checkpoint","comparison"].includes(kind)&&evidence.summary)return <div className="human-evidence-summary"><strong>{String(evidence.label??kind.replaceAll("_"," "))}</strong><pre>{formatEvidenceValue(evidence.summary)}</pre></div>;
	return <div className="human-annotation-evidence-empty"><strong>{String(evidence.id??evidence.artifactId??"Evidence unavailable")}</strong><p>{evidence.digest?"This evidence is digest-bound, but its immutable preview was not packaged with the task.":"This evidence adapter is unavailable in the current build."}</p></div>;
}

type ImageRegion = {kind:"image_region";x:number;y:number;width:number;height:number};

function ImageEvidenceViewer({evidence,dataUrl,selector,onSelect}:{evidence:Record<string,unknown>;dataUrl:string;selector:Record<string,unknown>;onSelect:(value:Record<string,unknown>)=>void}){
	const anchor=useRef<{x:number;y:number}|null>(null);
	const [draft,setDraft]=useState<ImageRegion|null>(null);
	const selected=selector.kind==="image_region"?normalizeImageRegion(selector):null;
	const visible=draft??selected;
	function point(event:ReactPointerEvent<HTMLDivElement>){
		const rect=event.currentTarget.getBoundingClientRect();
		return {x:clamp((event.clientX-rect.left)/rect.width),y:clamp((event.clientY-rect.top)/rect.height)};
	}
	function begin(event:ReactPointerEvent<HTMLDivElement>){
		if(event.button!==0)return;
		event.preventDefault();event.currentTarget.setPointerCapture(event.pointerId);
		const start=point(event);anchor.current=start;setDraft({kind:"image_region",x:start.x,y:start.y,width:0,height:0});
	}
	function move(event:ReactPointerEvent<HTMLDivElement>){
		if(!anchor.current||!event.currentTarget.hasPointerCapture(event.pointerId))return;
		setDraft(regionBetween(anchor.current,point(event)));
	}
	function finish(event:ReactPointerEvent<HTMLDivElement>){
		if(!anchor.current)return;
		const region=regionBetween(anchor.current,point(event));anchor.current=null;
		if(event.currentTarget.hasPointerCapture(event.pointerId))event.currentTarget.releasePointerCapture(event.pointerId);
		const bounded={...region,width:Math.max(region.width,.005),height:Math.max(region.height,.005)};
		setDraft(null);onSelect(roundImageRegion(bounded));
	}
	return <figure className="human-evidence-media human-evidence-image">
		<div className="human-image-selection-stage">
			<div className="human-image-selection-frame" role="img" aria-label="Drag across the image to select a review region. Exact normalized coordinates remain editable below." tabIndex={0} onPointerDown={begin} onPointerMove={move} onPointerUp={finish} onPointerCancel={()=>{anchor.current=null;setDraft(null);}}>
				<img draggable={false} src={dataUrl} alt={String(evidence.alt??evidence.label??"Review evidence")}/>
				{visible?<span className="human-image-selection-box" style={{left:`${visible.x*100}%`,top:`${visible.y*100}%`,width:`${visible.width*100}%`,height:`${visible.height*100}%`}} aria-hidden="true"><i/><i/><i/><i/></span>:null}
			</div>
		</div>
		<figcaption><span>{String(evidence.caption??evidence.label??"Digest-bound image")}</span><small>Drag to select a region; use the coordinate fields for keyboard precision.</small></figcaption>
	</figure>;
}

function regionBetween(start:{x:number;y:number},end:{x:number;y:number}):ImageRegion{
	return {kind:"image_region",x:Math.min(start.x,end.x),y:Math.min(start.y,end.y),width:Math.abs(end.x-start.x),height:Math.abs(end.y-start.y)};
}

function normalizeImageRegion(value:Record<string,unknown>):ImageRegion{
	const x=clamp(Number(value.x??0));const y=clamp(Number(value.y??0));
	return {kind:"image_region",x,y,width:clamp(Number(value.width??1),0,1-x),height:clamp(Number(value.height??1),0,1-y)};
}

function roundImageRegion(value:ImageRegion):ImageRegion{
	const rounded=(number:number)=>Math.round(number*10_000)/10_000;
	return {kind:"image_region",x:rounded(value.x),y:rounded(value.y),width:rounded(value.width),height:rounded(value.height)};
}

function clamp(value:number,min=0,max=1):number{return Math.min(max,Math.max(min,Number.isFinite(value)?value:min));}

function SelectorEditor({evidence,value,onChange}:{evidence:Record<string,unknown>;value:Record<string,unknown>;onChange:(value:Record<string,unknown>)=>void}){
	const kind=String(evidence.kind??"");
	const number=(key:string,fallback:number)=>Number(value[key]??fallback);
	if(["file","file_snapshot"].includes(kind))return <div className="human-selector"><strong>Text evidence target</strong><label>Quote<input value={String(value.quote??"")} onChange={(event)=>onChange({...value,kind:"text_quote",quote:event.target.value})}/></label><div><label>Start line<input type="number" min={1} value={number("lineStart",1)} onChange={(event)=>onChange({...value,kind:"text_quote",lineStart:Number(event.target.value)})}/></label><label>End line<input type="number" min={number("lineStart",1)} value={number("lineEnd",number("lineStart",1))} onChange={(event)=>onChange({...value,kind:"text_quote",lineEnd:Number(event.target.value)})}/></label></div></div>;
	if(kind==="dataset_row")return <div className="human-selector"><strong>Dataset field target</strong><label>Field<input value={String(value.field??"")} onChange={(event)=>onChange({kind:"dataset_field",field:event.target.value})}/></label></div>;
	if(kind==="image")return <RegionSelector value={value} onChange={onChange}/>;
	if(kind==="video")return <div className="human-selector"><strong>Video time range</strong><div><label>Start ms<input type="number" min={0} value={number("startMs",0)} onChange={(event)=>onChange({...value,kind:"time_range",startMs:Number(event.target.value)})}/></label><label>End ms<input type="number" min={number("startMs",0)} value={number("endMs",1000)} onChange={(event)=>onChange({...value,kind:"time_range",endMs:Number(event.target.value)})}/></label></div></div>;
	if(["rollout","trace_v5","trace_span","model_call"].includes(kind))return <div className="human-selector"><strong>Trace evidence range</strong><div><label>Start sequence<input type="number" min={0} value={number("startSequence",0)} onChange={(event)=>onChange({...value,kind:"trace_range",startSequence:Number(event.target.value)})}/></label><label>End sequence<input type="number" min={number("startSequence",0)} value={number("endSequence",number("startSequence",0))} onChange={(event)=>onChange({...value,kind:"trace_range",endSequence:Number(event.target.value)})}/></label></div></div>;
	return <div className="human-selector"><strong>Evidence target</strong><label>Semantic target<input value={String(value.targetId??evidence.id??"subject")} onChange={(event)=>onChange({kind:"visual_target",targetId:event.target.value})}/></label></div>;
}

function RegionSelector({value,onChange}:{value:Record<string,unknown>;onChange:(value:Record<string,unknown>)=>void}){
	const fields:Array<[string,string]>=[["x","X"],["y","Y"],["width","Width"],["height","Height"]];
	return <div className="human-selector"><strong>Normalized image region</strong><div>{fields.map(([key,label])=><label key={key}>{label}<input type="number" min={0} max={1} step={0.01} value={Number(value[key]??(key==="width"||key==="height"?1:0))} onChange={(event)=>onChange({...value,kind:"image_region",[key]:Number(event.target.value)})}/></label>)}</div></div>;
}

function CampaignPanel({campaign,note,onNote,onAdjudicate,onClose,saving}:{campaign:Record<string,unknown>;note:string;onNote:(value:string)=>void;onAdjudicate:()=>void;onClose:()=>void;saving:boolean}){
	const agreement=Array.isArray(campaign.agreement)?campaign.agreement as Array<Record<string,unknown>>:[];
	const state=String(campaign.state??"open");
	return <section className="human-campaign" aria-label="Campaign review"><header><div><strong>{String(campaign.name??"Annotation campaign")}</strong><span>{state.replaceAll("_"," ")}</span></div><b>{Number(campaign.submittedResultCount??0)} current results</b></header><div className="human-campaign-metrics"><span><b>{Number(campaign.disagreementCount??0)}</b> disagreements</span><span><b>{Number(campaign.adjudicationCount??0)}</b> adjudications</span><span><b>{Number(campaign.sessionCount??0)}</b> assignments</span></div>{agreement.length?<details><summary>Question agreement</summary>{agreement.map((item)=><div className="human-agreement" key={String(item.questionId)}><span>{String(item.questionId)}</span><progress value={Number(item.agreement??0)} max={1}/><b>{Math.round(Number(item.agreement??0)*100)}%</b></div>)}</details>:null}{state!=="closed"?<><label>Decision rationale<textarea value={note} onChange={(event)=>onNote(event.target.value)} placeholder="Explain the campaign decision…"/></label><div className="human-campaign-actions">{Number(campaign.disagreementCount??0)>0&&Number(campaign.adjudicationCount??0)===0?<button type="button" disabled={saving||!note.trim()} onClick={onAdjudicate}>Record adjudication</button>:null}<button type="button" disabled={saving||!note.trim()} onClick={onClose}>Close campaign</button></div></>:null}</section>;
}

function defaultSelector(evidence:Record<string,unknown>):Record<string,unknown>{
	switch(String(evidence.kind??"")){
		case "file":case "file_snapshot":return {kind:"text_quote",quote:"Review target",lineStart:1,lineEnd:1};
		case "dataset_row":return {kind:"dataset_field",field:Object.keys((evidence.row??(evidence.snapshot as Record<string,unknown>|undefined)?.row??{}) as Record<string,unknown>)[0]??"value"};
		case "image":return {kind:"image_region",x:0,y:0,width:1,height:1};
		case "video":return {kind:"time_range",startMs:0,endMs:1000};
		case "rollout":case "trace_v5":case "trace_span":case "model_call":return {kind:"trace_range",startSequence:0,endSequence:0};
		default:return {kind:"visual_target",targetId:String(evidence.id??evidence.artifactId??"subject")};
	}
}

function formatEvidenceValue(value:unknown):string{return typeof value==="string"?value:JSON.stringify(value,null,2);}

function evidenceKey(item:Record<string,unknown>):string{
	return `${String(item.kind??"evidence")}:${String(item.id??item.artifactId??item.digest??"unknown")}`;
}

function evidenceLabel(item:Record<string,unknown>,index:number):string{
	const explicit=item.label??item.role;
	if(typeof explicit==="string"&&explicit.trim())return explicit.replaceAll("_"," ");
	if(typeof item.artifactId==="string"&&item.artifactId.trim())return item.artifactId;
	return String(item.kind??`evidence ${index+1}`).replaceAll("_"," ");
}

function AudioReview({sessionId,attachment,onCorrect}:{sessionId:string;attachment:Record<string,unknown>;onCorrect:(text:string)=>void}){
	const api=bridges.humanAnnotations; const [source,setSource]=useState<string|null>(null); const original=String(attachment.machineTranscript??""); const [transcript,setTranscript]=useState(String(attachment.correctedTranscript??original));
	useEffect(()=>{setTranscript(String(attachment.correctedTranscript??original));},[attachment.correctedTranscript,original]);
	useEffect(()=>{if(!api)return;void api.audioRead(sessionId,String(attachment.attachmentId)).then((data)=>setSource(`data:${data.mediaType};base64,${data.base64Data}`)).catch(()=>setSource(null));},[api,sessionId,attachment.attachmentId]);
	return <section className="human-annotation-audio"><strong>Audio comment</strong>{source?<audio controls src={source} />:<span>Saved audio unavailable for playback</span>}<label>Transcript<textarea value={transcript} placeholder="Transcript unavailable; you can add one manually." onChange={(event)=>setTranscript(event.target.value)} /></label><button type="button" disabled={!transcript.trim()||transcript===String(attachment.correctedTranscript??original)} onClick={()=>onCorrect(transcript.trim())}>Save transcript correction</button></section>;
}

function QuestionControl({ question, value, optionOrder, onChange }: { question: Question; value: unknown; optionOrder: string[]; onChange: (value: unknown) => void }) {
	const options = [...(question.options ?? [])].sort((a,b) => optionOrder.indexOf(a.id)-optionOrder.indexOf(b.id));
	if (question.type === "yes_no") return <div className="human-annotation-options">{[{id:"yes",label:"Yes"},{id:"no",label:"No"},...(question.allowAbstain?[{id:"not_enough_evidence",label:"Not enough evidence"}]:[])].map((o) => <label key={o.id}><input type="radio" name={question.id} checked={value === o.id} onChange={() => onChange(o.id)} /><span><strong>{o.label}</strong>{question.answerGuidance?.[o.id] ? <small>{question.answerGuidance[o.id]}</small> : null}</span></label>)}</div>;
	if (["single_select","quiz_single","likert"].includes(question.type)) return <div className="human-annotation-options">{options.map((o) => <label key={o.id}><input type="radio" name={question.id} checked={value === o.id} onChange={() => onChange(o.id)} />{o.label ?? o.id}</label>)}</div>;
	if (question.type === "preference") { const selected=typeof value==="object"&&value?String((value as {optionId?:unknown}).optionId??""):String(value??""); const choices=[...options,...(question.allowTie?[{id:"tie",label:"Tie"}]:[]),...(question.allowNeither?[{id:"neither",label:"Neither acceptable"}]:[]),...(question.allowAbstain?[{id:"not_enough_evidence",label:"Not enough evidence"}]:[])]; return <div className="human-annotation-options">{choices.map((o) => <label key={o.id}><input type="radio" name={question.id} checked={selected === o.id} onChange={() => onChange({kind:"preference",optionId:o.id})} />{o.label ?? o.id}</label>)}</div>; }
	if (["multi_select","quiz_multi"].includes(question.type)) { const selected = Array.isArray(value) ? value as string[] : []; return <div className="human-annotation-options">{options.map((o) => <label key={o.id}><input type="checkbox" checked={selected.includes(o.id)} onChange={(event) => onChange(event.target.checked ? [...selected,o.id] : selected.filter((id) => id !== o.id))} />{o.label ?? o.id}</label>)}</div>; }
	if (["ranking","quiz_order"].includes(question.type)) return <RankingControl options={options} value={Array.isArray(value)?value as string[]:optionOrder} onChange={onChange} />;
	if (question.type==="rubric_score") return <div className="human-annotation-score">{Array.from({ length: (question.validation?.max ?? 5) - (question.validation?.min ?? 1) + 1 }, (_, index) => index + (question.validation?.min ?? 1)).map((score) => <button type="button" aria-label={`${score}${question.anchors?.[String(score)]?`: ${question.anchors[String(score)]}`:""}`} title={question.anchors?.[String(score)]} aria-pressed={value === score} key={score} onClick={() => onChange(score)}>{score}</button>)}</div>;
	if (question.type==="numeric") return <input type="number" min={question.validation?.min} max={question.validation?.max} value={typeof value==="number"?value:""} onChange={(event)=>{if(event.target.value!=="")onChange(Number(event.target.value));}} />;
	if (question.type==="target_selection") return <button type="button" className="human-annotation-target" aria-pressed={value!=null} onClick={()=>onChange({kind:"visual_target",targetId:"subject"})}>{value?"Subject selected":"Select the full subject"}</button>;
	if (["short_text","long_text"].includes(question.type)) return <TextAnswer multiline={question.type==="long_text"} value={typeof value === "string" ? value : ""} onCommit={onChange} />;
	return <textarea aria-label="Structured response JSON" value={value == null ? "" : JSON.stringify(value)} onChange={(event) => { try { onChange(JSON.parse(event.target.value)); } catch { /* keep editing */ } }} placeholder="Enter a structured response" />;
}

function RankingControl({options,value,onChange}:{options:Array<{id:string;label?:string}>;value:string[];onChange:(value:unknown)=>void}){
	const ordered=value.length?value:options.map((option)=>option.id); const labels=new Map(options.map((option)=>[option.id,option.label??option.id]));
	function move(index:number,delta:number){const next=[...ordered];const target=index+delta;if(target<0||target>=next.length)return;[next[index],next[target]]=[next[target],next[index]];onChange(next);}
	return <ol className="human-annotation-ranking">{ordered.map((id,index)=><li key={id}><span>{labels.get(id)??id}</span><button type="button" aria-label={`Move ${labels.get(id)??id} up`} disabled={index===0} onClick={()=>move(index,-1)}>↑</button><button type="button" aria-label={`Move ${labels.get(id)??id} down`} disabled={index===ordered.length-1} onClick={()=>move(index,1)}>↓</button></li>)}</ol>;
}

function TextAnswer({value,multiline,onCommit}:{value:string;multiline:boolean;onCommit:(value:string)=>void}){
	const [draft,setDraft]=useState(value);
	useEffect(()=>setDraft(value),[value]);
	// Text answers use an explicit commit. Combining blur-save with the Save
	// button fires twice when the button takes focus: the blur mutation wins,
	// then the click retries with a stale revision and surfaces a false conflict.
	const field=multiline?<textarea value={draft} onChange={(event)=>setDraft(event.target.value)} />:<input value={draft} onChange={(event)=>setDraft(event.target.value)} />;
	return <div className="human-annotation-text-answer">{field}<button type="button" disabled={draft===value} onClick={()=>onCommit(draft)}>Save response</button></div>;
}

function optionOrder(presentation:Record<string,unknown>,question:Question):string[]{
	const questions=(presentation.questions??{}) as Record<string,{optionOrder?:string[]}>;
	return questions[question.id]?.optionOrder ?? (question.options??[]).map((option)=>option.id);
}

function isPresented(question:Question,answers:Record<string,unknown>):boolean{
	const when=(question as Question & {visibility?:{when?:{questionId?:string;operator?:string;value?:unknown}}}).visibility?.when;
	if(!when?.questionId)return true;
	const stored=answers[when.questionId] as {value?:unknown}|undefined; const value=stored?.value;
	switch(when.operator??"equals"){
		case "answered":return stored!=null; case "not_answered":return stored==null;
		case "not_equals":return value!==when.value; case "contains":return Array.isArray(value)&&value.includes(when.value);
		case "contains_any":{const expected=when.value;return Array.isArray(value)&&Array.isArray(expected)&&value.some((item)=>expected.includes(item));}
		default:return value===when.value;
	}
}
