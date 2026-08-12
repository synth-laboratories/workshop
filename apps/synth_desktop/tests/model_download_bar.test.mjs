import assert from "node:assert/strict";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { transformSync } from "esbuild";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const source = join(appRoot, "src/renderer/src/components/ModelDownloadBar.tsx");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });
const compiled = join(compiledDir, "ModelDownloadBar.mjs");
writeFileSync(
	compiled,
	transformSync(readFileSync(source, "utf8"), {
		loader: "tsx",
		jsx: "automatic",
		format: "esm",
		target: "es2022",
		sourcefile: source
	}).code
);

const { ModelDownloadBar } = await import(pathToFileURL(compiled).href);

function state(selectedTargetId, status, detail = "invalid local model path") {
	return {
		selectedTargetId,
		model: { status, name: "Laguna-XS-2.1", detail }
	};
}

test("local model errors remain visible for the local target", () => {
	const html = renderToStaticMarkup(
		createElement(ModelDownloadBar, {
			state: state("local-laguna", "error"),
			onPauseToggle() {}
		})
	);
	assert.match(html, /model-status-error/);
	assert.match(html, /invalid local model path/);
});

test("local model errors do not pollute a Synth Cloud session", () => {
	const html = renderToStaticMarkup(
		createElement(ModelDownloadBar, {
			state: state("synth-cloud-laguna-s", "error"),
			onPauseToggle() {}
		})
	);
	assert.equal(html, "");
});

test("an explicit local model download remains visible in another session", () => {
	const downloading = state("synth-cloud-laguna-s", "downloading", null);
	downloading.model.downloadProgress = 42;
	const html = renderToStaticMarkup(
		createElement(ModelDownloadBar, {
			state: downloading,
			onPauseToggle() {}
		})
	);
	assert.match(html, /model-download-bar/);
	assert.match(html, /42%/);
});
