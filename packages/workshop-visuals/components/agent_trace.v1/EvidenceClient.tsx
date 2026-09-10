import { useEffect, useState } from 'react';
import type { TraceSelector, TraceVisual } from './model.ts';
import type { TraceAnnotation } from './annotations.ts';
import { evidenceEndpoint } from './evidenceEndpoint.ts';

export type EvidenceService = { url: string; capability: string };
function endpoint(service: EvidenceService, runId: string) {
  return evidenceEndpoint(service.url, runId);
}
export function useTraceEvidence(service: EvidenceService | undefined, runId: string, fallback?: TraceVisual) {
  const [state, setState] = useState<{ runId: string; projection: TraceVisual; digest: string } | null>(null);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    setError('');
    if (!service) return;
    const controller = new AbortController();
    let url: string;
    try { url = endpoint(service, runId); } catch (error) { setError(String(error)); return; }
    fetch(url, { signal: controller.signal }).then(async response => {
      const result = await response.json();
      if (!response.ok) throw new Error(result.error ?? 'Evidence unavailable');
      if (!controller.signal.aborted) setState({ runId, projection: result.projection, digest: result.evidence_digest });
    }).catch(error => { if (!controller.signal.aborted) setError(String(error)); });
    return () => controller.abort();
  }, [service?.url, runId]);
  const current = state?.runId === runId ? state : null;
  async function save(request: Record<string, unknown>) {
    if (!service || !current) throw new Error('Wait for current evidence to load before saving');
    setBusy(true); setError('');
    try {
      const response = await fetch(endpoint(service, runId), {
        method: 'POST', headers: { 'Content-Type': 'application/json', 'X-Annotation-Capability': service.capability },
        body: JSON.stringify({ ...request, expected_digest: current.digest }),
      });
      const result = await response.json();
      if (!response.ok) throw new Error(result.error ?? 'Annotation save failed');
      setState({ runId, projection: result.projection, digest: result.evidence_digest });
    } catch (error) { setError(String(error)); throw error; }
    finally { setBusy(false); }
  }
  return { projection: current?.projection ?? fallback, ready: Boolean(current), busy, error, save };
}

export function AnnotationEditor({ target, note, busy, onSave, onClose }: {
  target: TraceSelector; note?: TraceAnnotation; busy?: boolean;
  onSave: (request: Record<string, unknown>) => Promise<void>; onClose: () => void;
}) {
  const [body, setBody] = useState(note?.body ?? '');
  const [label, setLabel] = useState(note?.labels[0] ?? 'note');
  const [author, setAuthor] = useState('Local reviewer');
  const [kind, setKind] = useState('human');
  const [review, setReview] = useState(note?.reviewState ?? 'unreviewed');
  const [error, setError] = useState('');
  return <form className="atv-editor" aria-label="Annotation editor" onSubmit={async e => {
    e.preventDefault(); setError('');
    try { await onSave({ target, body, label, author, author_kind: kind, review_state: review, supersedes_id: note?.id }); onClose(); }
    catch (error) { setError(String(error)); }
  }}>
    <style>{`.atv-editor{padding:14px;border:1px solid #ba963c;background:#faf7e9;border-radius:8px;margin:12px 0;display:grid;gap:8px}.atv-editor textarea{width:100%;min-height:90px;font:inherit}.atv-editor label{display:flex;gap:8px;align-items:center}.atv-editor input,.atv-editor select{min-width:0;font:inherit}.atv-editor .fields{display:flex;gap:12px;flex-wrap:wrap}`}</style>
    <strong>{note ? 'Review / supersede annotation' : 'Annotate selected evidence'}</strong>
    <small>{target.kind} · {target.entity_id} {target.json_pointer ? `· ${target.json_pointer}` : ''}</small>
    <label>Note<textarea aria-label="Annotation note" required maxLength={12000} value={body} onChange={e => setBody(e.target.value)}/></label>
    <div className="fields"><label>Label<select aria-label="Annotation label" value={label} onChange={e => setLabel(e.target.value)}>{['note','failure','coordination','reward','capture-gap'].map(v => <option key={v}>{v}</option>)}</select></label>
      <label>Review<select aria-label="Annotation review state" value={review} onChange={e => setReview(e.target.value)}>{['unreviewed','accepted','rejected','needs_review','disputed'].map(v => <option key={v}>{v}</option>)}</select></label>
      <label>Author<input required aria-label="Annotation author" value={author} onChange={e => setAuthor(e.target.value)}/></label>
      <label>Author kind<select aria-label="Annotation author kind" value={kind} onChange={e => setKind(e.target.value)}><option value="human">Human</option><option value="model">Model</option></select></label>
    </div>
    <small>{note ? 'Saves a new evidence revision; the previous annotation remains in history.' : 'Saves to a separate evidence bundle; the rollout remains unchanged.'}</small>
    {error && <p role="alert">{error}</p>}
    <div><button disabled={busy} type="submit">{busy ? 'Saving…' : note ? 'Save annotation revision' : 'Save annotation'}</button> <button disabled={busy} type="button" onClick={onClose}>Cancel</button></div>
  </form>;
}
