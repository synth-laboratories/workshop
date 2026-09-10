#!/usr/bin/env node
// Managed-browser readiness probe.
//
// The host resolves one runtime and passes it in `SYNTH_BROWSER_RUNTIME_ROOT`,
// exactly as it does when launching `playwright_backend.mjs`. This file is the
// single definition of "is the runtime loadable", so Settings, the packaging
// tests and the backend can never disagree about which artifacts were checked.
//
// Always writes one JSON report to stdout and exits 0: a load failure is a
// result to report, not a crash to interpret.
import fs from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";

const require = createRequire(import.meta.url);
const report = { playwright: false, chromium: false };
try {
  const root = process.env.SYNTH_BROWSER_RUNTIME_ROOT;
  const specifier = root ? path.join(root, "node_modules", "playwright") : "playwright";
  report.specifier = specifier;
  const entry = require.resolve(specifier);
  const { chromium } = require(specifier);
  report.playwright = true;
  for (let dir = path.dirname(entry); dir !== path.dirname(dir); dir = path.dirname(dir)) {
    const manifest = path.join(dir, "package.json");
    if (fs.existsSync(manifest)) {
      report.version = JSON.parse(fs.readFileSync(manifest, "utf8")).version;
      break;
    }
  }
  const executable = chromium.executablePath();
  report.chromiumPath = executable;
  report.chromium = fs.existsSync(executable);
} catch (error) {
  report.error = String(error?.message ?? error);
}
process.stdout.write(JSON.stringify(report));
