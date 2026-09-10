import type {
  BindingDescriptor,
  Diagnostic,
  EvidenceGraph,
  ExtensionDescriptor,
  JsonValue,
  ResolvedSource,
} from "@synth/visuals-protocol";

export type ResolveRequest = { binding: BindingDescriptor; visualId?: string; revision?: number };

export interface BindingResolver<T = JsonValue> extends ExtensionDescriptor {
  kind: string;
  resolve(request: ResolveRequest, signal: AbortSignal): Promise<ResolvedSource<T>>;
  subscribe?(request: ResolveRequest, signal: AbortSignal): AsyncIterable<ResolvedSource<T>>;
}

export type ResolutionResult = {
  sources: ResolvedSource[];
  evidence: EvidenceGraph;
  diagnostics: Diagnostic[];
};

export class BindingResolverRegistry {
  readonly #resolvers = new Map<string, BindingResolver>();

  register(resolver: BindingResolver): void {
    if (!resolver.kind.trim()) throw new Error("Binding resolver kind is required");
    if (this.#resolvers.has(resolver.kind)) throw new Error(`Binding resolver for ${resolver.kind} is already registered`);
    this.#resolvers.set(resolver.kind, resolver);
  }

  get(kind: string): BindingResolver | undefined { return this.#resolvers.get(kind); }
  require(kind: string): BindingResolver {
    const resolver = this.get(kind);
    if (!resolver) throw new Error(`No registered binding resolver for ${kind}`);
    return resolver;
  }
  list(): BindingResolver[] { return [...this.#resolvers.values()].sort((a, b) => a.kind.localeCompare(b.kind)); }
}

export class BindingEngine {
  readonly registry: BindingResolverRegistry;
  constructor(registry: BindingResolverRegistry) { this.registry = registry; }

  async resolve(bindings: BindingDescriptor[], options: { visualId?: string; revision?: number; signal?: AbortSignal } = {}): Promise<ResolutionResult> {
    const controller = options.signal ? undefined : new AbortController();
    const signal = options.signal ?? controller!.signal;
    const sources: ResolvedSource[] = [];
    const diagnostics: Diagnostic[] = [];
    for (const binding of bindings) {
      if (signal.aborted) throw signal.reason ?? new DOMException("Binding resolution aborted", "AbortError");
      try {
        const resolved = await this.registry.require(binding.kind).resolve({ binding, visualId: options.visualId, revision: options.revision }, signal);
        signal.throwIfAborted();
        if (resolved.binding.input !== binding.input || resolved.binding.kind !== binding.kind) {
          throw new Error(`Resolver ${binding.kind} changed binding identity`);
        }
        sources.push(resolved as ResolvedSource);
        diagnostics.push(...(resolved.evidence.diagnostics ?? []));
      } catch (reason) {
        if(signal.aborted)throw signal.reason??reason;
        diagnostics.push({
          code: "visual.binding.resolve_failed",
          severity: "error",
          message: reason instanceof Error ? reason.message : String(reason),
          evidence: binding.source ? [binding.source] : undefined,
        });
      }
    }
    return { sources, evidence: { nodes: sources.map((source) => source.evidence), edges: [] }, diagnostics };
  }
}
