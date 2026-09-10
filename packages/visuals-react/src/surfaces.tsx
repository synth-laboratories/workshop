import type { Diagnostic, SemanticRef } from "@synth/visuals-protocol";
import type { ReactNode } from "react";

function attributes(ref: SemanticRef): Record<string, string> {
  return { "data-semantic-kind": ref.kind, "data-semantic-id": ref.id, "data-annotation-kind": ref.kind, "data-annotation-id": ref.id };
}

export function SemanticLink({ from, to, children, onActivate }: { from: SemanticRef; to: SemanticRef; children: ReactNode; onActivate?: () => void }) {
  return <button type="button" {...attributes(from)} data-semantic-link-kind={to.kind} data-semantic-link-id={to.id} onClick={onActivate}>{children}</button>;
}

export function DiagnosticSurface({ diagnostics }: { diagnostics: Diagnostic[] }) {
  if (!diagnostics.length) return null;
  return <section className="visuals-diagnostics" aria-label="Visual diagnostics"><ul>{diagnostics.map((diagnostic, index) => <li key={`${diagnostic.code}:${index}`} data-severity={diagnostic.severity}><strong>{diagnostic.code}</strong> {diagnostic.message}{diagnostic.remediation ? <small>{diagnostic.remediation}</small> : null}</li>)}</ul></section>;
}

export function StaleProjectionNotice({ reason }: { reason?: string }) {
  return <p role="status" className="visuals-stale-projection">Showing the last known good projection{reason ? `: ${reason}` : "."}</p>;
}
