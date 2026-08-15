/**
 * Mander motion engine + import-boundary checks.
 * Bundle the TypeScript engine with esbuild so node:test can import it.
 */
import assert from "node:assert/strict";
import { mkdirSync, readdirSync, readFileSync } from "node:fs";
import { dirname, extname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { buildSync } from "esbuild";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const renderer = join(appRoot, "src/renderer/src");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });
const compiled = join(compiledDir, "manderMotion.mjs");

buildSync({
	entryPoints: [join(renderer, "components/mander/Mander.motion.ts")],
	bundle: true,
	format: "esm",
	platform: "neutral",
	outfile: compiled,
	logLevel: "silent"
});

const {
	createManderMotion,
	sampleKeyframes
} = await import(pathToFileURL(compiled).href);

const posesUrl = join(compiledDir, "manderPoses.mjs");
buildSync({
	entryPoints: [join(renderer, "components/mander/Mander.poses.ts")],
	bundle: true,
	format: "esm",
	platform: "neutral",
	outfile: posesUrl,
	logLevel: "silent"
});
const { flattenPose, poseDistance, restPose } = await import(pathToFileURL(posesUrl).href);

const transitionsUrl = join(compiledDir, "manderTransitions.mjs");
buildSync({
	entryPoints: [join(renderer, "components/mander/Mander.transitions.ts")],
	bundle: true,
	format: "esm",
	platform: "neutral",
	outfile: transitionsUrl,
	logLevel: "silent"
});
const { transitions, reducedTransitions } = await import(pathToFileURL(transitionsUrl).href);

function createClock() {
	let now = 0;
	let nextId = 1;
	const pending = new Map();
	return {
		now: () => now,
		requestFrame(callback) {
			const id = nextId++;
			pending.set(id, callback);
			return id;
		},
		cancelFrame(id) {
			pending.delete(id);
		},
		advance(ms) {
			now += ms;
			const callbacks = [...pending.values()];
			pending.clear();
			for (const callback of callbacks) callback(now);
		},
		get pendingCount() {
			return pending.size;
		}
	};
}

function walk(dir, files = []) {
	for (const entry of readdirSync(dir, { withFileTypes: true })) {
		const path = join(dir, entry.name);
		if (entry.isDirectory()) walk(path, files);
		else files.push(path);
	}
	return files;
}

function pump(clock, ms, step = 16) {
	const steps = Math.max(1, Math.ceil(ms / step));
	for (let index = 0; index < steps; index += 1) clock.advance(step);
}

test("the 4x4 transition matrix is exhaustive", () => {
	const keys = ["idle", "thinking", "working", "success"].flatMap((from) =>
		["idle", "thinking", "working", "success"].map((to) => `${from}->${to}`)
	);
	assert.deepEqual(Object.keys(transitions).sort(), [...keys].sort());
	assert.deepEqual(Object.keys(reducedTransitions).sort(), [...keys].sort());
	assert.equal(transitions["idle->thinking"].loop, false);
	assert.equal(transitions["thinking->idle"].loop, false);
	assert.equal(transitions["idle->working"].loop, false);
	assert.equal(transitions["working->success"].loop, false);
	assert.equal(transitions["success->idle"].loop, false);
	assert.notEqual(transitions["idle->thinking"].durationMs, transitions["thinking->idle"].durationMs);
	assert.equal(transitions["idle->idle"].loop, true);
	assert.equal(transitions["thinking->thinking"].loop, true);
	assert.equal(transitions["working->working"].loop, true);
	assert.equal(transitions["success->success"].loop, true);
	assert.equal(reducedTransitions["idle->idle"].loop, false);
	assert.equal(reducedTransitions["thinking->thinking"].loop, false);
	assert.equal(reducedTransitions["working->working"].loop, false);
	assert.equal(reducedTransitions["success->success"].loop, false);
});

test("an interrupted transition continues from the currently rendered vector", () => {
	const clock = createClock();
	const engine = createManderMotion({ state: "idle", motion: "full", clock });
	engine.start();
	pump(clock, 16);
	engine.setState("thinking");
	pump(clock, 160);
	const mid = engine.snapshot().pose;
	assert.ok(poseDistance(mid, restPose("idle")) > 0.4, "engage should have moved off idle rest");
	engine.setState("idle");
	const atInterrupt = engine.snapshot().pose;
	assert.deepEqual(flattenPose(atInterrupt), flattenPose(mid));
	pump(clock, 80);
	const after = engine.snapshot().pose;
	assert.notDeepEqual(flattenPose(after), flattenPose(restPose("idle")));
	assert.notDeepEqual(flattenPose(after), flattenPose(restPose("thinking")));
	assert.ok(poseDistance(after, mid) > 0, "the new recipe should move from the interrupted pose");
	pump(clock, 280);
	const settled = engine.snapshot().pose;
	assert.ok(
		poseDistance(settled, restPose("idle")) < poseDistance(mid, restPose("idle")),
		"after the directed settle, the pose should be closer to idle than the interrupt point"
	);
});

test("still mode does not schedule an animation frame", () => {
	const clock = createClock();
	const engine = createManderMotion({ state: "idle", motion: "still", clock });
	engine.start();
	assert.equal(clock.pendingCount, 0);
	assert.equal(engine.running, false);
	engine.setState("thinking");
	assert.equal(clock.pendingCount, 0);
	assert.equal(engine.running, false);
	assert.deepEqual(flattenPose(engine.pose), flattenPose(restPose("thinking")));
	engine.setState("working");
	assert.deepEqual(flattenPose(engine.pose), flattenPose(restPose("working")));
	engine.setState("success");
	assert.deepEqual(flattenPose(engine.pose), flattenPose(restPose("success")));
});

test("reduced mode does not start either loop", () => {
	const clock = createClock();
	const engine = createManderMotion({ state: "idle", motion: "reduced", clock });
	engine.start();
	pump(clock, 16);
	engine.setState("thinking");
	assert.equal(engine.recipeKey, "idle->thinking");
	assert.equal(reducedTransitions[engine.recipeKey].loop, false);
	pump(clock, 220);
	assert.equal(engine.running, false);
	assert.equal(clock.pendingCount, 0);
	engine.setState("idle");
	pump(clock, 220);
	assert.equal(engine.running, false);
	assert.equal(clock.pendingCount, 0);
});

test("sampleKeyframes starts from the provided current pose rather than keyframe zero", () => {
	const current = restPose("thinking");
	const sampled = sampleKeyframes(current, transitions["thinking->idle"].keyframes, 0);
	assert.deepEqual(flattenPose(sampled), flattenPose(current));
});

test("files outside components/mander cannot import renderer internals", () => {
	const forbidden = /Mander\.geometry|Mander\.poses|Mander\.transitions|Mander\.motion|Mander\.figure|useManderMotion/;
	const allowedRoot = join(renderer, "components/mander");
	const offenders = [];
	for (const file of walk(renderer)) {
		if (![".ts", ".tsx", ".js", ".mjs"].includes(extname(file))) continue;
		if (file.startsWith(allowedRoot)) continue;
		const source = readFileSync(file, "utf8");
		if (forbidden.test(source)) offenders.push(file.slice(renderer.length + 1));
	}
	assert.deepEqual(offenders, []);
});
