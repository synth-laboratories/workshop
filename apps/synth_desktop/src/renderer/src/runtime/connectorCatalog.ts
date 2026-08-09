export type ConnectorDefinition = {
	id: string;
	name: string;
	description: string;
	category: "Bundled" | "Productivity" | "Development";
	glyph: string;
	bundled?: boolean;
};

/**
 * Presentation registry for the connector catalog. Adding a connector should be
 * a data-only change; transport/auth support can key off the stable id later.
 */
export const CONNECTOR_CATALOG: readonly ConnectorDefinition[] = [
	{ id: "synth-containers", name: "Synth Containers", description: "Register, inspect, probe, and run benchmark containers", category: "Bundled", glyph: "C", bundled: true },
	{ id: "synth-visuals", name: "Synth Visuals", description: "Create, bind, open, and save research visuals", category: "Bundled", glyph: "V", bundled: true },
	{ id: "parallel-search", name: "Parallel Search", description: "Real-time web search and page fetching", category: "Productivity", glyph: "P" },
	{ id: "notion", name: "Notion", description: "Pages, databases, docs, and project context", category: "Productivity", glyph: "N" },
	{ id: "linear", name: "Linear", description: "Issues, projects, cycles, and team workflows", category: "Productivity", glyph: "L" },
	{ id: "github", name: "GitHub", description: "Repositories, issues, pull requests, and reviews", category: "Development", glyph: "G" },
	{ id: "sentry", name: "Sentry", description: "Errors, traces, releases, and issue investigation", category: "Development", glyph: "S" },
	{ id: "vercel", name: "Vercel", description: "Projects, deployments, logs, and documentation", category: "Development", glyph: "▲" },
	{ id: "postgresql", name: "PostgreSQL", description: "Query and inspect application databases", category: "Development", glyph: "Pg" },
	{ id: "cloudflare", name: "Cloudflare", description: "Account configuration and product workflows", category: "Development", glyph: "Cf" }
];

