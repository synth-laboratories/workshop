#!/usr/bin/env node
// Call or list tools on one Workshop instance's local stdio MCP adapter.
//
// Usage:
//   workshop-mcp.mjs call <instance> <adapter> <tool> '[json arguments]'
//   workshop-mcp.mjs list <instance> <adapter>
//
// Examples:
//   workshop-mcp.mjs call visualqa visuals visual_list '{}'
//   workshop-mcp.mjs call visualqa display workshop_capture \
//     '{"scope":"visual","target":"vis_..."}'
//   workshop-mcp.mjs list visualqa optimizers

import { spawn } from "node:child_process";
import { homedir } from "node:os";
import { join } from "node:path";
import process from "node:process";

const [operation, instance, adapter, tool, rawArgs = "{}"] = process.argv.slice(2);

function usage(message) {
	if (message) process.stderr.write(`${message}\n`);
	process.stderr.write(
		"usage: workshop-mcp.mjs call <instance> <adapter> <tool> '[json arguments]'\n" +
			"       workshop-mcp.mjs list <instance> <adapter>\n"
	);
	process.exit(2);
}

if (!new Set(["call", "list"]).has(operation)) usage("operation must be call or list");
if (!instance || !adapter) usage("instance and adapter are required");
if (!/^[A-Za-z0-9._-]+$/.test(instance) || !/^[A-Za-z0-9_-]+$/.test(adapter)) {
	usage("instance or adapter contains unsupported characters");
}
if (operation === "call" && !tool) usage("tool is required for call");

let args = {};
if (operation === "call") {
	try {
		args = JSON.parse(rawArgs);
	} catch (error) {
		usage(`arguments must be JSON: ${error.message}`);
	}
	if (!args || Array.isArray(args) || typeof args !== "object") {
		usage("arguments must be a JSON object");
	}
}

const root = join(homedir(), ".synth-desktop/instances/v09", instance);
const binary = join(
	root,
	"build/target/debug/bundle/macos",
	`Synth Workshop v0.9 · ${instance}.app`,
	"Contents/MacOS",
	`synth-${adapter}-mcp`
);
const requestId = 2;
const messages = [
	{
		jsonrpc: "2.0",
		id: 1,
		method: "initialize",
		params: {
			protocolVersion: "2024-11-05",
			capabilities: {},
			clientInfo: { name: "workshop-mcp", version: "1" }
		}
	},
	{ jsonrpc: "2.0", method: "notifications/initialized" },
	operation === "list"
		? { jsonrpc: "2.0", id: requestId, method: "tools/list", params: {} }
		: {
				jsonrpc: "2.0",
				id: requestId,
				method: "tools/call",
				params: { name: tool, arguments: args }
			}
];

const child = spawn(binary, {
	env: { ...process.env, SYNTH_DESKTOP_DATA_ROOT: join(root, "data") },
	stdio: ["pipe", "pipe", "pipe"]
});

let stdout = "";
let stderr = "";
child.stdout.on("data", (chunk) => {
	stdout += chunk;
});
child.stderr.on("data", (chunk) => {
	stderr += chunk;
});
child.on("error", (error) => {
	process.stderr.write(`cannot start ${binary}: ${error.message}\n`);
});
child.stdin.write(`${messages.map((message) => JSON.stringify(message)).join("\n")}\n`);
child.stdin.end();

child.on("close", (code) => {
	for (const line of stdout.split("\n")) {
		if (!line.startsWith("{")) continue;
		let message;
		try {
			message = JSON.parse(line);
		} catch {
			continue;
		}
		if (message.id !== requestId) continue;
		if (message.error) {
			process.stderr.write(`${JSON.stringify(message.error, null, 2)}\n`);
			process.exitCode = 1;
			return;
		}
		if (operation === "list") {
			for (const listedTool of message.result?.tools ?? []) {
				const summary = (listedTool.description ?? "").split("\n")[0];
				process.stdout.write(`${listedTool.name}  —  ${summary}\n`);
			}
			return;
		}
		for (const item of message.result?.content ?? []) {
			if (item.type === "text") process.stdout.write(`${item.text}\n`);
			else if (item.type === "image") {
				process.stdout.write(`[image ${item.mimeType ?? "unknown"}; ${item.data?.length ?? 0} base64 characters]\n`);
			} else process.stdout.write(`${JSON.stringify(item)}\n`);
		}
		if (message.result?.isError) process.exitCode = 1;
		return;
	}
	if (stderr) process.stderr.write(stderr.slice(-4000));
	process.exitCode = code === 0 ? 1 : (code ?? 1);
});
