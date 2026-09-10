import type { ExtensionDescriptor, RendererDescriptor, VisualDefinition } from "@synth/visuals-protocol";

function validateExtension(extension:{id:string;version:string;protocolRange?:string}):void{
  if(!extension.id?.trim()||!extension.version?.trim())throw new Error("Extension identity and version are required");
  if(extension.protocolRange!==undefined&&!["*","1","1.x","^1"].includes(extension.protocolRange))throw new Error(`Unsupported visuals protocol range ${extension.protocolRange}`);
}

export class ExtensionRegistry<T extends ExtensionDescriptor> {
  readonly #kind: string;
  readonly #extensions = new Map<string, T>();

  constructor(kind: string) { this.#kind = kind; }

  register(extension: T): void {
    validateExtension(extension);
    if (!extension.id.trim() || !extension.version.trim()) throw new Error(`${this.#kind} identity and version are required`);
    if (this.#extensions.has(extension.id)) throw new Error(`${this.#kind} ${extension.id} is already registered`);
    this.#extensions.set(extension.id, Object.freeze(structuredClone(extension)));
  }

  get(id: string): T | undefined { const value=this.#extensions.get(id);return value?structuredClone(value):undefined; }
  require(id: string): T {
    const extension = this.get(id);
    if (!extension) throw new Error(`Unknown ${this.#kind.toLowerCase()} ${id}`);
    return extension;
  }
  list(): T[] { return structuredClone([...this.#extensions.values()]).sort((a, b) => a.id.localeCompare(b.id)); }
}

export class VisualExtensionRegistry {
  readonly #definitions = new Map<string, VisualDefinition>();
  readonly #renderers = new Map<string, RendererDescriptor>();

  registerDefinition(definition: VisualDefinition): void {
    validateExtension(definition);
    const existing = this.#definitions.get(definition.id);
    if (existing) throw new Error(`Visual definition ${definition.id} is already registered at ${existing.version}`);
    if (!this.#renderers.has(definition.renderer)) {
      throw new Error(`Visual definition ${definition.id} references unknown renderer ${definition.renderer}`);
    }
    this.#definitions.set(definition.id, structuredClone(definition));
  }

  registerRenderer(renderer: RendererDescriptor): void {
    validateExtension(renderer);
    const existing = this.#renderers.get(renderer.id);
    if (existing) throw new Error(`Renderer ${renderer.id} is already registered at ${existing.version}`);
    this.#renderers.set(renderer.id, structuredClone(renderer));
  }

  definition(id: string): VisualDefinition | undefined { const value=this.#definitions.get(id);return value?structuredClone(value):undefined; }
  renderer(id: string): RendererDescriptor | undefined { const value=this.#renderers.get(id);return value?structuredClone(value):undefined; }
  definitions(): VisualDefinition[] { return structuredClone([...this.#definitions.values()]).sort((a, b) => a.id.localeCompare(b.id)); }
  renderers(): RendererDescriptor[] { return structuredClone([...this.#renderers.values()]).sort((a, b) => a.id.localeCompare(b.id)); }
}
