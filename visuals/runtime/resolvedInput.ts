/**
 * One rule for every template that ships a bundled example.
 *
 * Five templates imported a fixture and used it whenever the bound source was
 * absent or did not match the shape they expected. The 2026-09-03 visual QA
 * sweep caught the consequence: a visual bound to a real retained Trace V5
 * rendered `reward.breakdown.v1` with a total of `4.20` while the run it named
 * had a mean of `0.25`. Plausible example values presented as measurement are
 * worse than an empty pane, because a reviewer cannot tell the difference.
 *
 * The rule: a bundled example renders only when the binding for that input is
 * explicitly `kind: "fixture"`. Anything else either resolves, or says what it
 * was asked to show and why it could not.
 */

import { bindingList, type VisualBinding } from "./types.ts";

export type UnresolvedInput = {
  /** Template input name this pane was asked to render. */
  input: string;
  /** Declared binding kind, when a binding exists at all. */
  kind: string | null;
  /** Declared locator, when a binding exists at all. */
  source: string | null;
  /** Why nothing is rendered, in words a reviewer can act on. */
  reason: string;
};

export type ResolvedInput<T> =
  | { status: "bound"; value: T; unresolved: null }
  | { status: "fixture"; value: T; unresolved: null }
  | { status: "unresolved"; value: null; unresolved: UnresolvedInput };

export function bindingFor(
  bindings: VisualBinding[] | { inputs?: VisualBinding[]; slots?: VisualBinding[] } | undefined,
  input: string
): VisualBinding | null {
  return bindingList(bindings).find((binding) => (binding.input ?? binding.slot) === input) ?? null;
}

/**
 * Resolve one template input, refusing to substitute example data.
 *
 * `accept` returns the typed payload for a value this template can actually
 * render, and `null` for anything else — including a well-formed document of
 * the wrong schema, which is exactly the case that used to fall through to the
 * fixture.
 */
export function resolveTemplateInput<T>(options: {
  input: string;
  /** Candidate payloads, in priority order. The first accepted one wins. */
  candidates: unknown[];
  bindings?: VisualBinding[] | { inputs?: VisualBinding[]; slots?: VisualBinding[] };
  accept: (value: unknown) => T | null;
  /** Bundled example. Rendered only under an explicit `kind: "fixture"` binding. */
  fixture?: unknown;
}): ResolvedInput<T> {
  const { input, candidates, bindings, accept, fixture } = options;
  for (const candidate of candidates) {
    const value = accept(candidate);
    if (value !== null) {
      return { status: "bound", value, unresolved: null };
    }
  }
  const binding = bindingFor(bindings, input);
  if (binding?.kind === "fixture") {
    const value = accept(fixture);
    if (value !== null) return { status: "fixture", value, unresolved: null };
    return {
      status: "unresolved",
      value: null,
      unresolved: {
        input,
        kind: "fixture",
        source: binding.source ?? null,
        reason: "the declared fixture did not resolve into a payload this template can render"
      }
    };
  }
  // No binding at all is the authoring case: a template opened with nothing
  // bound. It still gets an honest empty state, not an example.
  const present = candidates.some((candidate) => candidate !== undefined && candidate !== null);
  return {
    status: "unresolved",
    value: null,
    unresolved: {
      input,
      kind: binding?.kind ?? null,
      source: binding?.source ?? null,
      reason: binding
        ? present
          ? `the bound ${binding.kind} source resolved to a payload this template cannot render`
          : `the bound ${binding.kind} source did not resolve`
        : "no source is bound to this input"
    }
  };
}
