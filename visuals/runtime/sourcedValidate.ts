/**
 * Allowlist and fail-closed scan for sourced visual TSX.
 *
 * Kind `sourced_visual`. Protocol `whole_file.v1`. The pane compiles this
 * source; unconstrained modules that fetch, eval, or guess `/events` URLs
 * never mount.
 */

import { isGuessedStreamUrl } from "./liveStream.ts";

export const SOURCED_TEMPLATE_ID = "sourced.visual.v1" as const;
export const SOURCED_KIND = "sourced_visual" as const;
export const SOURCED_PROTOCOL = "whole_file.v1" as const;
export const SOURCED_MAX_SOURCE_BYTES = 256 * 1024;

export const SOURCED_ALLOWED_IMPORTS = [
  "react",
  "react/jsx-runtime",
  "react/jsx-dev-runtime",
  "react-dom",
  "@synth/visuals/chrome",
  "@synth/visuals/chrome/useLiveEvalStream",
  "@synth/visuals/components/event_stream.v1",
  "@synth/visuals/components/detail_modal.v1"
] as const;

export type SourcedValidateResult =
  | { ok: true; imports: string[] }
  | { ok: false; error: string };

const ALLOWED = new Set<string>(SOURCED_ALLOWED_IMPORTS);

const IMPORT_FROM =
  /(?:^|[\n;])\s*import\s+(type\s+)?([\s\S]*?)\sfrom\s+["']([^"']+)["']/g;
const SIDE_EFFECT_IMPORT = /(?:^|[\n;])\s*import\s+["']([^"']+)["']/g;
const EXPORT_FROM = /(?:^|[\n;])\s*export\s+[\s\S]*?\sfrom\s+["']([^"']+)["']/g;
const STRING_LITERAL = /(["'])((?:\\.|(?!\1).)*)\1/g;

const FORBIDDEN: Array<{ pattern: RegExp; label: string }> = [
  { pattern: /\bfetch\s*\(/, label: "fetch" },
  { pattern: /\bEventSource\b/, label: "EventSource" },
  { pattern: /\beval\s*\(/, label: "eval" },
  { pattern: /\bnew\s+Function\b/, label: "Function" },
  { pattern: /\bFunction\s*\(/, label: "Function" },
  { pattern: /\bWebSocket\b/, label: "WebSocket" },
  { pattern: /\bXMLHttpRequest\b/, label: "XMLHttpRequest" },
  { pattern: /\bimport\s*\(/, label: "dynamic import" },
  { pattern: /\brequire\s*\(/, label: "require" },
  { pattern: /\bwindow\s*\./, label: "window" },
  { pattern: /\bglobalThis\s*\./, label: "globalThis" },
  { pattern: /\bimport\.meta\b/, label: "import.meta" }
];

export function isSourcedTemplate(templateId: string | null | undefined): boolean {
  return templateId === SOURCED_TEMPLATE_ID;
}

function collectFrom(source: string, regex: RegExp, group: number, skipType = false): string[] {
  const found: string[] = [];
  regex.lastIndex = 0;
  let match: RegExpExecArray | null;
  while ((match = regex.exec(source))) {
    if (skipType && match[1]) continue;
    const specifier = match[group];
    if (specifier) found.push(specifier);
  }
  return found;
}

export function validateSourcedSource(source: string): SourcedValidateResult {
  if (typeof source !== "string" || source.trim().length === 0) {
    return { ok: false, error: "sourced.visual.v1 requires content" };
  }
  if (new TextEncoder().encode(source).length > SOURCED_MAX_SOURCE_BYTES) {
    return { ok: false, error: "Sourced module exceeds 256KiB" };
  }

  const sideEffect = collectFrom(source, SIDE_EFFECT_IMPORT, 1);
  if (sideEffect.length > 0) {
    return { ok: false, error: `Unknown import "${sideEffect[0]}"` };
  }

  const reExports = collectFrom(source, EXPORT_FROM, 1);
  for (const specifier of reExports) {
    if (!ALLOWED.has(specifier)) {
      return { ok: false, error: `Unknown import "${specifier}"` };
    }
  }

  const imports = collectFrom(source, IMPORT_FROM, 3, true);
  for (const specifier of imports) {
    if (!ALLOWED.has(specifier)) {
      return { ok: false, error: `Unknown import "${specifier}"` };
    }
  }

  for (const rule of FORBIDDEN) {
    if (rule.pattern.test(source)) {
      return { ok: false, error: `Sourced module must not use ${rule.label}` };
    }
  }

  STRING_LITERAL.lastIndex = 0;
  let literal: RegExpExecArray | null;
  while ((literal = STRING_LITERAL.exec(source))) {
    const value = literal[2]?.replace(/\\["'\\]/g, "") ?? "";
    if (isGuessedStreamUrl(value) || value === "/events") {
      return { ok: false, error: `Sourced module must not guess stream URL ${value}` };
    }
  }

  return { ok: true, imports };
}
