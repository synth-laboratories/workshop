export function compactModelLabel(label: string): string {
	const leaf = label.trim().replace(/^.*\//, "");
	const compact = leaf
		.replace(/^GPT[- ]?\d+(?:\.\d+)?\s*/i, "")
		.replace(/^Laguna[-_\s]+/i, "")
		.replace(/^xs[-_\s]+/i, "XS ")
		.replace(/^Select model$/i, "Model")
		.replace(/^offline$/i, "Offline")
		.replace(/^starting…?$/i, "Starting");
	return compact.length <= 12 ? compact : `${compact.slice(0, 11)}…`;
}
