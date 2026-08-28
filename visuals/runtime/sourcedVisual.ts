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
import { VisualChrome } from "../chrome/VisualChrome.tsx";
import { useLiveEvalStream } from "../chrome/useLiveEvalStream.ts";
import { CandidateInspector } from "../components/candidate_inspector.v1/CandidateInspector.tsx";
import { DetailModal } from "../components/detail_modal.v1/DetailModal.tsx";
import { EventStream } from "../components/event_stream.v1/EventStream.tsx";
import { Metrics } from "../components/metrics.v1/Metrics.tsx";
import { Scrubber } from "../components/scrubber.v1/Scrubber.tsx";
import {
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

const ALLOWED_MODULES: Record<string, Record<string, unknown>> = {
  react: asCjs(React),
  "react/jsx-runtime": asCjs(JsxRuntime),
  "react/jsx-dev-runtime": asCjs(JsxRuntime),
  "react-dom": asCjs(ReactDOM),
  "@synth/visuals/chrome": asCjs({ VisualChrome }),
  "@synth/visuals/chrome/useLiveEvalStream": asCjs({ useLiveEvalStream }),
  "@synth/visuals/components/event_stream.v1": asCjs({ EventStream }),
  "@synth/visuals/components/detail_modal.v1": asCjs({ DetailModal }),
  "@synth/visuals/components/metrics.v1": asCjs({ Metrics }),
  "@synth/visuals/components/scrubber.v1": asCjs({ Scrubber }),
  "@synth/visuals/components/candidate_inspector.v1": asCjs({ CandidateInspector })
};

function requireAllowed(id: string): Record<string, unknown> {
  const mod = ALLOWED_MODULES[id];
  if (!mod) throw new Error(`Unknown import "${id}"`);
  return mod;
}

export function sourcedInvalidShell(error: string): ComponentType<ShellProps> {
  return function SourcedInvalid(props: ShellProps) {
    return createElement(VisualChrome, {
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
