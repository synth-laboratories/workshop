import type { ReportRevision } from "../bridge";

export type ReportDraft = { base: ReportRevision; title: string; summary: string; findings: string; methods: string };
const drafts = new Map<string, ReportDraft>();
const key = (id: string) => `synth.report-draft.v1:${id}`;

export function readReportDraft(id: string): ReportDraft | null {
  if (drafts.has(id)) return drafts.get(id)!;
  try {
    const value = JSON.parse(sessionStorage.getItem(key(id)) ?? "null");
    if (value && value.base?.reportId === id && Number.isInteger(value.base?.revision)
      && Array.isArray(value.base?.blocks) && [value.title, value.summary, value.findings, value.methods].every(v => typeof v === "string")) {
      drafts.set(id, value);
      return value;
    }
  } catch { /* Session storage can be unavailable; the in-memory buffer still protects navigation. */ }
  return null;
}

export function writeReportDraft(id: string, draft: ReportDraft | null): void {
  if (draft) drafts.set(id, draft); else drafts.delete(id);
  try {
    if (draft) sessionStorage.setItem(key(id), JSON.stringify(draft));
    else sessionStorage.removeItem(key(id));
  } catch { /* Keep the in-memory draft when storage is unavailable or full. */ }
}
