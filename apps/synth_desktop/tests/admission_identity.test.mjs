import assert from "node:assert/strict";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { transformSync } from "esbuild";
import test from "node:test";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });

const landingSource = join(appRoot, "src/renderer/src/types/landing.ts");
const compiled = join(compiledDir, "landing.mjs");
writeFileSync(
  compiled,
  transformSync(readFileSync(landingSource, "utf8"), {
    loader: "ts",
    format: "esm",
    target: "es2022"
  }).code
);
const { formatVisualAdmissionIdentity, classifyVisualOpsRoute, formatVisualOpsIdentity, formatVisualOpsPart } = await import(pathToFileURL(compiled).href);

test("admission identity labels receipt vs content vs missing digest", () => {
  assert.equal(
    formatVisualAdmissionIdentity({
      visualId: "vis_b38ea7d7",
      revision: 1,
      receiptDigest: "sha256:abcdef012345"
    }),
    "vis_b38ea7d7 · rev 1 · receipt sha256:a"
  );
  assert.equal(
    formatVisualAdmissionIdentity({
      visualId: "vis_b38ea7d7",
      revision: 2,
      contentDigest: "sha256:ffffffffffff"
    }),
    "vis_b38ea7d7 · rev 2 · content sha256:f"
  );
  assert.equal(
    formatVisualAdmissionIdentity({ visualId: "vis_blank", revision: 1 }),
    "vis_blank · rev 1 · digest —"
  );
});

test("attach/pin/seal chrome show vis_ identity and do not call live pointers Frozen", () => {
  const visualsPage = readFileSync(
    join(appRoot, "src/renderer/src/components/VisualsPage.tsx"),
    "utf8"
  );
  assert.match(visualsPage, /visual-add-to-report-identity/);
  assert.match(visualsPage, /formatVisualAdmissionIdentity/);
  assert.match(visualsPage, /visuals-card-identity-\$\{visual\.id\}/);
  assert.match(visualsPage, /sourceDigest: sealForRevision\?\.receiptDigest/);
  assert.match(visualsPage, /anchor: `visual-\$\{selected\.id\}`/);
  assert.doesNotMatch(visualsPage, /selected\.id\.slice\(0, 12\)/);

  const reportsPage = readFileSync(
    join(appRoot, "src/renderer/src/components/ReportsPage.tsx"),
    "utf8"
  );
  assert.match(reportsPage, /Live pointer/);
  assert.doesNotMatch(reportsPage, /Frozen evidence attached to this revision/);
  assert.match(reportsPage, /data-testid="reports-visual-pointer"/);
  assert.match(reportsPage, /data-testid="reports-pin-seal-identity"/);
  assert.match(reportsPage, /unresolved — not sealable/);

  const visualHost = readFileSync(
    join(appRoot, "src/renderer/src/components/VisualHost.tsx"),
    "utf8"
  );
  assert.match(visualHost, /visual-pane-identity/);
  assert.match(visualHost, /visual-pane-ops/);
  assert.match(visualHost, /sessionId: visual\.sessionId/);
  assert.match(visualHost, /runId: optimizerRunIdFromBindings\(visual\.bindings\) \?\? visual\.runId \?\? undefined/);
  assert.match(visualHost, /traceId: visual\.traceId/);

  const chat = readFileSync(
    join(appRoot, "src/renderer/src/components/ChatTranscript.tsx"),
    "utf8"
  );
  assert.match(chat, /formatVisualAdmissionIdentity/);
  assert.match(chat, /outputs-visual-identity-\$\{artifact\.id\}/);
  assert.match(chat, /artifact\.visualId \?\? artifact\.id/);
  assert.match(chat, /receiptDigest: artifact\.receiptDigest/);
  assert.match(chat, /contentDigest: artifact\.contentDigest/);
  assert.match(chat, /<code>\{report\.id\} · \{report\.status\}<\/code>/);

  const dataPage = readFileSync(
    join(appRoot, "src/renderer/src/components/DataPage.tsx"),
    "utf8"
  );
  assert.match(dataPage, /formatVisualAdmissionIdentity/);
  assert.match(dataPage, /inventory-visual-identity-\$\{v\.id\}/);
  assert.match(dataPage, /visual-ops-\$\{v\.id\}/);
  assert.match(dataPage, /visualId: v\.id/);
  assert.match(dataPage, /revision: v\.currentRevision/);
  assert.match(dataPage, /contentDigest: v\.contentDigest/);

  assert.match(visualsPage, /visual-ops-\$\{visual\.id\}/);

  const routes = readFileSync(
    join(appRoot, "src/renderer/src/routes.tsx"),
    "utf8"
  );
  assert.match(routes, /view\.kind === "reports"/);
  assert.match(routes, /settingsWithPane/);
  assert.match(routes, /onBack=\{leaveReports\}/);
  assert.match(routes, /leaveInventory/);
  assert.match(routes, /onBack=\{\(\) => leaveInventory\(inventoryOriginRef\.current\)\}/);
  assert.match(routes, /VISUAL_OPS_FOLLOW_EVENT/);
  assert.match(routes, /key="window-visual-host"/);
  assert.doesNotMatch(routes, /onBack=\{\(\) => openChat/);
  assert.doesNotMatch(routes, /setView\(\{ kind: "sync"/);
  assert.doesNotMatch(routes, /<CloudDesk\b/);

  const reader = readFileSync(
    join(appRoot, "src-tauri/src/reports/reader.js"),
    "utf8"
  );
  assert.match(reader, /Live pointer/);
  assert.doesNotMatch(reader, /Frozen evidence is attached to this revision/);
});

test("ops ids classify Workshop session vs not a Workshop route", () => {
  assert.equal(classifyVisualOpsRoute("session", null), "missing");
  assert.equal(classifyVisualOpsRoute("session", "chat_local", true), "workshop-session");
  assert.equal(classifyVisualOpsRoute("session", "intern_cloud", false), "not-a-workshop-route");
  assert.equal(classifyVisualOpsRoute("run", "run_1"), "optimizer-run");
  assert.equal(classifyVisualOpsRoute("run", "run_remote", false), "not-a-workshop-route");
  assert.equal(classifyVisualOpsRoute("trace", "tr_1"), "local-trace");
  assert.equal(
    formatVisualOpsPart("session", "chat_local", true),
    "session chat_local · Workshop session"
  );
  assert.equal(
    formatVisualOpsPart("session", "shoal_session", false),
    "session shoal_session · not a Workshop route"
  );
  assert.equal(
    formatVisualOpsIdentity({}),
    "session — · run — · trace —"
  );
});
