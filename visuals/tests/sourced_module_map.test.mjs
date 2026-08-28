/**
 * The sourced allowlist and the sourced module map are one contract.
 *
 * `sourcedValidate.ts` decides which specifiers a sourced or user template may
 * import; `sourcedVisual.ts` decides what each specifier resolves to at
 * runtime. When those two disagree the failure is silent in the worst
 * direction: the import validates, `require` hands back a module missing the
 * name, and the component is `undefined` at render. That is exactly what
 * `@synth/visuals/chrome` did — allowlisted whole, resolved to a hand-written
 * `{ VisualChrome }` that omitted `MetricStrip`.
 *
 * `sourcedVisual.ts` cannot be imported here: it pulls in `.tsx` modules that
 * `node --experimental-strip-types` will not load. So this reads the two files
 * and asserts the structure that makes the drift impossible — one entry per
 * allowlisted specifier, and every entry a whole module namespace rather than
 * a curated subset of its exports.
 */
import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const validateSource = readFileSync(join(root, "runtime/sourcedValidate.ts"), "utf8");
const visualSource = readFileSync(join(root, "runtime/sourcedVisual.ts"), "utf8");

function block(source, declaration, open, close) {
  const start = source.indexOf(declaration);
  assert.notEqual(start, -1, `${declaration} not found`);
  const from = source.indexOf(open, start);
  const to = source.indexOf(close, from);
  assert.ok(to > from, `${declaration} is not a ${open}…${close} literal`);
  return source.slice(from + 1, to);
}

const allowlist = [
  ...block(validateSource, "export const SOURCED_ALLOWED_IMPORTS", "[", "]").matchAll(
    /"([^"]+)"/g
  )
].map((match) => match[1]);

const moduleMap = new Map(
  [
    ...block(visualSource, "const SOURCED_MODULE_SOURCES", "{", "\n};").matchAll(
      /^\s*(?:"([^"]+)"|([A-Za-z_$][\w$]*))\s*:\s*([^,\n]+),?$/gm
    )
  ].map((match) => [match[1] ?? match[2], match[3].trim()])
);

/** Identifier -> specifier, for every `import * as X from "…"` in the file. */
const namespaceImports = new Map(
  [...visualSource.matchAll(/^import \* as ([A-Za-z_$][\w$]*) from "([^"]+)";$/gm)].map(
    (match) => [match[1], match[2]]
  )
);

test("every allowlisted specifier has a module, and every module is allowlisted", () => {
  assert.ok(allowlist.length > 0, "allowlist did not parse");
  assert.deepEqual([...moduleMap.keys()].sort(), [...allowlist].sort());
});

test("a specifier resolves to a whole module namespace, not a curated subset", () => {
  for (const [specifier, value] of moduleMap) {
    assert.ok(
      namespaceImports.has(value),
      `${specifier} resolves to \`${value}\`, which is not an \`import * as\` namespace — ` +
        "a hand-listed subset of a module's exports is a second allowlist nobody maintains"
    );
  }
});

test("MetricStrip and every other chrome barrel export is reachable", () => {
  const barrel = namespaceImports.get(moduleMap.get("@synth/visuals/chrome"));
  assert.ok(barrel, "@synth/visuals/chrome is not bound to a namespace import");
  const chrome = readFileSync(join(root, "runtime", barrel), "utf8");
  const exported = [
    ...chrome.matchAll(/^export (?:function|const|class) ([A-Za-z_$][\w$]*)/gm)
  ].map((match) => match[1]);
  // The barrel is the module the specifier names, so its exports are reachable
  // by construction. Naming the two the sourced kit advertises keeps the
  // regression that started this — `MetricStrip` allowlisted but unreachable —
  // from coming back through a renamed or re-homed barrel.
  assert.ok(exported.includes("VisualChrome"), `chrome barrel exports: ${exported}`);
  assert.ok(exported.includes("MetricStrip"), `chrome barrel exports: ${exported}`);
});
