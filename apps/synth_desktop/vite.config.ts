import { readFile, readdir } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";
import { parseLiveGepaJsonl } from "./liveGepaJsonl";

function liveGepaQaEvents() {
	return {
		name: "live-gepa-qa-events",
		configureServer(server: { middlewares: { use: (path: string, handler: (request: unknown, response: { statusCode: number; setHeader: (name: string, value: string) => void; end: (body?: string) => void }) => void) => void } }) {
			server.middlewares.use("/api/gepa-events", async (_request, response) => {
				const path = process.env.SYNTH_GEPA_QA_EVENTS_PATH;
				if (!path) {
					response.statusCode = 404;
					response.end(JSON.stringify({ error: "SYNTH_GEPA_QA_EVENTS_PATH is not configured" }));
					return;
				}
				try {
					const text = await readFile(path, "utf8");
					const events = parseLiveGepaJsonl(text);
					const sourceLast: Record<string, any> = events.at(-1) ?? {};
					const runDir = dirname(path);
					const workspacesDir = join(runDir, "proposer_workspaces");
					const maxSequence = events.reduce<number>((max, event) => Math.max(max, Number(event.sequence_number ?? event.sequenceNumber ?? 0)), 0);
					const bounded = (value: unknown, limit = 20_000) => {
						const raw = typeof value === "string" ? value : value == null ? "" : JSON.stringify(value, null, 2);
						return raw.length <= limit ? raw : `${raw.slice(0, limit)}\n… truncated in projection (${raw.length} chars; sealed artifact retains the complete value)`;
					};
					try {
						const generations = (await readdir(workspacesDir, { withFileTypes: true }))
							.filter((entry) => entry.isDirectory() && /^generation_\d+$/.test(entry.name))
							.sort((a, b) => a.name.localeCompare(b.name));
						for (const [generationIndex, entry] of generations.entries()) {
							const generation = Number(entry.name.slice("generation_".length));
							const source = join(workspacesDir, entry.name, ".agent_artifacts", "opencode_sse_events.jsonl");
							let sourceText: string;
							try { sourceText = await readFile(source, "utf8"); } catch { continue; }
							const projectedItems: Array<Record<string, unknown>> = [];
							for (const line of sourceText.split(/\r?\n/).filter(Boolean)) {
								let envelope: Record<string, any>;
								try { envelope = JSON.parse(line); } catch { continue; }
								if (envelope.method !== "item/completed") continue;
								const item = envelope.params?.item ?? {};
								const at = typeof envelope.emittedAtMs === "number" ? new Date(envelope.emittedAtMs).toISOString() : undefined;
								const base = { id: String(item.id ?? `g${generation}-item-${projectedItems.length}`), sequence: projectedItems.length + 1, occurredAt: at, status: item.status };
								if (item.type === "userMessage") {
									projectedItems.push({ ...base, family: "input", kind: "message.input", title: "GEPA proposer request", body: bounded((item.content ?? []).map((part: any) => part.text ?? "").join("\n")) });
								} else if (item.type === "agentMessage" && item.text) {
									const final = item.phase === "final_answer";
									projectedItems.push({ ...base, family: final ? "output" : "thinking", kind: final ? "message.output" : "reasoning.summary", title: final ? "Proposer response" : "Reasoning summary", body: bounded(item.text) });
								} else if (item.type === "commandExecution") {
									projectedItems.push({ ...base, family: "tool", kind: "tool.shell", title: "Run shell command", body: bounded(item.command), detail: bounded(item.aggregatedOutput), status: item.exitCode === 0 ? "completed" : `exit ${item.exitCode ?? "?"}` });
								} else if (item.type === "fileChange") {
									const changes = Array.isArray(item.changes) ? item.changes : [];
									projectedItems.push({ ...base, family: "artifact", kind: "tool.file_change", title: changes.map((change: any) => change.path).filter(Boolean).join(", ") || "Workspace file change", detail: bounded(changes.map((change: any) => `${change.kind?.type ?? "change"} ${change.path ?? ""}\n${change.diff ?? ""}`).join("\n\n")) });
								}
							}
							events.push({
								schema_version: "optimizer_event.v1",
								type: "proposer.trace_v5.loaded",
								sequence_number: maxSequence + generationIndex + 1,
								run_id: events[0]?.run_id ?? events[0]?.optimizer_run_id,
								algorithm_id: "gepa",
								occurred_at: projectedItems.at(-1)?.occurredAt ?? new Date().toISOString(),
								delta: { generation, schema_version: "synth.trace-projection.rollout-inspector.v1", items: projectedItems, source_artifact: source }
							});
						}
					} catch { /* A missing live artifact must not break the optimizer event feed. */ }
					const first: Record<string, any> = events[0] ?? {};
					const terminal = ["optimizer.run.completed", "optimizer.run.failed", "optimizer.run.cancelled", "gepa.run.finished"].includes(String(sourceLast.type ?? ""));
					response.setHeader("content-type", "application/json; charset=utf-8");
					response.setHeader("cache-control", "no-store");
					response.end(JSON.stringify({
						run: {
							id: String(first.run_id ?? first.optimizer_run_id ?? "live-gepa"),
							algorithmId: "gepa",
							status: terminal ? (sourceLast.type === "gepa.run.finished" ? "completed" : String(sourceLast.type).split(".").at(-1) ?? "completed") : "running",
							source: "local-live-run"
						},
						events
					}));
				} catch (error) {
					response.statusCode = 500;
					response.end(JSON.stringify({ error: error instanceof Error ? error.message : String(error) }));
				}
			});
		}
	};
}

export default defineConfig({
	root: resolve("src/renderer"),
	// Parallel Playwright workers each own a Vite server. Their dependency
	// optimizer state must not share a cache; the fixture supplies a private
	// directory while normal development keeps Vite's default cache.
	cacheDir: process.env.SYNTH_DESKTOP_VITE_CACHE_DIR || undefined,
	resolve: {
		alias: {
			"@": resolve("src/renderer/src"),
			"@synth/visuals": resolve("../../visuals/registry/index.ts"),
			"@synth/visual-templates": resolve("../../visuals/templates")
		}
	},
	plugins: [react(), liveGepaQaEvents()],
	clearScreen: false,
	server: {
		port: 1420,
		strictPort: true
	},
	build: {
		outDir: resolve("dist"),
		emptyOutDir: true
	}
});
