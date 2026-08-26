import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  SOURCED_ALLOWED_IMPORTS,
  SOURCED_TEMPLATE_ID,
  validateSourcedSource
} from "../runtime/sourcedValidate.ts";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

const validSource = `import { useState } from "react";
import { VisualChrome } from "@synth/visuals/chrome";
import { EventStream } from "@synth/visuals/components/event_stream.v1";
import { useLiveEvalStream } from "@synth/visuals/chrome/useLiveEvalStream";

export default function Shell(props) {
  const { events, state, error } = useLiveEvalStream({
    replay: props.replay,
    fixtureEvents: props.stream?.events,
    visualId: props.visualId,
    revision: props.revision
  });
  const [cursorId, setCursorId] = useState(null);
  return (
    <VisualChrome title={props.title ?? "Custom"} testId="visual-sourced">
      <EventStream events={events} state={state} error={error} cursorId={cursorId} onSelect={(_event, id) => setCursorId(id)} />
    </VisualChrome>
  );
}
`;

test("sourced.visual.v1 accepts allowlisted component imports", () => {
  const result = validateSourcedSource(validSource);
  assert.equal(result.ok, true);
  if (result.ok) {
    assert.ok(result.imports.includes("react"));
    assert.ok(result.imports.includes("@synth/visuals/components/event_stream.v1"));
  }
});

test("unknown imports fail closed", () => {
  const result = validateSourcedSource(`import _ from "lodash";\nexport default function Shell() { return null; }\n`);
  assert.equal(result.ok, false);
  if (!result.ok) assert.match(result.error, /Unknown import "lodash"/);
});

test("fetch and EventSource fail closed", () => {
  const fetchHit = validateSourcedSource(`import { VisualChrome } from "@synth/visuals/chrome";
export default function Shell() { fetch("/rollouts/r1/events"); return <VisualChrome title="x" testId="visual-sourced">x</VisualChrome>; }
`);
  assert.equal(fetchHit.ok, false);
  if (!fetchHit.ok) assert.match(fetchHit.error, /fetch/);

  const sseHit = validateSourcedSource(`import { VisualChrome } from "@synth/visuals/chrome";
export default function Shell() { new EventSource("https://example.test/stream"); return <VisualChrome title="x" testId="visual-sourced">x</VisualChrome>; }
`);
  assert.equal(sseHit.ok, false);
  if (!sseHit.ok) assert.match(sseHit.error, /EventSource/);
});

test("guessed /events URLs fail closed", () => {
  const result = validateSourcedSource(`import { VisualChrome } from "@synth/visuals/chrome";
const url = "http://127.0.0.1:8298/events";
export default function Shell() { return <VisualChrome title={url} testId="visual-sourced">x</VisualChrome>; }
`);
  assert.equal(result.ok, false);
  if (!result.ok) assert.match(result.error, /guess stream URL/);
});

test("empty source fails closed", () => {
  const result = validateSourcedSource("   ");
  assert.equal(result.ok, false);
  if (!result.ok) assert.match(result.error, /requires content/);
});

test("allowlist matches the advertised sourced kit", () => {
  assert.deepEqual([...SOURCED_ALLOWED_IMPORTS], [
    "react",
    "react/jsx-runtime",
    "react/jsx-dev-runtime",
    "react-dom",
    "@synth/visuals/chrome",
    "@synth/visuals/chrome/useLiveEvalStream",
    "@synth/visuals/components/event_stream.v1",
    "@synth/visuals/components/detail_modal.v1"
  ]);
  const template = JSON.parse(
    readFileSync(join(root, "families/analysis/sourced.visual.v1/template.json"), "utf8")
  );
  assert.equal(template.id, SOURCED_TEMPLATE_ID);
  assert.equal(template.kind, "sourced_visual");
  assert.equal(template.protocolId, "whole_file.v1");
  assert.equal(template.rendererKind, "tsx");
});
