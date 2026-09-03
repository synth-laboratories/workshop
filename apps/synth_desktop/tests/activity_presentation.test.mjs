import assert from "node:assert/strict";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { transformSync } from "esbuild";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });

const source = join(appRoot, "src/renderer/src/preferences/activityPresentation.ts");
const compiled = join(compiledDir, "activityPresentation.mjs");
writeFileSync(compiled, transformSync(readFileSync(source, "utf8"), {
	loader: "ts",
	format: "esm",
	target: "es2022",
	sourcefile: source
}).code);

const { pairActivityGroupLines, presentActivityLines } = await import(pathToFileURL(compiled).href);

const thought = (id) => ({ id, label: "Thought", kind: "thought", detail: "Checking the workspace" });
const command = (id, toolStatus = "completed") => ({
	id,
	label: "Run Shell Command",
	kind: "command",
	detail: "pwd",
	toolStatus
});

test("grouped mode folds consecutive calls and their intervening thoughts", () => {
	const lines = [thought("t1"), command("c1"), thought("t2"), command("c2", "running"), thought("t3")];
	const presented = presentActivityLines(lines, "grouped", { running: true });

	assert.equal(presented.length, 1);
	assert.deepEqual(presented[0], {
		kind: "group",
		id: "group-t1",
		label: "Ran commands",
		summary: "2 calls",
		count: 2,
		status: "running",
		lines,
		expanded: false
	});
});

test("active activity tail renders the latest token total as a compact value", () => {
	const lines = [command("c1"), { ...command("c2", "running"), tokenTotal: 12_345 }];
	const [group] = presentActivityLines(lines, "grouped", { running: true });
	assert.equal(group.kind, "group");
	assert.equal(group.summary, "2 calls · 12.3K tokens");
});

test("mixed tool runs receive a concise Codex-style action label", () => {
	const lines = [
		{ id: "w1", label: "Wrote", kind: "file_write", path: "/tmp/a.ts", toolStatus: "completed" },
		thought("t1"),
		{ id: "r1", label: "Read", kind: "file_read", path: "/tmp/a.ts", toolStatus: "completed" },
		command("c1")
	];
	const [group] = presentActivityLines(lines, "grouped");

	assert.equal(group.kind, "group");
	assert.equal(group.label, "Edited files, read files, ran commands");
	assert.equal(group.summary, "3 calls");
	assert.equal(group.status, "completed");
});

test("expansion restores every event in its original order", () => {
	const lines = [command("c1"), thought("t1"), command("c2")];
	const [group] = presentActivityLines(lines, "grouped", {
		expandedGroupIds: new Set(["group-c1"])
	});

	assert.equal(group.kind, "group");
	assert.equal(group.expanded, true);
	assert.deepEqual(group.lines.map((line) => line.id), ["c1", "t1", "c2"]);
});

test("a single tool call stays inline and summary rows split groups", () => {
	const summary = { id: "done", label: "Completed", kind: "run_summary" };
	const lines = [command("c1"), summary, command("c2"), thought("t2"), command("c3")];
	const presented = presentActivityLines(lines, "grouped");

	assert.equal(presented[0].kind, "line");
	assert.equal(presented[0].line.id, "c1");
	assert.equal(presented[1].kind, "line");
	assert.equal(presented[1].line.id, "done");
	assert.equal(presented[2].kind, "group");
	assert.deepEqual(presented[2].lines.map((line) => line.id), ["c2", "t2", "c3"]);
});

test("grouped mode keeps lifecycle milestones visible and folds visual plumbing", () => {
	const lines = [
		{ id: "c1", label: "synth_containers.container_list", kind: "working", toolStatus: "completed" },
		{ id: "c2", label: "synth_containers.container_register", kind: "working", toolStatus: "completed", containerId: "ctr_craftax" },
		command("sh1"),
		command("sh2"),
		{
			id: "v1",
			label: "Visual draft created",
			kind: "visual_lifecycle",
			visualStage: "draft",
			toolStatus: "completed",
			artifactId: "vis_w1_craftax",
			detail: "template_id live.craftax.v1"
		},
		{ id: "v2", label: "synth_visuals.visual_show", kind: "visual", toolStatus: "completed", artifactId: "vis_w1_craftax" },
		{ id: "v3", label: "synth_visuals.visual_authoring_context", kind: "visual", toolStatus: "completed", artifactId: "vis_w1_craftax" }
	];
	const presented = presentActivityLines(lines, "grouped");

	assert.equal(presented[0].kind, "line");
	assert.equal(presented[0].line.id, "c1");
	assert.equal(presented[1].kind, "line");
	assert.equal(presented[1].line.id, "c2");
	assert.equal(presented[2].kind, "group");
	assert.equal(presented[2].label, "Ran commands");
	assert.equal(presented[2].summary, "2 calls");
	assert.equal(presented[3].kind, "line");
	assert.equal(presented[3].line.id, "v1");
	assert.equal(presented[4].kind, "group");
	assert.equal(presented[4].label, "Used visuals");
	assert.deepEqual(presented[4].lines.map((line) => line.id), ["v2", "v3"]);
});

test("compact mode keeps a visual lifecycle milestone while folding visual calls", () => {
	const lines = [
		{ id: "c1", label: "synth_containers.container_list", kind: "working", toolStatus: "completed" },
		command("sh1"),
		command("sh2"),
		{
			id: "v1",
			label: "Visual review",
			kind: "visual_lifecycle",
			visualStage: "review",
			toolStatus: "completed",
			artifactId: "vis_w1_craftax"
		},
		{ id: "v2", label: "synth_visuals.visual_show", kind: "visual", toolStatus: "completed", artifactId: "vis_w1_craftax" },
		{ id: "v3", label: "synth_visuals.visual_authoring_context", kind: "visual", toolStatus: "completed", artifactId: "vis_w1_craftax" }
	];
	const presented = presentActivityLines(lines, "compact");

	assert.equal(presented[0].kind, "line");
	assert.equal(presented[0].line.id, "c1");
	assert.equal(presented[1].kind, "group");
	assert.equal(presented[1].label, "Ran commands");
	assert.equal(presented[2].kind, "line");
	assert.equal(presented[2].line.id, "v1");
	assert.equal(presented[3].kind, "group");
	assert.equal(presented[3].label, "Used visuals");
});

test("detailed mode keeps every event inline", () => {
	const lines = [command("c1"), thought("t1"), command("c2")];
	const presented = presentActivityLines(lines, "detailed");

	assert.deepEqual(presented.map((item) => item.line.id), ["c1", "t1", "c2"]);
});

test("expanded groups pair preceding thought with its next tool call", () => {
	const rows = pairActivityGroupLines([command("c0"), thought("t1"), command("c1"), thought("t2")]);

	assert.deepEqual(rows.map((row) => ({
		context: row.context.map((line) => line.id),
		action: row.action?.id
	})), [
		{ context: [], action: "c0" },
		{ context: ["t1"], action: "c1" },
		{ context: ["t2"], action: undefined }
	]);
});
