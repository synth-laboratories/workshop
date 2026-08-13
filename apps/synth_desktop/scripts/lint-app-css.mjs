import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const appCss = "src/renderer/src/styles/app.css";
const cwd = fileURLToPath(new URL("..", import.meta.url));
const baseline = JSON.parse(readFileSync(new URL("./app-css-debt-baseline.json", import.meta.url), "utf8"));

function git(args) {
	return execFileSync("git", args, { cwd, encoding: "utf8" });
}

function addedLines(diff) {
	return diff
		.split("\n")
		.filter((line) => line.startsWith("+") && !line.startsWith("+++"))
		.map((line) => line.slice(1));
}

let diff = git(["diff", "--no-ext-diff", "--unified=0", "HEAD", "--", appCss]);
if (!diff.trim()) {
	try {
		diff = git(["diff", "--no-ext-diff", "--unified=0", "HEAD^", "HEAD", "--", appCss]);
	} catch {
		diff = "";
	}
}

const rules = [
	{ key: "hexColors", name: "hex color", pattern: /#[0-9a-f]{3,8}\b/gi },
	{ key: "bareFontSizes", name: "bare font-size", pattern: /\bfont-size\s*:\s*(?!var\()[^;}]+/gi },
	{ key: "bareBorderRadii", name: "bare border-radius", pattern: /\bborder-radius\s*:\s*(?!var\()[^;}]+/gi }
];
const failures = [];

const source = readFileSync(new URL(`../${appCss}`, import.meta.url), "utf8");
for (const rule of rules) {
	const count = source.match(rule.pattern)?.length ?? 0;
	if (count > baseline[rule.key]) {
		failures.push(`${rule.name} debt increased: ${baseline[rule.key]} -> ${count}`);
	}
}

for (const line of addedLines(diff)) {
	for (const rule of rules) {
		rule.pattern.lastIndex = 0;
		if (rule.pattern.test(line)) failures.push(`${rule.name}: ${line.trim()}`);
	}
}

if (failures.length) {
	console.error(`app.css may not add style literals; use tokens instead:\n${failures.map((failure) => `- ${failure}`).join("\n")}`);
	process.exit(1);
}

console.log("app.css style-literal debt did not increase; added lines use tokens");
