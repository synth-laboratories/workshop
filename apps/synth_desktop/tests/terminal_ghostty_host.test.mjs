import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const desktop = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (rel) => readFileSync(join(desktop, rel), "utf8");

test("desktop terminals use the pinned native Ghostty host with a browser fallback", () => {
	const packageManifest = read("src-tauri/ghostty-host/Package.swift");
	const swiftHost = read("src-tauri/ghostty-host/Sources/SynthGhosttyHost/SynthGhosttyHost.swift");
	const rustTerminal = read("src-tauri/src/terminal.rs");
	const panel = read("src/renderer/src/components/TerminalPanel.tsx");
	const bundle = JSON.parse(read("src-tauri/tauri.conf.json"));

	assert.match(packageManifest, /libghostty-spm\.git/);
	assert.match(packageManifest, /exact: "1\.5\.2"/);
	assert.match(packageManifest, /\.product\(name: "GhosttyTerminal"/);
	assert.match(swiftHost, /backend: \.inMemory\(session\)/);
	assert.match(swiftHost, /@_cdecl\("synth_ghostty_host_create"\)/);
	assert.match(swiftHost, /parent\.isFlipped/);
	assert.match(rustTerminal, /fn feed_ghostty/);
	assert.match(rustTerminal, /surface\.receive\(&bytes\)/);
	assert.match(panel, /mountNative/);
	assert.match(panel, /mounted \? "ghostty" : "xterm"/);
	assert.deepEqual(bundle.bundle.macOS.frameworks, ["generated-frameworks/libSynthGhosttyHost.dylib"]);
});

test("terminal actions belong to the tab strip", () => {
	const panel = read("src/renderer/src/components/TerminalPanel.tsx");
	const css = read("src/renderer/src/styles/app.css");

	assert.match(panel, /viewBox="0 0 16 16"/);
	assert.match(panel, /strokeWidth="1\.6"/);
	assert.match(panel, /className="terminal-tab-close"/);
	assert.match(panel, /className="terminal-tab-add"/);
	assert.match(panel, /<TerminalTabIcon kind="new"/);
	assert.match(panel, /<TerminalTabIcon kind="close"/);
	assert.doesNotMatch(panel, /className="terminal-actions"/);
	assert.doesNotMatch(panel, /aria-label="Hide terminal"/);
	assert.match(css, /\.terminal-tab-close, \.terminal-tab-add \{[^}]*height: 34px;/);
	assert.match(css, /\.terminal-tab-action-icon \{[^}]*width: 12px;[^}]*height: 12px;/);
});

test("hiding the terminal destroys the native surface without closing its session", () => {
	const app = read("src/renderer/src/App.tsx");
	const panel = read("src/renderer/src/components/TerminalPanel.tsx");

	assert.match(app, /bottomPanel=\{c\.terminalOpen \? \(/);
	assert.match(panel, /if \(disposed\) \{[\s\S]*if \(mounted\) void bridges\.terminal\.unmountNative\?\.\(terminalId\)/);
	assert.match(panel, /return \(\) => \{[\s\S]*unmountNative\?\.\(terminalId\)/);
	assert.doesNotMatch(panel, /return \(\) => \{[\s\S]*setNativeVisible\?\.\(terminalId, false\)/);
});
