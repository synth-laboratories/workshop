/**
 * sourced.visual.v1: agent TSX compiles and mounts in the Desktop pane.
 * Host-owned ingest. Unknown imports and fetch fail closed.
 */
import { expect, test } from "./browser.fixture";
import { installVisuals, liveVisual, openVisual, streamBinding } from "./v02-helpers";

const MARKER = "SOURCED-REWARD-3.1";

const happySource = `import { useState } from "react";
import { VisualChrome } from "@synth/visuals/chrome";
import { EventStream } from "@synth/visuals/components/event_stream.v1";
import { DetailModal } from "@synth/visuals/components/detail_modal.v1";
import { useLiveEvalStream } from "@synth/visuals/chrome/useLiveEvalStream";

export default function Shell(props) {
  const stream = props.stream && typeof props.stream === "object" ? props.stream : {};
  const declared = props.replay?.streams?.length ?? 0;
  const { events, state, error } = useLiveEvalStream({
    replay: props.replay,
    fixtureEvents: declared > 0 ? undefined : stream.events,
    visualId: props.visualId,
    revision: props.revision
  });
  const [selected, setSelected] = useState(null);
  const [cursorId, setCursorId] = useState(null);
  return (
    <VisualChrome kicker="Sourced" title={props.title ?? "Custom visual"} testId="visual-sourced">
      <EventStream
        events={events}
        state={state}
        error={error}
        includeKinds={["rollout.finished"]}
        cursorId={cursorId}
        onSelect={(event, identity) => { setSelected(event); setCursorId(identity); }}
      />
      <DetailModal event={selected} onClose={() => { setSelected(null); setCursorId(null); }} />
    </VisualChrome>
  );
}
`;

function sourcedVisual(id: string, title: string) {
	return liveVisual({
		id,
		templateId: "sourced.visual.v1",
		title,
		rendererKind: "tsx",
		contentDigest: `sha256:${id}`,
		bindings: streamBinding([
			{ ts: "2026-08-26T16:00:00.000Z", run_id: "sourced_run", kind: "stream.subscribed", sequence: null, payload: { ready: true } },
			{ ts: "2026-08-26T16:00:02.000Z", run_id: "sourced_run", kind: "rollout.finished", sequence: 2, payload: { marker: MARKER, reward: 3.1 } }
		])
	});
}

test("sourced visual compiles agent TSX, mounts advertised components, and hides control envelopes", async ({ page }) => {
	await installVisuals(page, [sourcedVisual("vis_sourced_proto", "Sourced prototype")], {
		vis_sourced_proto: happySource
	});
	const pane = await openVisual(page, "vis_sourced_proto");
	const viewer = pane.getByTestId("visual-sourced");
	await expect(viewer).toBeVisible();
	await expect(viewer.getByTestId("compose-event-stream")).toBeVisible();
	await expect(viewer.getByRole("button", { name: /rollout\.finished/ })).toBeVisible({ timeout: 20_000 });
	await expect(viewer).not.toContainText("stream.subscribed");
	await viewer.getByRole("button", { name: /rollout\.finished/ }).click();
	await expect(viewer.getByTestId("compose-detail-payload")).toContainText(MARKER);
});

test("sourced visual fails closed on an unknown import instead of mounting a shell", async ({ page }) => {
	await installVisuals(page, [sourcedVisual("vis_sourced_import", "Sourced unknown import")], {
		vis_sourced_import: `import _ from "lodash";
export default function Shell() { return <div data-testid="visual-sourced-bootleg">bootleg</div>; }
`
	});
	const pane = await openVisual(page, "vis_sourced_import");
	const viewer = pane.getByTestId("visual-sourced");
	await expect(viewer).toBeVisible();
	await expect(viewer.getByTestId("visual-sourced-invalid")).toContainText(/Unknown import "lodash"/);
	await expect(viewer.getByTestId("compose-event-stream")).toHaveCount(0);
	await expect(pane.getByTestId("visual-sourced-bootleg")).toHaveCount(0);
});

test("sourced visual fails closed on fetch instead of mounting a shell", async ({ page }) => {
	await installVisuals(page, [sourcedVisual("vis_sourced_fetch", "Sourced fetch")], {
		vis_sourced_fetch: `import { VisualChrome } from "@synth/visuals/chrome";
export default function Shell({ title }) {
  fetch("http://127.0.0.1:8298/rollouts/r1/events");
  return <VisualChrome title={title ?? "x"} testId="visual-sourced">leaked</VisualChrome>;
}
`
	});
	const pane = await openVisual(page, "vis_sourced_fetch");
	const viewer = pane.getByTestId("visual-sourced");
	await expect(viewer).toBeVisible();
	await expect(viewer.getByTestId("visual-sourced-invalid")).toContainText(/fetch/);
	await expect(viewer).not.toContainText("leaked");
	await expect(viewer.getByTestId("compose-event-stream")).toHaveCount(0);
});
