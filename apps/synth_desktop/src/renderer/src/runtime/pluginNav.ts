/**
 * The Plugins section's contents.
 *
 * Built-in surfaces are declared here rather than read from the plugin
 * registry: the registry knows only `optimizers`, and `plugins_list` returns a
 * single element, so a section sourced from it would render one row. Adding a
 * second managed plugin is one more entry — not another sidebar redesign.
 */

export type PluginNavKind = "builtin" | "managed";

export type PluginNavEntry = {
	id: "visuals" | "reports" | "experiments" | "optimizers" | "inventory" | "inference" | "computer-use";
	/** Sidebar destination name. Short: a place, not a description of contents. */
	label: string;
	description: string;
	testId: string;
	kind: PluginNavKind;
	/** Registry id, managed entries only. */
	pluginId?: string;
};

export const PLUGIN_NAV: readonly PluginNavEntry[] = [
	{ id: "visuals", label: "Visuals", description: "Explore agent- and user-created visual workspaces.", testId: "open-visuals", kind: "builtin" },
	{ id: "reports", label: "Reports", description: "Collect durable narrative and evidence reports.", testId: "open-reports", kind: "builtin" },
	{ id: "experiments", label: "Experiments", description: "Review evaluation and training experiments.", testId: "open-experiments", kind: "builtin" },
	{ id: "optimizers", label: "Optimizers", description: "Configure and inspect managed optimization runs.", testId: "open-optimizers", kind: "managed", pluginId: "optimizers" },
	{ id: "inventory", label: "Data", description: "Inspect containers, traces, visuals, and usage records.", testId: "open-inventory", kind: "builtin" },
	{ id: "inference", label: "Local Inference", description: "Monitor the local model runtime and requests.", testId: "open-inference", kind: "builtin" },
	// Managed like Optimizers, but its lifecycle is human-only: the agent can
	// read this plugin's status and nothing else. See docs/COMPUTER_USE.md §4.
	{
		id: "computer-use",
		label: "Computer Use",
		description: "Control approved desktop applications with auditable actions.",
		testId: "open-computer-use",
		kind: "managed",
		pluginId: "computer-use"
	}
] as const;
