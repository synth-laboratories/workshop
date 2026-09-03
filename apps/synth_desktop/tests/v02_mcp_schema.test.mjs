import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const tools = JSON.parse(
	readFileSync(join(dirname(fileURLToPath(import.meta.url)), "../../../visuals/mcp/tools.json"), "utf8")
);

const ADVERTISED = [
	"visual_manage",
	"visual_list_templates",
	"visual_list",
	"visual_get",
	"visual_create",
	"visual_create_from_template",
	"visual_update",
	"visual_bind_data_source",
	"visual_authoring_context",
	"visual_review",
	"visual_capture_review",
	"visual_chart",
	"visual_mark_ready",
	"visual_open_in_pane",
	"visual_show",
	"visual_fork",
	"visual_archive"
];

test("v0.2 Visuals MCP dump advertises the implemented bin surface", () => {
	const names = tools.tools.map((tool) => tool.name);
	assert.deepEqual([...names].sort(), [...ADVERTISED].sort());
	assert.ok(!names.includes("resources/list"));
	assert.ok(!names.includes("resources/read"));
	assert.ok(!names.includes("resources/templates/list"));
	assert.ok(names.includes("visual_manage"));
	assert.ok(names.includes("visual_capture_review"));
	assert.ok(!names.includes("visual_stream_live_eval"));
	assert.ok(!names.includes("shell"));
});

test("v0.2 visual_create_from_template rejects unknown properties", () => {
	const create = tools.tools.find((tool) => tool.name === "visual_create_from_template");
	assert.equal(create.inputSchema.additionalProperties, false);
	assert.ok(create.inputSchema.required.includes("template_id"));
});

test("v0.2 visual_bind_data_source does not accept a guessed events slot name as schema enum", () => {
	const bind = tools.tools.find((tool) => tool.name === "visual_bind_data_source");
	assert.deepEqual(bind.inputSchema.properties.kind.enum, ["trace_v5", "local_cas", "live_sse", "fixture"]);
	assert.equal(bind.inputSchema.additionalProperties, false);
});
