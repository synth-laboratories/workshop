import type { ObservationContract, VisualDefinition, VisualInputContract } from "@synth/visuals-protocol";

type LegacyInput = { name?: string; accepts?: string[]; required?: boolean; multiple?: boolean; schema?: string };
type LegacyTemplate = {
  id?: string;
  version?: string;
  rendererKind?: string;
  inputs?: LegacyInput[];
  slots?: LegacyInput[];
  observationContract?: ObservationContract;
};

/** v0.10 compatibility adapter. It converts a bundled Workshop manifest into
 * a core definition without moving Workshop meaning into the engine. */
export function definitionFromWorkshopTemplate(template: LegacyTemplate): VisualDefinition {
  if (!template.id) throw new Error("Workshop visual template requires an id");
  const sourceInputs = template.inputs ?? template.slots ?? [];
  const inputs: VisualInputContract[] = sourceInputs.map((input) => {
    if (!input.name) throw new Error(`Template ${template.id} has an unnamed input`);
    return {
      name: input.name,
      schemas: input.schema ? [input.schema] : [],
      bindingKinds: input.accepts ?? [],
      required: input.required !== false,
      multiple: input.multiple,
    };
  });
  return {
    id: template.id,
    version: template.version ?? "1.0.0",
    renderer: template.rendererKind ?? "workshop.component",
    // Session-level capabilities come from the mounted engine host. Do not
    // infer rich semantic interaction from a legacy observation declaration.
    capabilities: ["snapshot", "recording", "mcp"],
    inputSchemas: [...new Set(inputs.flatMap((input) => input.schemas))],
    inputs,
    observation: template.observationContract,
  };
}
