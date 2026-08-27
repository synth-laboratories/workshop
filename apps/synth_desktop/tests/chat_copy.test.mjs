import assert from "node:assert/strict";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { transformSync } from "esbuild";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const source = join(appRoot, "src/renderer/src/runtime/chatCopy.ts");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
const compiled = join(compiledDir, "chatCopy.mjs");
mkdirSync(compiledDir, { recursive: true });
writeFileSync(compiled, transformSync(readFileSync(source, "utf8"), {
	loader: "ts",
	format: "esm",
	target: "es2022",
	sourcefile: source
}).code);

const { conversationMarkdown } = await import(pathToFileURL(compiled).href);

test("chat copy preserves visible turns and their roles", () => {
	assert.equal(conversationMarkdown("A useful chat", [
		{ id: "u", role: "user", body: "  Start here.  ", at: "now" },
		{ id: "a", role: "assistant", body: "Result\n\n```ts\nconst ok = true;\n```", at: "later" }
	]), "# A useful chat\n\n## User\n\nStart here.\n\n## Assistant\n\nResult\n\n```ts\nconst ok = true;\n```");
});

test("chat copy omits empty transport messages without inventing content", () => {
	assert.equal(conversationMarkdown("", [
		{ id: "empty", role: "assistant", body: "  ", at: "now" },
		{ id: "system", role: "system", body: "Policy", at: "later" }
	]), "# Untitled chat\n\n## System\n\nPolicy");
});
