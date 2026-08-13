import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const appCss = "src/renderer/src/styles/app.css";
const cwd = fileURLToPath(new URL("..", import.meta.url));

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
	{ name: "hex color", pattern: /#[0-9a-f]{3,8}\b/i },
	{ name: "bare font-size", pattern: /\bfont-size\s*:\s*(?!var\()/i },
	{ name: "bare border-radius", pattern: /\bborder-radius\s*:\s*(?!var\()/i }
];
const failures = [];

for (const line of addedLines(diff)) {
	for (const rule of rules) {
		if (rule.pattern.test(line)) failures.push(`${rule.name}: ${line.trim()}`);
	}
}

if (failures.length) {
	console.error(`app.css may not add style literals; use tokens instead:\n${failures.map((failure) => `- ${failure}`).join("\n")}`);
	process.exit(1);
}

console.log("app.css added lines use tokens for color, type size, and radius");
