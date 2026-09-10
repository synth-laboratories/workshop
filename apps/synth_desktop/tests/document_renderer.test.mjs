/**
 * The document pane's renderer core: markdown and syntax highlighting.
 *
 * Both are written rather than installed (no md dependency exists in this app
 * and none can be added right now), so the behaviour a dependency would have
 * given us for free is the behaviour that has to be pinned here. The one that
 * matters most is the last test: neither module ever produces HTML, so a
 * `<script>` in a README is text by construction and not by sanitizer.
 */
import assert from "node:assert/strict";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { transformSync } from "esbuild";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });

function compile(relative, outName) {
	const source = join(appRoot, relative);
	const compiled = join(compiledDir, outName);
	writeFileSync(
		compiled,
		transformSync(readFileSync(source, "utf8"), {
			loader: "ts",
			format: "esm",
			target: "es2022",
			sourcefile: source
		}).code
	);
	return pathToFileURL(compiled).href;
}

const { parseMarkdown, parseInline, outline, markdownText } = await import(
	compile("src/renderer/src/documents/markdown.ts", "documents-markdown.mjs")
);
const { highlight, isHighlightable } = await import(
	compile("src/renderer/src/documents/highlight.ts", "documents-highlight.mjs")
);

test("headings carry a stable slug the outline can address", () => {
	const blocks = parseMarkdown("# Part VII — Right panel\n\ntext\n\n## Gap table\n");
	assert.equal(blocks[0].type, "heading");
	assert.equal(blocks[0].depth, 1);
	assert.equal(blocks[0].slug, "part-vii-right-panel");
	const headings = outline(blocks);
	assert.deepEqual(headings.map((entry) => entry.depth), [1, 2]);
	assert.equal(headings[1].slug, "gap-table");
});

test("a fenced block keeps its language and its blank lines", () => {
	const blocks = parseMarkdown("intro\n\n```rust\nfn main() {\n\n}\n```\n\nafter\n");
	const code = blocks.find((block) => block.type === "code");
	assert.equal(code.language, "rust");
	assert.equal(code.value, "fn main() {\n\n}");
	assert.equal(blocks[blocks.length - 1].type, "paragraph");
});

test("an unterminated fence runs to the end instead of eating the parse", () => {
	const blocks = parseMarkdown("```python\nprint(1)\n");
	assert.equal(blocks.length, 1);
	assert.equal(blocks[0].type, "code");
	assert.equal(blocks[0].value, "print(1)");
});

test("lists carry task state and ordered starts", () => {
	const blocks = parseMarkdown("- [x] done\n- [ ] pending\n- plain\n");
	assert.equal(blocks[0].type, "list");
	assert.equal(blocks[0].ordered, false);
	assert.deepEqual(blocks[0].items.map((item) => item.checked), [true, false, null]);

	const ordered = parseMarkdown("3. three\n4. four\n");
	assert.equal(ordered[0].ordered, true);
	assert.equal(ordered[0].start, 3);
});

test("a GFM table keeps its alignment row out of the body", () => {
	const blocks = parseMarkdown("| a | b |\n| :- | -: |\n| 1 | 2 |\n");
	const table = blocks[0];
	assert.equal(table.type, "table");
	assert.deepEqual(table.align, ["left", "right"]);
	assert.equal(table.rows.length, 1);
	assert.equal(markdownText([table]).includes("1 2"), true);
});

test("emphasis takes the longest marker and leaves lone asterisks alone", () => {
	const [strong] = parseInline("***both***");
	assert.equal(strong.type, "strong");
	const arithmetic = parseInline("2 * 3 * 4");
	assert.deepEqual(arithmetic.map((node) => node.type), ["text"]);
	assert.equal(arithmetic[0].value, "2 * 3 * 4");
});

test("inline code wins over emphasis inside it", () => {
	const nodes = parseInline("call `a * b` twice");
	assert.deepEqual(nodes.map((node) => node.type), ["text", "code", "text"]);
	assert.equal(nodes[1].value, "a * b");
});

test("links and autolinks resolve, images stay references", () => {
	const [link] = parseInline("[docs](./docs/readme.md)");
	assert.equal(link.type, "link");
	assert.equal(link.href, "./docs/readme.md");
	const [auto] = parseInline("<https://example.com/x>");
	assert.equal(auto.type, "link");
	const [image] = parseInline("![alt](./x.png)");
	assert.equal(image.type, "image");
	assert.equal(image.src, "./x.png");
});

test("a quote survives lazy continuation", () => {
	const blocks = parseMarkdown("> first\ncontinued\n\nafter\n");
	assert.equal(blocks[0].type, "quote");
	assert.equal(markdownText(blocks[0].blocks).includes("continued"), true);
});

test("the highlighter names what it can and cannot colour", () => {
	assert.equal(isHighlightable("rust"), true);
	assert.equal(isHighlightable("tsx"), true);
	assert.equal(isHighlightable("brainfuck"), false);
	const plain = highlight("noop noop", "brainfuck");
	assert.deepEqual(plain, [{ kind: "plain", value: "noop noop" }]);
});

test("comments, strings and keywords are separated", () => {
	const tokens = highlight('// note\nlet value = "text";\n', "rust");
	const kinds = tokens.filter((token) => token.kind !== "plain").map((token) => token.kind);
	assert.equal(kinds.includes("comment"), true);
	assert.equal(kinds.includes("keyword"), true);
	assert.equal(kinds.includes("string"), true);
	assert.equal(tokens.map((token) => token.value).join(""), '// note\nlet value = "text";\n');
});

test("an unterminated string stops at its line rather than painting the file", () => {
	const tokens = highlight('let a = "oops\nlet b = 1;\n', "rust");
	const string = tokens.find((token) => token.kind === "string");
	assert.equal(string.value.includes("\n"), false);
	assert.equal(tokens.some((token) => token.kind === "keyword" && token.value === "let"), true);
});

test("a diff colours by line prefix", () => {
	const tokens = highlight("@@ -1 +1 @@\n-old\n+new\n unchanged", "diff");
	const byKind = tokens.filter((token) => token.kind !== "plain").map((token) => token.kind);
	assert.deepEqual(byKind, ["meta", "deleted", "inserted"]);
});

test("every token concatenates back to the original source", () => {
	for (const [source, language] of [
		["def f(x):\n    return x  # note\n", "python"],
		["SELECT * FROM t WHERE a = 'b';", "sql"],
		['{"a": [1, 2, null]}', "json"],
		["FROM node:22\nRUN npm ci\n", "dockerfile"]
	]) {
		assert.equal(highlight(source, language).map((token) => token.value).join(""), source);
	}
});

test("neither module ever emits HTML", () => {
	const hostile = '<script>alert(1)</script>\n\n<img src=x onerror=alert(1)>\n';
	const blocks = parseMarkdown(hostile);
	const flattened = JSON.stringify(blocks);
	// The angle brackets survive as literal text in a `text` node — they are
	// never a tag, and there is no field anywhere that holds markup.
	assert.equal(flattened.includes("<script>"), true);
	assert.equal(blocks.every((block) => block.type === "paragraph"), true);
	assert.equal(markdownText(blocks).includes("<script>alert(1)</script>"), true);

	const tokens = highlight(hostile, "html");
	assert.equal(tokens.map((token) => token.value).join(""), hostile);
	assert.equal(tokens.every((token) => typeof token.value === "string"), true);
});
