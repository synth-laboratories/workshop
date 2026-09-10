/** Display names for optimizer algorithm ids. Shared by chrome and node tests. */
export function algorithmLabel(id: string): string {
  if (id === "gepa") return "GEPA";
  if (id === "go-ex") return "GELO";
  if (id === "sft") return "SFT";
  if (id === "cispo") return "CISPO";
  return id;
}
