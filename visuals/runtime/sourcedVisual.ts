/**
 * Compile allowlisted agent TSX and return a pane Shell.
 *
 * Host ingest stays outside this module: VisualHost builds ReplayClient and
 * passes replay / events / state. The compiled module may import advertised
 * components and useLiveEvalStream; it may not discover URLs.
 */

import { createElement, type ComponentType } from "react";
import * as React from "react";
import * as JsxRuntime from "react/jsx-runtime";
import * as ReactDOM from "react-dom";
import { transform } from "sucrase";
import * as Chrome from "../chrome/VisualChrome.tsx";
import * as ChromeUseLiveEvalStream from "../chrome/useLiveEvalStream.ts";
import * as CandidateInspectorV1 from "../components/candidate_inspector.v1/CandidateInspector.tsx";
import * as DetailModalV1 from "../components/detail_modal.v1/DetailModal.tsx";
import * as EventStreamV1 from "../components/event_stream.v1/EventStream.tsx";
import * as MetricsV1 from "../components/metrics.v1/Metrics.tsx";
import * as ScrubberV1 from "../components/scrubber.v1/Scrubber.tsx";
import {
  SOURCED_ALLOWED_IMPORTS,
  SOURCED_TEMPLATE_ID,
  validateSourcedSource
} from "./sourcedValidate.ts";

export {
  isSourcedTemplate,
  SOURCED_ALLOWED_IMPORTS,
  SOURCED_KIND,
  SOURCED_MAX_SOURCE_BYTES,
  SOURCED_PROTOCOL,
  SOURCED_TEMPLATE_ID,
  validateSourcedSource
} from "./sourcedValidate.ts";

type ShellProps = {
  title?: string;
  [key: string]: unknown;
};

export type SourcedCompileResult =
  | { ok: true; Shell: ComponentType<ShellProps> }
  | { ok: false; error: string };

function asCjs(mod: object): Record<string, unknown> {
  const record = mod as Record<string, unknown>;
  return {
    ...record,
    default: record.default ?? mod,
    __esModule: true
  };
}

/** Every specifier the validator allows, as a type. */
type SourcedSpecifier = (typeof SOURCED_ALLOWED_IMPORTS)[number];

/**
 * One entry per allowlisted specifier, and the entry *is* the module that
 * specifier names — never a hand-picked subset of its exports.
 *
 * `@synth/visuals/chrome` used to map to `{ VisualChrome }` while the validator
 * allowlisted the specifier whole, so `MetricStrip` — a real export of the
 * chrome barrel, used by most bundled families — passed validation and then
 * resolved `undefined` at runtime: the allowlist said yes and the module map
 * said nothing. A curated export subset is a second allowlist nobody
 * maintains, and it will drift again.
 *
 * Two properties keep the two lists from disagreeing. The key type is the
 * allowlist itself, so the compiler rejects an allowlisted specifier with no
 * module *and* a module no specifier allows. And each value is the module
 * namespace, so a specifier grants exactly what that module exports — every
 * name, and not one name more. Widening still requires editing the module or
 * the allowlist; it can no longer happen by accident.
 */
const SOURCED_MODULE_SOURCES: Record<SourcedSpecifier, object> = {
  react: React,
  "react/jsx-runtime": JsxRuntime,
  "react/jsx-dev-runtime": JsxRuntime,
  "react-dom": ReactDOM,
  "@synth/visuals/chrome": Chrome,
  "@synth/visuals/chrome/useLiveEvalStream": ChromeUseLiveEvalStream,
  "@synth/visuals/components/event_stream.v1": EventStreamV1,
  "@synth/visuals/components/detail_modal.v1": DetailModalV1,
  "@synth/visuals/components/metrics.v1": MetricsV1,
  "@synth/visuals/components/scrubber.v1": ScrubberV1,
  "@synth/visuals/components/candidate_inspector.v1": CandidateInspectorV1
};

const ALLOWED_MODULES: Record<string, Record<string, unknown>> = Object.fromEntries(
  Object.entries(SOURCED_MODULE_SOURCES).map(([specifier, source]) => [
    specifier,
    asCjs(source)
  ])
);

function requireAllowed(id: string): Record<string, unknown> {
  const mod = ALLOWED_MODULES[id];
  if (!mod) throw new Error(`Unknown import "${id}"`);
  return mod;
}

export function sourcedInvalidShell(error: string): ComponentType<ShellProps> {
  return function SourcedInvalid(props: ShellProps) {
    return createElement(Chrome.VisualChrome, {
      kicker: "Sourced",
      title: typeof props.title === "string" ? props.title : "Custom visual",
      testId: "visual-sourced",
      children: createElement(
        "p",
        { role: "alert", "data-testid": "visual-sourced-invalid" },
        error
      )
    });
  };
}

function moduleShell(exports: Record<string, unknown>): ComponentType<ShellProps> | null {
  const candidate = exports.default ?? exports.Shell;
  return typeof candidate === "function" ? (candidate as ComponentType<ShellProps>) : null;
}

/**
 * Compile one pane-sourced module. `templateId` is the template the source
 * belongs to — `sourced.visual.v1` for a one-off carried on the visual record,
 * or a user template id whose `shell.tsx` was read from the instance state
 * root. It travels into the validator message and into sucrase's `filePath`,
 * so a syntax error names the file the author is editing.
 */
export function compileSourcedModule(
  source: string,
  templateId: string = SOURCED_TEMPLATE_ID
): SourcedCompileResult {
  const validated = validateSourcedSource(source, templateId);
  if (!validated.ok) return validated;
  try {
    const transformed = transform(source, {
      transforms: ["typescript", "jsx", "imports"],
      jsxRuntime: "automatic",
      production: true,
      filePath: `${templateId}.tsx`
    });
    const module = { exports: {} as Record<string, unknown> };
    const loader = new Function(
      "exports",
      "require",
      "module",
      `"use strict";
const fetch = undefined;
const EventSource = undefined;
const WebSocket = undefined;
const XMLHttpRequest = undefined;
${transformed.code}`
    ) as (
      exports: Record<string, unknown>,
      require: (id: string) => Record<string, unknown>,
      module: { exports: Record<string, unknown> }
    ) => void;
    loader(module.exports, requireAllowed, module);
    const Shell = moduleShell(module.exports);
    if (!Shell) {
      return { ok: false, error: "Sourced module must default-export a Shell component" };
    }
    return { ok: true, Shell };
  } catch (reason) {
    const message = reason instanceof Error ? reason.message : String(reason);
    return { ok: false, error: message };
  }
}
