export const TRACE_CITATION_EVENT = "synth:trace-citation";
export type TraceCitation = { traceDigest: string; selector: string; revision: number };
// A single navigation identity shared across independently loaded visual modules.
// No source bodies or credentials are retained.
const host = globalThis as typeof globalThis & { __synthTraceCitation?: TraceCitation };
export function requestTraceCitation(traceDigest: string, selector: string) {
  host.__synthTraceCitation = { traceDigest, selector, revision: (host.__synthTraceCitation?.revision ?? 0) + 1 };
  window.dispatchEvent(new CustomEvent(TRACE_CITATION_EVENT));
}
export function currentTraceCitation(traceDigest: string) {
  const pending = host.__synthTraceCitation;
  return pending?.traceDigest === traceDigest ? pending : null;
}
