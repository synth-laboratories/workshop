import type { BindingResolver } from "./bindings.ts";
import { stableSerialize } from "./query.ts";
import type { JsonValue, ResolvedSource, SourceAuthority } from "@synth/visuals-protocol";

function atPath(value: JsonValue | undefined, path?: string): JsonValue | undefined {
  if (!path) return value;
  let current: JsonValue | undefined = value;
  for (const part of path.replace(/^\//, "").split(/[./]/).filter(Boolean)) {
    if(["__proto__","prototype","constructor"].includes(part))throw new Error("Unsafe binding path");
    if (Array.isArray(current)) current = current[Number(part)];
    else if (current && typeof current === "object") current = current[part];
    else return undefined;
  }
  return current;
}

async function evidenceDigest(value: JsonValue | undefined): Promise<string> {
  const bytes = new TextEncoder().encode(stableSerialize(value));
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return `sha256:${[...new Uint8Array(digest)].map((byte) => byte.toString(16).padStart(2, "0")).join("")}`;
}

export function inlineBindingResolver(): BindingResolver {
  return {
    id: "visuals.binding.inline",
    kind: "inline",
    version: "1.0.0",
    protocolRange: "^1",
    deterministic: true,
    authorities: ["none"],
    async resolve({ binding }): Promise<ResolvedSource> {
      const value = atPath(binding.data, binding.path);
      if(value===undefined)throw new Error("Inline binding data or selected path is missing");
      const digest = await evidenceDigest(binding.data);
      return {
        binding,
        value,
        evidence: {
          id: `inline:${digest}`,
          kind: "inline",
          schema: binding.schema ?? "application/json",
          digest,
          authority: "untrusted",
          freshness: "current",
          completeness: "complete",
          exactness: "exact",
        },
      };
    },
  };
}

export function loaderBindingResolver(options: {
  kind: string;
  id?: string;
  version?: string;
  authority?: SourceAuthority;
  load: (source: string, signal: AbortSignal) => Promise<JsonValue>;
  schema?: string;
}): BindingResolver {
  return {
    id: options.id ?? `visuals.binding.${options.kind}`,
    kind: options.kind,
    version: options.version ?? "1.0.0",
    protocolRange: "^1",
    deterministic: true,
    authorities: options.kind === "fixture" || options.kind === "local_cas" ? ["filesystem_read"] : ["none"],
    async resolve({ binding }, signal): Promise<ResolvedSource> {
      if (!binding.source) throw new Error(`${options.kind} binding requires a source`);
      const loaded = await options.load(binding.source, signal);
      signal.throwIfAborted();
      const value = atPath(loaded, binding.path);
      if(value===undefined)throw new Error("Selected binding path is missing");
      const digest = await evidenceDigest(loaded);
      return {
        binding,
        value,
        evidence: {
          id: `${options.kind}:${binding.source}`,
          kind: options.kind,
          schema: binding.schema ?? options.schema ?? "application/json",
          digest,
          authority: options.authority ?? (options.kind==="fixture"?"derived":"untrusted"),
          freshness: "current",
          completeness: "complete",
          exactness: "exact",
        },
      };
    },
  };
}
